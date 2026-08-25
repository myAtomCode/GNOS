use core::fmt::Write;
use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};

use crate::fixed::InlineString;
use crate::linux::path;
use crate::linux::{Errno, Result as LinuxResult};
use crate::sync::SpinMutex;

pub const MAX_DIR_ENTRIES: usize = 24;
const MAX_USER_FILES: usize = 12;
const MAX_USER_DIRS: usize = 12;
const MAX_FILE_TEXT: usize = 2048;
const MAX_PATH_LEN: usize = 64;
const MAX_SYMLINKS: usize = 8;
const MAX_USER_LINKS: usize = 12;
const CACHE_SLOTS: usize = 16;
const CACHE_PAGE_BYTES: usize = 4096;
pub const MAX_FILE_BYTES: usize = MAX_FILE_TEXT;
pub const MAX_VFS_PATH: usize = MAX_PATH_LEN;
pub const ROOT_UID: u32 = 0;
pub const ROOT_GID: u32 = 0;
pub const DEFAULT_UMASK: u16 = 0o022;
pub const MODE_REGULAR: u16 = 0o100000;
pub const MODE_DIRECTORY: u16 = 0o040000;
pub const MODE_CHAR_DEVICE: u16 = 0o020000;
pub const MODE_SYMLINK: u16 = 0o120000;
pub const PERM_READ: u8 = 0b100;
pub const PERM_WRITE: u8 = 0b010;
pub const PERM_EXEC: u8 = 0b001;

static PROCESS_UMASK: AtomicU16 = AtomicU16::new(DEFAULT_UMASK);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InodeKind {
    Directory,
    Regular,
    Symlink,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SuperBlock {
    pub id: u32,
    pub name: &'static str,
    pub root_inode: u64,
    pub read_only: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mount {
    pub mountpoint: &'static str,
    pub superblock: u32,
    pub root_inode: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Inode {
    pub number: u64,
    pub kind: InodeKind,
    pub size: u64,
    pub links: u32,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Clone, Copy)]
pub struct Dentry {
    pub parent: u64,
    pub inode: Inode,
    pub name: InlineString<32>,
}

#[derive(Clone, Copy)]
pub struct FileEntry {
    pub path: &'static str,
    pub text: &'static str,
    pub executable: bool,
}

#[derive(Clone, Copy)]
struct UserFile {
    used: bool,
    inode: u64,
    executable: bool,
    mode: u16,
    uid: u32,
    gid: u32,
    path: InlineString<64>,
    data: [u8; MAX_FILE_BYTES],
    len: usize,
    dirty_data: bool,
    dirty_metadata: bool,
    synced_generation: u64,
}

impl UserFile {
    const fn new() -> Self {
        Self {
            used: false,
            inode: 0,
            executable: false,
            mode: 0,
            uid: ROOT_UID,
            gid: ROOT_GID,
            path: InlineString::new(),
            data: [0; MAX_FILE_BYTES],
            len: 0,
            dirty_data: false,
            dirty_metadata: false,
            synced_generation: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct DirEntries {
    items: [InlineString<32>; MAX_DIR_ENTRIES],
    len: usize,
}

impl DirEntries {
    pub const fn new() -> Self {
        Self {
            items: [InlineString::new(); MAX_DIR_ENTRIES],
            len: 0,
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn push_unique(&mut self, item: &str) {
        for existing in &self.items[..self.len] {
            if existing.as_str() == item {
                return;
            }
        }

        if self.len < self.items.len() {
            self.items[self.len].set(item);
            self.len += 1;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.items[..self.len].iter().map(|entry| entry.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn get(&self, index: usize) -> Option<&str> {
        self.items.get(index).map(InlineString::as_str)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileMetadata {
    pub size: u64,
    pub directory: bool,
    pub executable: bool,
    pub char_device: bool,
    pub inode: u64,
    pub links: u32,
    pub symlink: bool,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
}

impl FileMetadata {
    pub const fn file_type(self) -> u16 {
        if self.symlink {
            MODE_SYMLINK
        } else if self.directory {
            MODE_DIRECTORY
        } else if self.char_device {
            MODE_CHAR_DEVICE
        } else {
            MODE_REGULAR
        }
    }

    pub const fn stat_mode(self) -> u16 {
        self.file_type() | (self.mode & 0o7777)
    }

    pub fn allows(self, uid: u32, gid: u32, permission: u8) -> bool {
        if uid == ROOT_UID {
            return permission != PERM_EXEC || self.mode & 0o111 != 0 || self.directory;
        }
        let shift = if uid == self.uid {
            6
        } else if gid == self.gid {
            3
        } else {
            0
        };
        ((self.mode >> shift) as u8 & permission) == permission
    }
}

#[derive(Clone, Copy)]
struct UserDir {
    used: bool,
    path: InlineString<64>,
    mode: u16,
    uid: u32,
    gid: u32,
}

impl UserDir {
    const fn new() -> Self {
        Self {
            used: false,
            path: InlineString::new(),
            mode: 0,
            uid: ROOT_UID,
            gid: ROOT_GID,
        }
    }
}

#[derive(Clone, Copy)]
struct CacheEntry {
    valid: bool,
    inode: u64,
    index: u64,
}

impl CacheEntry {
    const fn empty() -> Self {
        Self {
            valid: false,
            inode: 0,
            index: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct PageCacheEntry {
    valid: bool,
    dirty: bool,
    inode: u64,
    index: u64,
    length: usize,
    generation: u64,
    data: [u8; CACHE_PAGE_BYTES],
}

impl PageCacheEntry {
    const fn empty() -> Self {
        Self {
            valid: false,
            dirty: false,
            inode: 0,
            index: 0,
            length: 0,
            generation: 0,
            data: [0; CACHE_PAGE_BYTES],
        }
    }
}

struct CacheState {
    page: [PageCacheEntry; CACHE_SLOTS],
    block: [CacheEntry; CACHE_SLOTS],
    next_page: usize,
    next_block: usize,
}

impl CacheState {
    const fn new() -> Self {
        Self {
            page: [PageCacheEntry::empty(); CACHE_SLOTS],
            block: [CacheEntry::empty(); CACHE_SLOTS],
            next_page: 0,
            next_block: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheSnapshot {
    pub page_hits: u64,
    pub page_misses: u64,
    pub block_hits: u64,
    pub block_misses: u64,
    pub page_entries: usize,
    pub block_entries: usize,
    pub dirty_pages: usize,
    pub readahead_pages: u64,
    pub writeback_pages: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncSnapshot {
    pub write_generation: u64,
    pub synced_generation: u64,
    pub fsync_count: u64,
    pub recovery_count: u64,
    pub dirty_files: usize,
}

#[derive(Clone, Copy)]
struct UserLink {
    used: bool,
    symlink: bool,
    path: InlineString<64>,
    target: InlineString<64>,
}

impl UserLink {
    const fn new() -> Self {
        Self {
            used: false,
            symlink: false,
            path: InlineString::new(),
            target: InlineString::new(),
        }
    }
}

const STATIC_FILES: [FileEntry; 20] = [
    FileEntry {
        path: "/README.txt",
        text: "Rustix v-kernel bootable microkernel prototype\nservices: console, vfs, proc\n",
        executable: false,
    },
    FileEntry {
        path: "/etc/motd",
        text: "Welcome to Rustix v-kernel.\nEverything here goes through services and IPC.\n",
        executable: false,
    },
    FileEntry {
        path: "/bin/hello",
        text: "Microkernel hello app. Run with: exec /bin/hello\n",
        executable: true,
    },
    FileEntry {
        path: "/bin/game",
        text: "Microkernel ASCII dungeon. Run with: exec /bin/game\n",
        executable: true,
    },
    FileEntry {
        path: "/bin/echo",
        text: "Linux-style echo utility. Run with: exec /bin/echo hello\n",
        executable: true,
    },
    FileEntry {
        path: "/bin/calc",
        text: "Tiny calculator. Run with: exec /bin/calc 12 + 30\n",
        executable: true,
    },
    FileEntry {
        path: "/bin/settings",
        text: "Linux-style settings manager. Run with: settings show\n",
        executable: true,
    },
    FileEntry {
        path: "/bin/video-player",
        text: "Safe demo video player. Run with: video-player /usr/share/media/demo-video.rvf\n",
        executable: true,
    },
    FileEntry {
        path: "/bin/audio-player",
        text: "Safe demo audio player. Run with: audio-player /usr/share/media/demo-audio.rta\n",
        executable: true,
    },
    FileEntry {
        path: "/usr/share/ipc.txt",
        text: "IPC model\n- shell asks proc to exec\n- apps ask vfs for files\n- apps ask console for I/O\n",
        executable: false,
    },
    FileEntry {
        path: "/etc/rustix/settings.conf",
        text: "brightness=72\nvolume=58\nui_scale=1\ntheme=graphite\n",
        executable: false,
    },
    FileEntry {
        path: "/usr/share/media/demo-video.rvf",
        text: "title=Rustix Demo Video\nfps=2\n\n+--------------------+\n|  RUSTIX MICROKERN  |\n|                    |\n|     BOOT STAGE     |\n|        [==]        |\n+--------------------+\n---\n+--------------------+\n|  RUSTIX MICROKERN  |\n|                    |\n|    SERVICE IPC     |\n|      [====]        |\n+--------------------+\n---\n+--------------------+\n|  RUSTIX MICROKERN  |\n|                    |\n|     /BIN APPS      |\n|     [======]       |\n+--------------------+\n---\n+--------------------+\n|  RUSTIX MICROKERN  |\n|                    |\n|   VIDEO READY :)   |\n|    [========]      |\n+--------------------+\n",
        executable: false,
    },
    FileEntry {
        path: "/usr/share/media/demo-audio.rta",
        text: "title=Rustix Startup Tune\nvolume=58\n\n880 120 A5\n988 120 B5\n1047 120 C6\n0 60 REST\n1047 120 C6\n1175 120 D6\n1319 180 E6\n",
        executable: false,
    },
    FileEntry {
        path: "/home/notes.txt",
        text: "Rustix notes\nEdit this file from the GUI editor.\n",
        executable: false,
    },
    FileEntry {
        path: "/home/todo.txt",
        text: "- build microkernel services\n- improve GUI responsiveness\n- keep Linux-facing paths stable\n",
        executable: false,
    },
    FileEntry {
        path: "/usr/share/man/rustix.txt",
        text: "Rustix userland notes\n- ls, cat, touch, mkdir follow Linux path conventions\n- GUI editor opens files from the file manager\n",
        executable: false,
    },
    FileEntry {
        path: "/usr/share/man/settings.txt",
        text: "settings(1)\nsettings show\nsettings get <key>\nsettings set <brightness|volume|ui_scale|theme> <value>\n",
        executable: false,
    },
    FileEntry {
        path: "/usr/share/man/video-player.txt",
        text: "video-player(1)\nvideo-player [path]\nDefault path: /usr/share/media/demo-video.rvf\n",
        executable: false,
    },
    FileEntry {
        path: "/usr/share/man/audio-player.txt",
        text: "audio-player(1)\naudio-player [path]\nDefault path: /usr/share/media/demo-audio.rta\n",
        executable: false,
    },
    FileEntry {
        path: "/etc/init.d/rcS",
        text: "#!/bin/sh\nprintf \"[busybox] abi=ok\\n\"\necho \"===== T2: coreutils =====\"\n/bin/ls /\necho \"ls_rc=$?\"\n/bin/cat /etc/motd.initramfs\necho \"cat_rc=$?\"\necho \"===== T0: fwtest =====\"\n/bin/fwtest\necho \"fw_rc=$?\"\necho \"===== T3: curl =====\"\n/bin/curl -s -m 5 http://10.0.2.2:8000/hello.txt\necho \"curl_rc=$?\"\necho \"===== T1: bash =====\"\n/bin/bash -c 'echo BASH_OK version=$BASH_VERSION'\necho \"bash_rc=$?\"\necho \"===== T4: fastfetch =====\"\n/bin/fastfetch\necho \"ff_rc=$?\"\necho \"===== ALL TESTS DONE =====\"\nwhile true; do :; done\n",
        executable: true,
    },
];

const DYNAMIC_FILES: [&str; 13] = [
    "/proc/services",
    "/proc/tasks",
    "/proc/ipc",
    "/proc/version",
    "/proc/mounts",
    "/proc/filesystems",
    "/proc/cache",
    "/sys/kernel/umask",
    "/sys/kernel/cache",
    "/proc/storage",
    "/sys/block/devices",
    "/proc/mounts.initramfs",
    "/sys/fs/sync",
];

const DEVICE_FILES: [&str; 4] = ["/dev/null", "/dev/zero", "/dev/console", "/dev/random"];

const SUPERBLOCKS: [SuperBlock; 5] = [
    SuperBlock {
        id: 1,
        name: "initramfs",
        root_inode: 1,
        read_only: false,
    },
    SuperBlock {
        id: 2,
        name: "tmpfs",
        root_inode: 2,
        read_only: false,
    },
    SuperBlock {
        id: 3,
        name: "devfs",
        root_inode: 3,
        read_only: false,
    },
    SuperBlock {
        id: 4,
        name: "procfs",
        root_inode: 4,
        read_only: true,
    },
    SuperBlock {
        id: 5,
        name: "sysfs",
        root_inode: 5,
        read_only: true,
    },
];

const MOUNTS: [Mount; 5] = [
    Mount {
        mountpoint: "/",
        superblock: 1,
        root_inode: 1,
    },
    Mount {
        mountpoint: "/tmp",
        superblock: 2,
        root_inode: 2,
    },
    Mount {
        mountpoint: "/dev",
        superblock: 3,
        root_inode: 3,
    },
    Mount {
        mountpoint: "/proc",
        superblock: 4,
        root_inode: 4,
    },
    Mount {
        mountpoint: "/sys",
        superblock: 5,
        root_inode: 5,
    },
];

struct UserStorage {
    files: [UserFile; MAX_USER_FILES],
    links: [UserLink; MAX_USER_LINKS],
    directories: [UserDir; MAX_USER_DIRS],
}

impl UserStorage {
    const fn new() -> Self {
        Self {
            files: [UserFile::new(); MAX_USER_FILES],
            links: [UserLink::new(); MAX_USER_LINKS],
            directories: [UserDir::new(); MAX_USER_DIRS],
        }
    }
}

static USER_STORAGE: SpinMutex<UserStorage> = SpinMutex::new(UserStorage::new());
static CACHE_STATE: SpinMutex<CacheState> = SpinMutex::new(CacheState::new());
static PAGE_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static PAGE_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
static BLOCK_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static BLOCK_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
static READAHEAD_PAGES: AtomicU64 = AtomicU64::new(0);
static WRITEBACK_PAGES: AtomicU64 = AtomicU64::new(0);
static WRITE_GENERATION: AtomicU64 = AtomicU64::new(1);
static SYNCED_GENERATION: AtomicU64 = AtomicU64::new(0);
static FSYNC_COUNT: AtomicU64 = AtomicU64::new(0);
static RECOVERY_COUNT: AtomicU64 = AtomicU64::new(0);

pub struct ConsoleService;

impl ConsoleService {
    pub const fn new() -> Self {
        Self
    }

    pub fn write(&mut self, text: &str) {
        crate::arch::console_write(text);
    }

    pub fn write_line(&mut self, text: &str) {
        self.write(text);
        self.write("\n");
    }

    pub fn prompt<'a>(
        &mut self,
        prompt: &str,
        buffer: &'a mut InlineString<128>,
        mut on_idle: impl FnMut(),
    ) -> &'a str {
        self.write(prompt);
        crate::gui::sync_console();

        loop {
            on_idle();
            let mut processed = false;
            while let Some(input) = crate::arch::poll_console_input() {
                processed = true;
                match input {
                    crate::arch::ConsoleInput::MouseByte(byte) => {
                        crate::gui::handle_mouse_byte(byte);
                    }
                    crate::arch::ConsoleInput::Key { code, translated } => {
                        if crate::gui::handle_key(code, translated) {
                            continue;
                        }
                        if self.handle_prompt_byte(buffer, translated) {
                            return buffer.as_str();
                        }
                    }
                    crate::arch::ConsoleInput::Byte(byte) => {
                        if self.handle_prompt_byte(buffer, Some(byte)) {
                            return buffer.as_str();
                        }
                    }
                }
            }
            crate::gui::flush_mouse();

            if !processed {
                crate::gui::tick_idle();
                crate::arch::delay(1_000);
            }
        }
    }

    fn handle_prompt_byte(&mut self, buffer: &mut InlineString<128>, byte: Option<u8>) -> bool {
        let Some(byte) = byte else {
            return false;
        };
        match byte {
            b'\n' => {
                self.write("\n");
                crate::gui::sync_console();
                true
            }
            0x08 => {
                if buffer.pop().is_some() {
                    self.write(crate::arch::backspace_echo());
                    crate::gui::sync_console();
                }
                false
            }
            byte if byte.is_ascii_control() => false,
            _ => {
                if buffer.push_byte(byte).is_ok() {
                    let scratch = [byte];
                    if let Ok(text) = core::str::from_utf8(&scratch) {
                        self.write(text);
                        crate::gui::sync_console();
                    }
                }
                false
            }
        }
    }
}

pub struct VfsService;

impl VfsService {
    pub const fn new() -> Self {
        Self
    }

    pub fn superblocks(&self) -> &'static [SuperBlock] {
        &SUPERBLOCKS
    }

    pub fn mounts(&self) -> &'static [Mount] {
        &MOUNTS
    }

    pub fn lookup(&self, path: &str) -> LinuxResult<Dentry> {
        let mut raw = InlineString::<MAX_PATH_LEN>::new();
        path::normalize_absolute(path, &mut raw)?;
        {
            let storage = USER_STORAGE.lock();
            if let Some(link) = storage
                .links
                .iter()
                .find(|link| link.used && link.symlink && link.path.as_str() == raw.as_str())
            {
                let mut name = InlineString::<32>::new();
                name.set(raw.as_str().rsplit('/').next().unwrap_or(""));
                return Ok(Dentry {
                    parent: stable_inode(parent_path(raw.as_str())),
                    inode: Inode {
                        number: stable_inode(raw.as_str()),
                        kind: InodeKind::Symlink,
                        size: link.target.len() as u64,
                        links: 1,
                        mode: MODE_SYMLINK | 0o777,
                        uid: ROOT_UID,
                        gid: ROOT_GID,
                    },
                    name,
                });
            }
        }
        let mut resolved = InlineString::<MAX_PATH_LEN>::new();
        let path = self.resolve_path("/", "/", path, &mut resolved)?;
        let metadata = self.metadata(path)?;
        let mut name = InlineString::<32>::new();
        name.set(path.rsplit('/').next().unwrap_or(""));
        Ok(Dentry {
            parent: stable_inode(parent_path(path)),
            inode: Inode {
                number: metadata.inode,
                kind: if metadata.symlink {
                    InodeKind::Symlink
                } else if metadata.directory {
                    InodeKind::Directory
                } else {
                    InodeKind::Regular
                },
                size: metadata.size,
                links: metadata.links,
                mode: metadata.stat_mode(),
                uid: metadata.uid,
                gid: metadata.gid,
            },
            name,
        })
    }

    pub fn resolve_path<'a>(
        &self,
        root: &str,
        cwd: &str,
        input: &str,
        output: &'a mut InlineString<MAX_PATH_LEN>,
    ) -> LinuxResult<&'a str> {
        path::normalize_in_root(root, cwd, input, output)?;
        self.resolve_links(root, output)
    }

    fn resolve_links<'a>(
        &self,
        root: &str,
        output: &'a mut InlineString<MAX_PATH_LEN>,
    ) -> LinuxResult<&'a str> {
        let mut current = InlineString::<MAX_PATH_LEN>::new();
        current.set(output.as_str());
        for _ in 0..MAX_SYMLINKS {
            let mut link_path = InlineString::<MAX_PATH_LEN>::new();
            let mut target = InlineString::<MAX_PATH_LEN>::new();
            let mut is_symlink = false;
            {
                let storage = USER_STORAGE.lock();
                let mut best = 0;
                for link in &storage.links {
                    if !link.used || !is_link_prefix(link.path.as_str(), current.as_str()) {
                        continue;
                    }
                    if link.path.len() > best {
                        best = link.path.len();
                        link_path.set(link.path.as_str());
                        target.set(link.target.as_str());
                        is_symlink = link.symlink;
                    }
                }
            }
            if link_path.is_empty() {
                output.set(current.as_str());
                return Ok(output.as_str());
            }
            let mut suffix = InlineString::<MAX_PATH_LEN>::new();
            suffix.set(&current.as_str()[link_path.len()..]);
            if !is_symlink {
                current.set(target.as_str());
                current
                    .push_str(suffix.as_str())
                    .map_err(|_| Errno::NameTooLong)?;
                let inside_root = root == "/"
                    || current.as_str() == root
                    || current
                        .as_str()
                        .strip_prefix(root)
                        .is_some_and(|suffix| suffix.starts_with('/'));
                if !inside_root {
                    return Err(Errno::EACCES);
                }
                continue;
            }
            let mut next_input = InlineString::<MAX_PATH_LEN>::new();
            let relative_target = !target.as_str().starts_with('/');
            next_input
                .push_str(target.as_str())
                .map_err(|_| Errno::NameTooLong)?;
            next_input
                .push_str(suffix.as_str())
                .map_err(|_| Errno::NameTooLong)?;
            let base = if relative_target {
                parent_path(link_path.as_str())
            } else {
                root
            };
            let mut next = InlineString::<MAX_PATH_LEN>::new();
            path::normalize_in_root(root, base, next_input.as_str(), &mut next)?;
            current = next;
        }
        Err(Errno::ELOOP)
    }

    pub fn namespace_self_test(&self) -> bool {
        let directory = "/home/.vfs-selftest";
        let source = "/home/.vfs-selftest/source";
        let hard = "/home/.vfs-selftest/hard";
        let soft = "/home/.vfs-selftest/soft";
        if self.superblocks().len() != 5
            || self.mounts().len() != 5
            || self.mounts()[1].mountpoint != "/tmp"
            || self.mounts()[2].mountpoint != "/dev"
            || self.mounts()[3].mountpoint != "/proc"
            || self.mounts()[4].mountpoint != "/sys"
        {
            return false;
        }
        let Ok(tmp_metadata) = self.metadata("/tmp") else {
            return false;
        };
        let Ok(null_metadata) = self.metadata("/dev/null") else {
            return false;
        };
        if tmp_metadata.mode != 0o1777
            || !null_metadata.char_device
            || !self.path_exists("/proc/mounts")
            || !self.path_exists("/sys/kernel/cache")
        {
            return false;
        }
        if self.create_dir(directory).is_err() && !self.directory_exists(directory) {
            return false;
        }
        if self.write_text(source, "namespace-ok\n").is_err() {
            return false;
        }
        let Ok(source_metadata) = self.metadata(source) else {
            return false;
        };
        if source_metadata.mode != 0o644
            || source_metadata.uid != ROOT_UID
            || source_metadata.gid != ROOT_GID
        {
            return false;
        }
        let cache_before = self.cache_snapshot();
        let mut cached = [0u8; 16];
        if self.read_at(source, 0, &mut cached).is_err()
            || self.read_at(source, 0, &mut cached).is_err()
        {
            return false;
        }
        let cache_after = self.cache_snapshot();
        if cache_after.page_hits <= cache_before.page_hits
            || cache_after.page_misses <= cache_before.page_misses
            || cache_after.block_hits <= cache_before.block_hits
            || cache_after.block_misses <= cache_before.block_misses
        {
            return false;
        }
        if self.write_at(source, 0, b"namespace-ok\n").is_err() {
            return false;
        }
        let dirty_cache = self.cache_snapshot();
        if dirty_cache.dirty_pages == 0 || self.fsync_path(source).is_err() {
            return false;
        }
        let clean_cache = self.cache_snapshot();
        if clean_cache.dirty_pages >= dirty_cache.dirty_pages
            || clean_cache.writeback_pages <= dirty_cache.writeback_pages
        {
            return false;
        }
        if self.hard_link(source, hard).is_err() && !raw_path_exists(hard) {
            return false;
        }
        if self.symlink("source", soft).is_err() && !raw_path_exists(soft) {
            return false;
        }
        let mut text = InlineString::<MAX_FILE_TEXT>::new();
        if self.read_file(soft, &mut text) != Some("namespace-ok\n") {
            return false;
        }
        let mut escaped = InlineString::<MAX_PATH_LEN>::new();
        if self
            .resolve_path(directory, directory, "../../etc/motd", &mut escaped)
            .is_err()
            || escaped.as_str() != "/home/.vfs-selftest/etc/motd"
        {
            return false;
        }
        let Ok(source_dentry) = self.lookup(source) else {
            return false;
        };
        let Ok(hard_dentry) = self.lookup(hard) else {
            return false;
        };
        let Ok(soft_dentry) = self.lookup(soft) else {
            return false;
        };
        let mut target = InlineString::<MAX_PATH_LEN>::new();
        source_dentry.inode.number == hard_dentry.inode.number
            && source_dentry.inode.links >= 2
            && soft_dentry.inode.kind == InodeKind::Symlink
            && self.read_link(soft, &mut target).is_ok()
            && target.as_str() == "source"
    }

    pub fn read_link(
        &self,
        path: &str,
        output: &mut InlineString<MAX_PATH_LEN>,
    ) -> LinuxResult<()> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        path::normalize_absolute(path, &mut normalized)?;
        let storage = USER_STORAGE.lock();
        let link = storage
            .links
            .iter()
            .find(|link| link.used && link.symlink && link.path.as_str() == normalized.as_str())
            .ok_or(Errno::EINVAL)?;
        output.set(link.target.as_str());
        Ok(())
    }

    pub fn symlink(&self, target: &str, path: &str) -> LinuxResult<()> {
        let mut link_path = InlineString::<MAX_PATH_LEN>::new();
        path::normalize_absolute(path, &mut link_path)?;
        if target.is_empty() || target.len() >= MAX_PATH_LEN || raw_path_exists(link_path.as_str())
        {
            return Err(Errno::EEXIST);
        }
        if !self.directory_exists(parent_path(link_path.as_str())) {
            return Err(Errno::ENOENT);
        }
        let mut storage = USER_STORAGE.lock();
        let link = storage
            .links
            .iter_mut()
            .find(|link| !link.used)
            .ok_or(Errno::ENOSPC)?;
        link.used = true;
        link.symlink = true;
        link.path = link_path;
        link.target.set(target);
        Ok(())
    }

    pub fn hard_link(&self, old: &str, new: &str) -> LinuxResult<()> {
        let mut old_path = InlineString::<MAX_PATH_LEN>::new();
        self.resolve_path("/", "/", old, &mut old_path)?;
        if self.is_directory(old_path.as_str()) {
            return Err(Errno::EPERM);
        }
        let mut new_path = InlineString::<MAX_PATH_LEN>::new();
        path::normalize_absolute(new, &mut new_path)?;
        if raw_path_exists(new_path.as_str()) {
            return Err(Errno::EEXIST);
        }
        if !self.directory_exists(parent_path(new_path.as_str())) {
            return Err(Errno::ENOENT);
        }
        let mut storage = USER_STORAGE.lock();
        let link = storage
            .links
            .iter_mut()
            .find(|link| !link.used)
            .ok_or(Errno::ENOSPC)?;
        link.used = true;
        link.symlink = false;
        link.path = new_path;
        link.target = old_path;
        Ok(())
    }

    pub fn read_static_file(&self, path: &str) -> Option<&'static str> {
        for entry in STATIC_FILES {
            if entry.path == path {
                return Some(entry.text);
            }
        }
        None
    }

    pub fn read_file<'a>(
        &self,
        path: &str,
        scratch: &'a mut InlineString<MAX_FILE_TEXT>,
    ) -> Option<&'a str> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized).ok()?;
        scratch.clear();
        if user_file_exists(path) {
            let mut bytes = [0u8; MAX_FILE_TEXT];
            let length = self.read_at(path, 0, &mut bytes).ok()?;
            let text = core::str::from_utf8(&bytes[..length]).ok()?;
            let _ = scratch.push_str(text);
            return Some(scratch.as_str());
        }
        if path == "/dev/null" || path == "/dev/console" {
            return Some(scratch.as_str());
        }

        let static_text = match path {
            "/proc/version" => {
                let _ = scratch.push_str("Rustix v-kernel 0.1.0 ");
                let _ = scratch.push_str(crate::arch::arch_name());
                let _ = scratch.push_str("\n");
                return Some(scratch.as_str());
            }
            "/proc/services" => {
                Some("Dynamic proc file. Use `services` in CMD for the live service table.\n")
            }
            "/proc/tasks" => Some("Dynamic proc file. Use `ps` in CMD for the live task table.\n"),
            "/proc/ipc" => Some("Dynamic proc file. Use `ipc` in CMD for the live IPC trace.\n"),
            "/proc/mounts" => {
                for mount in MOUNTS {
                    let superblock = SUPERBLOCKS
                        .iter()
                        .find(|superblock| superblock.id == mount.superblock)?;
                    let _ = writeln!(
                        scratch,
                        "{} {} {} {} 0 0",
                        superblock.name,
                        mount.mountpoint,
                        superblock.name,
                        if superblock.read_only { "ro" } else { "rw" }
                    );
                }
                return Some(scratch.as_str());
            }
            "/proc/filesystems" => {
                Some("nodev\trootfs\nnodev\ttmpfs\nnodev\tdevfs\nnodev\tprocfs\nnodev\tsysfs\n\text2\n\text4\n")
            }
            "/proc/cache" | "/sys/kernel/cache" => {
                let cache = self.cache_snapshot();
                let _ = writeln!(
                    scratch,
                    "page_cache entries={} dirty={} hits={} misses={} readahead={} writeback={}",
                    cache.page_entries,
                    cache.dirty_pages,
                    cache.page_hits,
                    cache.page_misses,
                    cache.readahead_pages,
                    cache.writeback_pages
                );
                let _ = writeln!(
                    scratch,
                    "block_cache entries={} hits={} misses={}",
                    cache.block_entries, cache.block_hits, cache.block_misses
                );
                return Some(scratch.as_str());
            }
            "/sys/kernel/umask" => {
                let _ = writeln!(scratch, "{:04o}", self.umask());
                return Some(scratch.as_str());
            }
            "/proc/storage" | "/sys/block/devices" => {
                let storage = crate::storage::summary();
                let _ = writeln!(
                    scratch,
                    "ata={} ahci={} nvme={} async_depth={}",
                    storage.ata_present as u8,
                    storage.ahci_present as u8,
                    storage.nvme_present as u8,
                    storage.async_queue_depth
                );
                let _ = writeln!(
                    scratch,
                    "block_io={} async_io={} ext2={} ext4={} ext_rw={}",
                    storage.block_self_test as u8,
                    storage.async_self_test as u8,
                    storage.ext2_supported as u8,
                    storage.ext4_supported as u8,
                    storage.ext_self_test as u8
                );
                return Some(scratch.as_str());
            }
            "/proc/mounts.initramfs" => {
                let root = crate::initramfs::summary();
                let _ = writeln!(
                    scratch,
                    "rootfs / initramfs rw entries={} bytes={}",
                    root.entries, root.bytes
                );
                return Some(scratch.as_str());
            }
            "/sys/fs/sync" => {
                let sync = self.sync_snapshot();
                let _ = writeln!(
                    scratch,
                    "generation={} synced={} dirty={} fsync={} recovery={}",
                    sync.write_generation,
                    sync.synced_generation,
                    sync.dirty_files,
                    sync.fsync_count,
                    sync.recovery_count
                );
                return Some(scratch.as_str());
            }
            _ => self.read_static_file(path),
        }?;
        let _ = scratch.push_str(static_text);
        Some(scratch.as_str())
    }

    pub fn path_exists(&self, path: &str) -> bool {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let Ok(path) = normalize_path(path, &mut normalized) else {
            return false;
        };

        self.read_static_file(path).is_some()
            || user_file_exists(path)
            || DYNAMIC_FILES.iter().any(|item| *item == path)
            || DEVICE_FILES.iter().any(|item| *item == path)
            || self.directory_exists(path)
    }

    pub fn is_executable(&self, path: &str) -> bool {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let Ok(path) = normalize_path(path, &mut normalized) else {
            return false;
        };

        for entry in STATIC_FILES {
            if entry.path == path {
                return entry.executable;
            }
        }
        user_file_executable(path)
    }

    pub fn is_directory(&self, path: &str) -> bool {
        self.directory_exists(path)
    }

    pub fn preview_text(&self, path: &str) -> Option<&'static str> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized).ok()?;
        match path {
            "/proc/version" => Some(crate::arch::proc_version()),
            "/proc/services" => {
                Some("Dynamic proc file. Use `services` in CMD for the live service table.\n")
            }
            "/proc/tasks" => Some("Dynamic proc file. Use `ps` in CMD for the live task table.\n"),
            "/proc/ipc" => Some("Dynamic proc file. Use `ipc` in CMD for the live IPC trace.\n"),
            "/proc/mounts" => Some("rootfs / rootfs rw 0 0\ntmpfs /tmp tmpfs rw 0 0\ndevfs /dev devfs rw 0 0\nprocfs /proc procfs ro 0 0\nsysfs /sys sysfs ro 0 0\n"),
            "/proc/filesystems" => Some(
                "nodev\trootfs\nnodev\ttmpfs\nnodev\tdevfs\nnodev\tprocfs\nnodev\tsysfs\n\text2\n\text4\n",
            ),
            _ => self.read_static_file(path),
        }
    }

    pub fn umask(&self) -> u16 {
        PROCESS_UMASK.load(Ordering::Acquire) & 0o777
    }

    pub fn set_umask(&self, mask: u16) -> u16 {
        PROCESS_UMASK.swap(mask & 0o777, Ordering::AcqRel)
    }

    pub fn cache_snapshot(&self) -> CacheSnapshot {
        let cache = CACHE_STATE.lock();
        CacheSnapshot {
            page_hits: PAGE_CACHE_HITS.load(Ordering::Relaxed),
            page_misses: PAGE_CACHE_MISSES.load(Ordering::Relaxed),
            block_hits: BLOCK_CACHE_HITS.load(Ordering::Relaxed),
            block_misses: BLOCK_CACHE_MISSES.load(Ordering::Relaxed),
            page_entries: cache.page.iter().filter(|entry| entry.valid).count(),
            block_entries: cache.block.iter().filter(|entry| entry.valid).count(),
            dirty_pages: cache
                .page
                .iter()
                .filter(|entry| entry.valid && entry.dirty)
                .count(),
            readahead_pages: READAHEAD_PAGES.load(Ordering::Relaxed),
            writeback_pages: WRITEBACK_PAGES.load(Ordering::Relaxed),
        }
    }

    pub fn sync_snapshot(&self) -> SyncSnapshot {
        let storage = USER_STORAGE.lock();
        SyncSnapshot {
            write_generation: WRITE_GENERATION.load(Ordering::Acquire),
            synced_generation: SYNCED_GENERATION.load(Ordering::Acquire),
            fsync_count: FSYNC_COUNT.load(Ordering::Relaxed),
            recovery_count: RECOVERY_COUNT.load(Ordering::Relaxed),
            dirty_files: storage
                .files
                .iter()
                .filter(|file| file.used && (file.dirty_data || file.dirty_metadata))
                .count(),
        }
    }

    pub fn fsync_path(&self, path: &str) -> LinuxResult<()> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized)?;
        if DEVICE_FILES.contains(&path) || DYNAMIC_FILES.contains(&path) {
            return Ok(());
        }
        let generation = WRITE_GENERATION.load(Ordering::Acquire);
        let mut storage = USER_STORAGE.lock();
        if let Some(file) = storage
            .files
            .iter_mut()
            .find(|file| file.used && file.path.as_str() == path)
        {
            writeback_file_cache(file);
            core::sync::atomic::compiler_fence(Ordering::Release);
            file.dirty_data = false;
            core::sync::atomic::compiler_fence(Ordering::SeqCst);
            file.dirty_metadata = false;
            file.synced_generation = generation;
            SYNCED_GENERATION.fetch_max(generation, Ordering::AcqRel);
            FSYNC_COUNT.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        if STATIC_FILES.iter().any(|entry| entry.path == path) || self.directory_exists(path) {
            return Ok(());
        }
        Err(Errno::ENOENT)
    }

    pub fn sync_all(&self) {
        let generation = WRITE_GENERATION.load(Ordering::Acquire);
        let mut storage = USER_STORAGE.lock();
        for file in &mut storage.files {
            if file.used && (file.dirty_data || file.dirty_metadata) {
                writeback_file_cache(file);
                file.dirty_data = false;
                core::sync::atomic::compiler_fence(Ordering::SeqCst);
                file.dirty_metadata = false;
                file.synced_generation = generation;
                FSYNC_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }
        SYNCED_GENERATION.store(generation, Ordering::Release);
    }

    pub fn recover_consistency(&self) -> bool {
        let synced = SYNCED_GENERATION.load(Ordering::Acquire);
        let mut storage = USER_STORAGE.lock();
        let mut recovered = false;
        for file in &mut storage.files {
            if file.used && file.synced_generation > synced {
                file.dirty_data = false;
                file.dirty_metadata = false;
                file.synced_generation = synced;
                recovered = true;
            }
        }
        if recovered {
            RECOVERY_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        true
    }

    pub fn metadata(&self, path: &str) -> LinuxResult<FileMetadata> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized)?;
        if self.directory_exists(path) {
            let (mode, uid, gid) = directory_attributes(path);
            return Ok(FileMetadata {
                size: 0,
                directory: true,
                executable: false,
                char_device: false,
                inode: stable_inode(path),
                links: 1,
                symlink: false,
                mode,
                uid,
                gid,
            });
        }
        let storage = USER_STORAGE.lock();
        if let Some(file) = storage
            .files
            .iter()
            .find(|file| file.used && file.path.as_str() == path)
        {
            let links = storage.links.iter().fold(1u32, |count, link| {
                if link.used && !link.symlink && link.target.as_str() == path {
                    count.saturating_add(1)
                } else {
                    count
                }
            });
            return Ok(FileMetadata {
                size: file.len as u64,
                directory: false,
                executable: file.executable,
                char_device: false,
                inode: file.inode.max(stable_inode(path)),
                links,
                symlink: false,
                mode: file.mode,
                uid: file.uid,
                gid: file.gid,
            });
        }
        drop(storage);
        if let Some(entry) = STATIC_FILES.iter().find(|entry| entry.path == path) {
            return Ok(FileMetadata {
                size: entry.text.len() as u64,
                directory: false,
                executable: entry.executable,
                char_device: false,
                inode: stable_inode(path),
                links: link_count(path),
                symlink: false,
                mode: if entry.executable { 0o755 } else { 0o644 },
                uid: ROOT_UID,
                gid: ROOT_GID,
            });
        }
        if DEVICE_FILES.contains(&path) {
            return Ok(FileMetadata {
                size: 0,
                directory: false,
                executable: false,
                char_device: true,
                inode: stable_inode(path),
                links: 1,
                symlink: false,
                mode: device_mode(path),
                uid: ROOT_UID,
                gid: ROOT_GID,
            });
        }
        if DYNAMIC_FILES.contains(&path) {
            let mut scratch = InlineString::<MAX_FILE_TEXT>::new();
            let size = self
                .read_file(path, &mut scratch)
                .map(str::len)
                .unwrap_or(0);
            return Ok(FileMetadata {
                size: size as u64,
                directory: false,
                executable: false,
                char_device: false,
                inode: stable_inode(path),
                links: 1,
                symlink: false,
                mode: 0o444,
                uid: ROOT_UID,
                gid: ROOT_GID,
            });
        }
        Err(Errno::ENOENT)
    }

    pub fn read_at(&self, path: &str, offset: u64, output: &mut [u8]) -> LinuxResult<usize> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized)?;
        if self.directory_exists(path) {
            return Err(Errno::EISDIR);
        }
        let offset = usize::try_from(offset).map_err(|_| Errno::EOVERFLOW)?;
        if DEVICE_FILES.contains(&path) {
            return read_device(path, offset, output);
        }
        let storage = USER_STORAGE.lock();
        if let Some(file) = storage
            .files
            .iter()
            .find(|file| file.used && file.path.as_str() == path)
        {
            return Ok(read_cached_source(
                file.inode,
                &file.data[..file.len],
                offset,
                output,
            ));
        }
        drop(storage);
        if let Some(entry) = STATIC_FILES.iter().find(|entry| entry.path == path) {
            return Ok(read_cached_source(
                stable_inode(path),
                entry.text.as_bytes(),
                offset,
                output,
            ));
        }
        if DYNAMIC_FILES.contains(&path) {
            let mut scratch = InlineString::<MAX_FILE_TEXT>::new();
            let text = self.read_file(path, &mut scratch).ok_or(Errno::ENOENT)?;
            return Ok(copy_bytes_at(text.as_bytes(), offset, output));
        }
        Err(Errno::ENOENT)
    }

    pub fn write_at(&self, path: &str, offset: u64, input: &[u8]) -> LinuxResult<usize> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized)?;
        if DYNAMIC_FILES.contains(&path) {
            return Err(Errno::EROFS);
        }
        if DEVICE_FILES.contains(&path) {
            return write_device(path, input);
        }
        if self.directory_exists(path) {
            return Err(Errno::EISDIR);
        }
        if self.is_executable(path) {
            return Err(Errno::EACCES);
        }
        let offset = usize::try_from(offset).map_err(|_| Errno::EFBIG)?;
        let end = offset.checked_add(input.len()).ok_or(Errno::EFBIG)?;
        if end > MAX_FILE_BYTES {
            return Err(Errno::EFBIG);
        }
        let static_source = STATIC_FILES
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.text.as_bytes());
        let mut storage = USER_STORAGE.lock();
        let index = match storage
            .files
            .iter()
            .position(|file| file.used && file.path.as_str() == path)
        {
            Some(index) => index,
            None => {
                let source = static_source.ok_or(Errno::ENOENT)?;
                let index = storage
                    .files
                    .iter()
                    .position(|file| !file.used)
                    .ok_or(Errno::ENOSPC)?;
                let file = &mut storage.files[index];
                file.used = true;
                file.inode = stable_inode(path);
                file.executable = false;
            file.path.set(path);
            file.len = source.len();
            file.data[..source.len()].copy_from_slice(source);
            file.dirty_metadata = true;
                index
            }
        };
        let file = &mut storage.files[index];
        cache_write(
            file.inode,
            &file.data[..file.len],
            offset,
            input,
            WRITE_GENERATION.load(Ordering::Acquire).wrapping_add(1),
        )?;
        file.len = file.len.max(end);
        file.dirty_data = true;
        file.dirty_metadata = true;
        WRITE_GENERATION.fetch_add(1, Ordering::AcqRel);
        Ok(input.len())
    }

    pub fn truncate(&self, path: &str) -> LinuxResult<()> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized)?;
        if DYNAMIC_FILES.contains(&path) {
            return Err(Errno::EROFS);
        }
        if DEVICE_FILES.contains(&path) {
            return Err(Errno::EINVAL);
        }
        if self.directory_exists(path) {
            return Err(Errno::EISDIR);
        }
        if self.is_executable(path) {
            return Err(Errno::EACCES);
        }
        let mut storage = USER_STORAGE.lock();
        if let Some(file) = storage
            .files
            .iter_mut()
            .find(|file| file.used && file.path.as_str() == path)
        {
            file.len = 0;
            file.dirty_data = true;
            file.dirty_metadata = true;
            WRITE_GENERATION.fetch_add(1, Ordering::AcqRel);
            invalidate_cache(file.inode);
            return Ok(());
        }
        if STATIC_FILES.iter().any(|entry| entry.path == path) {
            let file = storage
                .files
                .iter_mut()
                .find(|file| !file.used)
                .ok_or(Errno::ENOSPC)?;
            file.used = true;
            file.inode = stable_inode(path);
            file.executable = false;
            file.mode = 0o644;
            file.uid = ROOT_UID;
            file.gid = ROOT_GID;
            file.path.set(path);
            file.len = 0;
            file.dirty_data = true;
            file.dirty_metadata = true;
            WRITE_GENERATION.fetch_add(1, Ordering::AcqRel);
            invalidate_cache(file.inode);
            return Ok(());
        }
        Err(Errno::ENOENT)
    }

    pub fn write_text(&self, path: &str, text: &str) -> LinuxResult<()> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized)?;
        if DYNAMIC_FILES.iter().any(|item| *item == path) {
            return Err(Errno::ReadOnlyFilesystem);
        }
        if self.directory_exists(path) {
            return Err(Errno::IsDirectory);
        }
        if self.is_executable(path) {
            return Err(Errno::Access);
        }
        if !self.path_exists(path) {
            let parent = parent_path(path);
            if !self.directory_exists(parent) {
                return Err(Errno::NoEntry);
            }
            create_user_file(path, 0o666, ROOT_UID, ROOT_GID, self.umask())?;
        }
        update_user_file(path, text)
    }

    pub fn create_file(&self, path: &str) -> LinuxResult<()> {
        self.create_file_with(path, 0o666, ROOT_UID, ROOT_GID, self.umask())
    }

    pub fn create_file_with(
        &self,
        path: &str,
        mode: u16,
        uid: u32,
        gid: u32,
        umask: u16,
    ) -> LinuxResult<()> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized)?;
        if self.path_exists(path) {
            if self.directory_exists(path) {
                return Err(Errno::IsDirectory);
            }
            return Ok(());
        }
        let parent = parent_path(path);
        if !self.directory_exists(parent) {
            return Err(Errno::NoEntry);
        }
        create_user_file(path, mode, uid, gid, umask)
    }

    pub fn create_dir(&self, path: &str) -> LinuxResult<()> {
        self.create_dir_with(path, 0o777, ROOT_UID, ROOT_GID, self.umask())
    }

    pub fn create_dir_with(
        &self,
        path: &str,
        mode: u16,
        uid: u32,
        gid: u32,
        umask: u16,
    ) -> LinuxResult<()> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized)?;
        if path == "/" {
            return Err(Errno::Exists);
        }
        if self.path_exists(path) {
            return Err(Errno::Exists);
        }
        let parent = parent_path(path);
        if !self.directory_exists(parent) {
            return Err(Errno::NoEntry);
        }
        create_user_dir(path, mode, uid, gid, umask)
    }

    pub fn chmod(&self, path: &str, mode: u16) -> LinuxResult<()> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized)?;
        if DYNAMIC_FILES.contains(&path) || DEVICE_FILES.contains(&path) {
            return Err(Errno::EROFS);
        }
        let mut storage = USER_STORAGE.lock();
        if let Some(file) = storage
            .files
            .iter_mut()
            .find(|file| file.used && file.path.as_str() == path)
        {
            file.mode = mode & 0o7777;
            file.executable = mode & 0o111 != 0;
            return Ok(());
        }
        if let Some(directory) = storage
            .directories
            .iter_mut()
            .find(|directory| directory.used && directory.path.as_str() == path)
        {
            directory.mode = mode & 0o7777;
            return Ok(());
        }
        Err(Errno::ENOENT)
    }

    pub fn chown(&self, path: &str, uid: u32, gid: u32) -> LinuxResult<()> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized)?;
        if DYNAMIC_FILES.contains(&path) || DEVICE_FILES.contains(&path) {
            return Err(Errno::EROFS);
        }
        let mut storage = USER_STORAGE.lock();
        if let Some(file) = storage
            .files
            .iter_mut()
            .find(|file| file.used && file.path.as_str() == path)
        {
            file.uid = uid;
            file.gid = gid;
            return Ok(());
        }
        if let Some(directory) = storage
            .directories
            .iter_mut()
            .find(|directory| directory.used && directory.path.as_str() == path)
        {
            directory.uid = uid;
            directory.gid = gid;
            return Ok(());
        }
        Err(Errno::ENOENT)
    }

    pub fn directory_exists(&self, path: &str) -> bool {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let Ok(path) = normalize_path(path, &mut normalized) else {
            return false;
        };
        if path == "/" {
            return true;
        }
        if MOUNTS.iter().any(|mount| mount.mountpoint == path) {
            return true;
        }
        for entry in STATIC_FILES {
            if child_name(entry.path, path).is_some() {
                return true;
            }
        }
        for entry in DYNAMIC_FILES {
            if child_name(entry, path).is_some() {
                return true;
            }
        }
        let storage = USER_STORAGE.lock();
        for directory in &storage.directories {
            if directory.used {
                let entry = directory.path.as_str();
                if entry == path || child_name(entry, path).is_some() {
                    return true;
                }
            }
        }
        for file in &storage.files {
            if file.used && child_name(file.path.as_str(), path).is_some() {
                return true;
            }
        }
        for link in &storage.links {
            if link.used && child_name(link.path.as_str(), path).is_some() {
                return true;
            }
        }
        false
    }

    pub fn list_dir(&self, path: &str, out: &mut DirEntries) -> LinuxResult<()> {
        let mut normalized = InlineString::<MAX_PATH_LEN>::new();
        let path = normalize_path(path, &mut normalized)?;

        out.clear();

        for entry in STATIC_FILES {
            if let Some(child) = child_name(entry.path, path) {
                out.push_unique(child);
            }
        }

        for entry in DYNAMIC_FILES {
            if let Some(child) = child_name(entry, path) {
                out.push_unique(child);
            }
        }
        for entry in DEVICE_FILES {
            if let Some(child) = child_name(entry, path) {
                out.push_unique(child);
            }
        }
        for mount in MOUNTS {
            if let Some(child) = child_name(mount.mountpoint, path) {
                out.push_unique(child);
            }
        }
        let storage = USER_STORAGE.lock();
        for directory in &storage.directories {
            if directory.used {
                if let Some(child) = child_name(directory.path.as_str(), path) {
                    out.push_unique(child);
                }
            }
        }
        for file in &storage.files {
            if file.used {
                if let Some(child) = child_name(file.path.as_str(), path) {
                    out.push_unique(child);
                }
            }
        }
        for link in &storage.links {
            if link.used {
                if let Some(child) = child_name(link.path.as_str(), path) {
                    out.push_unique(child);
                }
            }
        }
        drop(storage);

        if out.is_empty() && !self.directory_exists(path) {
            Err(Errno::NotDirectory)
        } else {
            Ok(())
        }
    }
}

fn child_name<'a>(path: &'a str, dir: &str) -> Option<&'a str> {
    if dir == "/" {
        let trimmed = path.trim_start_matches('/');
        return trimmed.split('/').next();
    }

    let remainder = path.strip_prefix(dir)?;
    let remainder = remainder.strip_prefix('/')?;
    if remainder.is_empty() {
        return None;
    }

    remainder.split('/').next()
}

fn user_file_exists(path: &str) -> bool {
    USER_STORAGE
        .lock()
        .files
        .iter()
        .any(|file| file.used && file.path.as_str() == path)
}

fn user_file_executable(path: &str) -> bool {
    USER_STORAGE
        .lock()
        .files
        .iter()
        .find(|file| file.used && file.path.as_str() == path)
        .map(|file| file.executable)
        .unwrap_or(false)
}

fn create_user_file(path: &str, mode: u16, uid: u32, gid: u32, umask: u16) -> LinuxResult<()> {
    let mut storage = USER_STORAGE.lock();
    let file = storage
        .files
        .iter_mut()
        .find(|file| !file.used)
        .ok_or(Errno::NoSpace)?;
    file.len = 0;
    file.inode = stable_inode(path);
    file.path.set(path);
    if file.inode == 0 {
        file.inode = stable_inode(path);
    }
    file.executable = mode & 0o111 != 0;
    file.mode = mode & !umask & 0o7777;
    file.uid = uid;
    file.gid = gid;
    file.used = true;
    file.dirty_metadata = true;
    WRITE_GENERATION.fetch_add(1, Ordering::AcqRel);
    Ok(())
}

fn update_user_file(path: &str, text: &str) -> LinuxResult<()> {
    if text.len() > MAX_FILE_BYTES {
        return Err(Errno::FileTooLarge);
    }

    let mut storage = USER_STORAGE.lock();
    let existing = storage
        .files
        .iter()
        .position(|file| file.used && file.path.as_str() == path);
    let index = match existing {
        Some(index) => index,
        None if STATIC_FILES.iter().any(|entry| entry.path == path) => storage
            .files
            .iter()
            .position(|file| !file.used)
            .ok_or(Errno::NoSpace)?,
        None => return Err(Errno::NoEntry),
    };

    let file = &mut storage.files[index];
    file.path.set(path);
    file.inode = stable_inode(path);
    file.data[..text.len()].copy_from_slice(text.as_bytes());
    file.len = text.len();
    file.executable = false;
    if !file.used {
        file.mode = 0o644;
        file.uid = ROOT_UID;
        file.gid = ROOT_GID;
    }
    file.used = true;
    file.dirty_data = true;
    file.dirty_metadata = true;
    WRITE_GENERATION.fetch_add(1, Ordering::AcqRel);
    invalidate_cache(file.inode);
    Ok(())
}

fn copy_bytes_at(source: &[u8], offset: usize, output: &mut [u8]) -> usize {
    if offset >= source.len() {
        return 0;
    }
    let count = output.len().min(source.len() - offset);
    output[..count].copy_from_slice(&source[offset..offset + count]);
    count
}

fn create_user_dir(path: &str, mode: u16, uid: u32, gid: u32, umask: u16) -> LinuxResult<()> {
    let mut storage = USER_STORAGE.lock();
    let index = storage
        .directories
        .iter()
        .position(|directory| !directory.used)
        .ok_or(Errno::NoSpace)?;
    storage.directories[index].path.set(path);
    storage.directories[index].mode = mode & !umask & 0o7777;
    storage.directories[index].uid = uid;
    storage.directories[index].gid = gid;
    storage.directories[index].used = true;
    Ok(())
}

fn directory_attributes(path: &str) -> (u16, u32, u32) {
    if path == "/tmp" {
        return (0o1777, ROOT_UID, ROOT_GID);
    }
    USER_STORAGE
        .lock()
        .directories
        .iter()
        .find(|directory| directory.used && directory.path.as_str() == path)
        .map(|directory| (directory.mode, directory.uid, directory.gid))
        .unwrap_or((0o755, ROOT_UID, ROOT_GID))
}

fn device_mode(path: &str) -> u16 {
    match path {
        "/dev/random" => 0o444,
        "/dev/console" => 0o620,
        _ => 0o666,
    }
}

fn read_device(path: &str, offset: usize, output: &mut [u8]) -> LinuxResult<usize> {
    match path {
        "/dev/null" | "/dev/console" => Ok(0),
        "/dev/zero" => {
            output.fill(0);
            Ok(output.len())
        }
        "/dev/random" => {
            let mut state = stable_inode(path).wrapping_add(offset as u64);
            for byte in output.iter_mut() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *byte = state as u8;
            }
            Ok(output.len())
        }
        _ => Err(Errno::ENODEV),
    }
}

fn write_device(path: &str, input: &[u8]) -> LinuxResult<usize> {
    match path {
        "/dev/null" | "/dev/zero" | "/dev/random" => Ok(input.len()),
        "/dev/console" => {
            if let Ok(text) = core::str::from_utf8(input) {
                crate::arch::console_write(text);
            }
            Ok(input.len())
        }
        _ => Err(Errno::ENODEV),
    }
}

fn read_cached_source(inode: u64, source: &[u8], offset: usize, output: &mut [u8]) -> usize {
    if output.is_empty() || offset >= source.len() {
        return 0;
    }
    let mut cache = CACHE_STATE.lock();
    let mut copied = 0usize;
    while copied < output.len() && offset + copied < source.len() {
        let absolute = offset + copied;
        let page_index = absolute / CACHE_PAGE_BYTES;
        let page_offset = absolute % CACHE_PAGE_BYTES;
        let slot = match find_page(&cache, inode, page_index as u64) {
            Some(slot) => {
                PAGE_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                slot
            }
            None => {
                PAGE_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
                let Some(slot) = fill_page(&mut cache, inode, page_index as u64, source) else {
                    let length = output
                        .len()
                        .min(source.len() - absolute)
                        .min(CACHE_PAGE_BYTES - page_offset);
                    output[copied..copied + length]
                        .copy_from_slice(&source[absolute..absolute + length]);
                    copied += length;
                    continue;
                };
                slot
            }
        };
        touch_block(&mut cache, inode, (absolute / 512) as u64);
        let available = cache.page[slot].length.saturating_sub(page_offset);
        let length = (output.len() - copied)
            .min(source.len() - absolute)
            .min(available);
        if length == 0 {
            break;
        }
        output[copied..copied + length]
            .copy_from_slice(&cache.page[slot].data[page_offset..page_offset + length]);
        copied += length;

        let next_page = page_index + 1;
        if next_page * CACHE_PAGE_BYTES < source.len()
            && find_page(&cache, inode, next_page as u64).is_none()
            && fill_page(&mut cache, inode, next_page as u64, source).is_some()
        {
            READAHEAD_PAGES.fetch_add(1, Ordering::Relaxed);
        }
    }
    copied
}

fn find_page(cache: &CacheState, inode: u64, index: u64) -> Option<usize> {
    cache
        .page
        .iter()
        .position(|entry| entry.valid && entry.inode == inode && entry.index == index)
}

fn fill_page(cache: &mut CacheState, inode: u64, index: u64, source: &[u8]) -> Option<usize> {
    let slot = (0..CACHE_SLOTS)
        .map(|step| (cache.next_page + step) % CACHE_SLOTS)
        .find(|slot| !cache.page[*slot].valid || !cache.page[*slot].dirty)?;
    let start = usize::try_from(index).ok()?.checked_mul(CACHE_PAGE_BYTES)?;
    let length = source.len().saturating_sub(start).min(CACHE_PAGE_BYTES);
    let entry = &mut cache.page[slot];
    *entry = PageCacheEntry::empty();
    entry.valid = true;
    entry.inode = inode;
    entry.index = index;
    entry.length = length;
    if length != 0 {
        entry.data[..length].copy_from_slice(&source[start..start + length]);
    }
    cache.next_page = (slot + 1) % CACHE_SLOTS;
    Some(slot)
}

fn touch_block(cache: &mut CacheState, inode: u64, index: u64) {
    if cache
        .block
        .iter()
        .any(|entry| entry.valid && entry.inode == inode && entry.index == index)
    {
        BLOCK_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
        return;
    }
    BLOCK_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
    let slot = cache.next_block;
    cache.block[slot] = CacheEntry {
        valid: true,
        inode,
        index,
    };
    cache.next_block = (slot + 1) % CACHE_SLOTS;
}

fn cache_write(
    inode: u64,
    backing: &[u8],
    offset: usize,
    input: &[u8],
    generation: u64,
) -> LinuxResult<()> {
    let mut cache = CACHE_STATE.lock();
    let mut written = 0usize;
    while written < input.len() {
        let absolute = offset + written;
        let page_index = absolute / CACHE_PAGE_BYTES;
        let page_offset = absolute % CACHE_PAGE_BYTES;
        let slot = match find_page(&cache, inode, page_index as u64) {
            Some(slot) => slot,
            None => {
                fill_page(&mut cache, inode, page_index as u64, backing).ok_or(Errno::ENOSPC)?
            }
        };
        let length = (input.len() - written).min(CACHE_PAGE_BYTES - page_offset);
        let entry = &mut cache.page[slot];
        entry.data[page_offset..page_offset + length]
            .copy_from_slice(&input[written..written + length]);
        entry.length = entry.length.max(page_offset + length);
        entry.dirty = true;
        entry.generation = generation;
        written += length;
    }
    Ok(())
}

fn writeback_file_cache(file: &mut UserFile) {
    let mut cache = CACHE_STATE.lock();
    for entry in &mut cache.page {
        if !entry.valid || !entry.dirty || entry.inode != file.inode {
            continue;
        }
        let start = entry.index as usize * CACHE_PAGE_BYTES;
        if start < file.len {
            let length = entry
                .length
                .min(file.len - start)
                .min(MAX_FILE_BYTES - start);
            file.data[start..start + length].copy_from_slice(&entry.data[..length]);
        }
        entry.dirty = false;
        WRITEBACK_PAGES.fetch_add(1, Ordering::Relaxed);
    }
}

fn invalidate_cache(inode: u64) {
    let mut cache = CACHE_STATE.lock();
    for entry in &mut cache.page {
        if entry.valid && entry.inode == inode {
            *entry = PageCacheEntry::empty();
        }
    }
    for entry in &mut cache.block {
        if entry.valid && entry.inode == inode {
            *entry = CacheEntry::empty();
        }
    }
}

fn parent_path(path: &str) -> &str {
    path::parent(path)
}

fn normalize_path<'a>(path: &str, out: &'a mut InlineString<MAX_PATH_LEN>) -> LinuxResult<&'a str> {
    VfsService::new().resolve_path("/", "/", path, out)
}

fn stable_inode(path: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in path.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash.max(1)
}

fn link_count(path: &str) -> u32 {
    USER_STORAGE.lock().links.iter().fold(1u32, |count, link| {
        if link.used && !link.symlink && link.target.as_str() == path {
            count.saturating_add(1)
        } else {
            count
        }
    })
}

fn raw_path_exists(path: &str) -> bool {
    if STATIC_FILES.iter().any(|entry| entry.path == path)
        || DYNAMIC_FILES.contains(&path)
        || DEVICE_FILES.contains(&path)
        || MOUNTS.iter().any(|mount| mount.mountpoint == path)
    {
        return true;
    }
    let storage = USER_STORAGE.lock();
    storage
        .files
        .iter()
        .any(|file| file.used && file.path.as_str() == path)
        || storage
            .links
            .iter()
            .any(|link| link.used && link.path.as_str() == path)
        || storage
            .directories
            .iter()
            .any(|directory| directory.used && directory.path.as_str() == path)
}

fn is_link_prefix(link: &str, path: &str) -> bool {
    path == link
        || path
            .strip_prefix(link)
            .is_some_and(|rest| rest.starts_with('/'))
}
