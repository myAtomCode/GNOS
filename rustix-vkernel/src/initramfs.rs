use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::fixed::InlineString;
use crate::services::VfsService;

#[derive(Clone, Copy)]
pub struct InitramfsEntry {
    pub path: &'static str,
    pub data: &'static [u8],
    pub mode: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootMountSummary {
    pub mounted: bool,
    pub entries: usize,
    pub bytes: usize,
    pub recovered: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NewcSummary {
    pub valid: bool,
    pub entries: usize,
    pub bytes: usize,
}

static ROOT_MOUNTED: AtomicBool = AtomicBool::new(false);
static ROOT_ENTRIES: AtomicUsize = AtomicUsize::new(0);
static ROOT_BYTES: AtomicUsize = AtomicUsize::new(0);
static ROOT_RECOVERED: AtomicBool = AtomicBool::new(false);

static INIT: &[u8] = b"#!/bin/sh\necho Rustix initramfs\n";
static CONFIG: &[u8] = b"root=/dev/ram0\nrootfstype=tmpfs\nfsck.mode=auto\n";
static MOTD: &[u8] = b"Rustix initramfs root mounted\n";

const BUILTIN: [InitramfsEntry; 3] = [
    InitramfsEntry {
        path: "/init",
        data: INIT,
        mode: 0o755,
    },
    InitramfsEntry {
        path: "/etc/initramfs.conf",
        data: CONFIG,
        mode: 0o644,
    },
    InitramfsEntry {
        path: "/etc/motd.initramfs",
        data: MOTD,
        mode: 0o644,
    },
];

pub fn mount_root(vfs: &VfsService) -> RootMountSummary {
    if ROOT_MOUNTED.load(Ordering::Acquire) {
        return summary();
    }
    let mut entries = 0;
    let mut bytes = 0;
    for entry in BUILTIN {
        if vfs
            .create_file_with(entry.path, 0o644, 0, 0, 0)
            .is_ok()
            && vfs
                .write_text(entry.path, core::str::from_utf8(entry.data).unwrap_or(""))
                .is_ok()
            && vfs.chmod(entry.path, entry.mode).is_ok()
        {
            entries += 1;
            bytes += entry.data.len();
        }
    }
    ROOT_ENTRIES.store(entries, Ordering::Release);
    ROOT_BYTES.store(bytes, Ordering::Release);
    ROOT_RECOVERED.store(vfs.recover_consistency(), Ordering::Release);
    ROOT_MOUNTED.store(entries == BUILTIN.len(), Ordering::Release);
    summary()
}

pub fn summary() -> RootMountSummary {
    RootMountSummary {
        mounted: ROOT_MOUNTED.load(Ordering::Acquire),
        entries: ROOT_ENTRIES.load(Ordering::Acquire),
        bytes: ROOT_BYTES.load(Ordering::Acquire),
        recovered: ROOT_RECOVERED.load(Ordering::Acquire),
    }
}

pub fn parse_newc(archive: &[u8]) -> NewcSummary {
    let mut offset = 0usize;
    let mut entries = 0usize;
    let mut bytes = 0usize;
    let mut trailer = false;
    while offset + 110 <= archive.len() {
        if &archive[offset..offset + 6] != b"070701" {
            break;
        }
        let size = parse_hex(&archive[offset + 54..offset + 62]);
        let name_size = parse_hex(&archive[offset + 94..offset + 102]);
        let name_start = offset + 110;
        let name_end = name_start.saturating_add(name_size);
        let data_start = (name_end + 3) & !3;
        let data_end = data_start.saturating_add(size);
        if name_end > archive.len() || data_end > archive.len() || name_size == 0 {
            return NewcSummary {
                valid: false,
                entries,
                bytes,
            };
        }
        let name = &archive[name_start..name_end.saturating_sub(1)];
        if name == b"TRAILER!!!" {
            trailer = true;
            break;
        }
        entries += 1;
        bytes = bytes.saturating_add(size);
        offset = (data_end + 3) & !3;
    }
    NewcSummary {
        valid: trailer,
        entries,
        bytes,
    }
}

pub fn unpack_newc(vfs: &VfsService, archive: &[u8]) -> NewcSummary {
    let mut offset = 0usize;
    let mut entries = 0usize;
    let mut bytes = 0usize;
    while offset + 110 <= archive.len() {
        if &archive[offset..offset + 6] != b"070701" {
            break;
        }
        let mode = parse_hex(&archive[offset + 14..offset + 22]) as u16;
        let size = parse_hex(&archive[offset + 54..offset + 62]);
        let name_size = parse_hex(&archive[offset + 94..offset + 102]);
        let name_start = offset + 110;
        let name_end = name_start.saturating_add(name_size);
        let data_start = (name_end + 3) & !3;
        let data_end = data_start.saturating_add(size);
        if name_end > archive.len() || data_end > archive.len() || name_size == 0 {
            return NewcSummary {
                valid: false,
                entries,
                bytes,
            };
        }
        let name = &archive[name_start..name_end.saturating_sub(1)];
        if name == b"TRAILER!!!" {
            return NewcSummary {
                valid: true,
                entries,
                bytes,
            };
        }
        let Ok(name) = core::str::from_utf8(name) else {
            return NewcSummary {
                valid: false,
                entries,
                bytes,
            };
        };
        let mut path = InlineString::<64>::new();
        if path.push_byte(b'/').is_err()
            || path.push_str(name.trim_start_matches('/')).is_err()
            || ensure_parents(vfs, path.as_str()).is_err()
        {
            return NewcSummary {
                valid: false,
                entries,
                bytes,
            };
        }
        let file_type = mode & 0xf000;
        let result = if file_type == 0x4000 {
            vfs.create_dir_with(path.as_str(), mode & 0o7777, 0, 0, 0)
                .or_else(|_| {
                    if vfs.is_directory(path.as_str()) {
                        Ok(())
                    } else {
                        Err(crate::linux::Errno::EEXIST)
                    }
                })
        } else if file_type == 0x8000 && size <= crate::services::MAX_FILE_BYTES {
            vfs.create_file_with(path.as_str(), 0o600, 0, 0, 0)
                .and_then(|_| {
                    let text = core::str::from_utf8(&archive[data_start..data_end])
                        .map_err(|_| crate::linux::Errno::EINVAL)?;
                    vfs.write_text(path.as_str(), text)
                })
                .and_then(|_| vfs.chmod(path.as_str(), mode & 0o7777))
        } else {
            Err(crate::linux::Errno::EOPNOTSUPP)
        };
        if result.is_err() {
            return NewcSummary {
                valid: false,
                entries,
                bytes,
            };
        }
        entries += 1;
        bytes = bytes.saturating_add(size);
        offset = (data_end + 3) & !3;
    }
    NewcSummary {
        valid: false,
        entries,
        bytes,
    }
}

fn ensure_parents(vfs: &VfsService, path: &str) -> crate::linux::Result<()> {
    let mut parent = InlineString::<64>::new();
    let parent_path = path.rsplit_once('/').map_or("", |(parent, _)| parent);
    for component in parent_path.trim_matches('/').split('/') {
        if component.is_empty() {
            continue;
        }
        parent.push_byte(b'/').map_err(|_| crate::linux::Errno::ENAMETOOLONG)?;
        parent
            .push_str(component)
            .map_err(|_| crate::linux::Errno::ENAMETOOLONG)?;
        if !vfs.is_directory(parent.as_str()) {
            vfs.create_dir_with(parent.as_str(), 0o755, 0, 0, 0)?;
        }
    }
    Ok(())
}

fn parse_hex(bytes: &[u8]) -> usize {
    bytes.iter().fold(0usize, |value, byte| {
        value.saturating_mul(16).saturating_add(match byte {
            b'0'..=b'9' => usize::from(byte - b'0'),
            b'a'..=b'f' => usize::from(byte - b'a' + 10),
            b'A'..=b'F' => usize::from(byte - b'A' + 10),
            _ => 0,
        })
    })
}
