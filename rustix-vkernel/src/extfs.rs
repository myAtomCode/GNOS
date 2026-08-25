use core::cmp::min;

use crate::storage::{BlockDevice, BlockError, SECTOR_SIZE};

const EXT_MAGIC: u16 = 0xef53;
const EXT4_EXTENTS: u32 = 0x40;
const EXT4_64BIT: u32 = 0x80;
const EXTENT_MAGIC: u16 = 0xf30a;
const ROOT_INODE: u32 = 2;
const MAX_COMPONENT: usize = 255;
const MAX_COMPONENTS: usize = 16;
const MAX_BLOCK_SIZE: usize = 4096;
const EXT_VALID_FS: u16 = 1;
const EXT_ERROR_FS: u16 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtError {
    Block(BlockError),
    InvalidSuperblock,
    Unsupported,
    NotFound,
    NotDirectory,
    IsDirectory,
    InvalidPath,
    Corrupt,
    ReadOnly,
    NoSpace,
}

impl From<BlockError> for ExtError {
    fn from(error: BlockError) -> Self {
        Self::Block(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtKind {
    Ext2,
    Ext4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtFileKind {
    Regular,
    Directory,
    Symlink,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtFileStat {
    pub inode: u32,
    pub kind: ExtFileKind,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
}

pub struct ExtFilesystem<'a, D: BlockDevice> {
    device: &'a mut D,
    kind: ExtKind,
    block_size: usize,
    inode_size: usize,
    descriptor_size: usize,
    inodes_per_group: u32,
    first_data_block: u32,
    blocks_per_group: u32,
    total_blocks: u64,
    groups: u32,
    read_only: bool,
    dirty: bool,
}

impl<'a, D: BlockDevice> ExtFilesystem<'a, D> {
    pub fn mount(device: &'a mut D) -> Result<Self, ExtError> {
        let mut superblock = [0u8; 1024];
        device.read_sectors(2, &mut superblock)?;
        if read_u16(&superblock, 56) != EXT_MAGIC {
            return Err(ExtError::InvalidSuperblock);
        }
        let log_block_size = read_u32(&superblock, 24);
        if log_block_size > 2 {
            return Err(ExtError::Unsupported);
        }
        let block_size = 1024usize << log_block_size;
        let inode_size = if read_u32(&superblock, 76) >= 1 {
            read_u16(&superblock, 88) as usize
        } else {
            128
        };
        if inode_size < 128 || inode_size > MAX_BLOCK_SIZE || block_size > MAX_BLOCK_SIZE {
            return Err(ExtError::Unsupported);
        }
        let incompat = read_u32(&superblock, 96);
        let kind = if incompat & EXT4_EXTENTS != 0 || incompat & EXT4_64BIT != 0 {
            ExtKind::Ext4
        } else {
            ExtKind::Ext2
        };
        let blocks = u64::from(read_u32(&superblock, 4));
        let blocks_high = if incompat & EXT4_64BIT != 0 {
            u64::from(read_u32(&superblock, 336)) << 32
        } else {
            0
        };
        let total_blocks = blocks | blocks_high;
        let descriptor_size = if incompat & EXT4_64BIT != 0 {
            (read_u16(&superblock, 254) as usize).max(64)
        } else {
            32
        };
        let inodes_per_group = read_u32(&superblock, 40);
        let blocks_per_group = read_u32(&superblock, 32);
        if inodes_per_group == 0 || blocks_per_group == 0 || total_blocks == 0 {
            return Err(ExtError::Corrupt);
        }
        let groups = total_blocks
            .div_ceil(u64::from(blocks_per_group))
            .min(u64::from(u32::MAX)) as u32;
        let clean = read_u16(&superblock, 58) == EXT_VALID_FS;
        Ok(Self {
            device,
            kind,
            block_size,
            inode_size,
            descriptor_size,
            inodes_per_group,
            first_data_block: read_u32(&superblock, 20),
            blocks_per_group,
            total_blocks,
            groups,
            read_only: false,
            dirty: !clean,
        })
    }

    pub fn kind(&self) -> ExtKind {
        self.kind
    }

    pub fn block_size(&self) -> usize {
        self.block_size
    }

    pub fn group_count(&self) -> u32 {
        self.groups
    }

    pub fn total_blocks(&self) -> u64 {
        self.total_blocks
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    pub fn needs_recovery(&self) -> bool {
        self.dirty
    }

    pub fn recover(&mut self) -> Result<(), ExtError> {
        if !self.dirty {
            return Ok(());
        }
        if self.read_only {
            return Err(ExtError::ReadOnly);
        }
        self.set_superblock_state(EXT_VALID_FS)?;
        self.device.flush()?;
        self.dirty = false;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<(), ExtError> {
        self.device.flush()?;
        self.set_superblock_state(EXT_VALID_FS)?;
        self.device.flush()?;
        self.dirty = false;
        Ok(())
    }

    pub fn stat(&mut self, path: &str) -> Result<ExtFileStat, ExtError> {
        let inode = self.lookup_path(path)?;
        self.read_inode_stat(inode)
    }

    pub fn read_file(&mut self, path: &str, output: &mut [u8]) -> Result<usize, ExtError> {
        let inode = self.lookup_path(path)?;
        let stat = self.read_inode_stat(inode)?;
        if stat.kind == ExtFileKind::Directory {
            return Err(ExtError::IsDirectory);
        }
        self.read_inode_range(inode, 0, output)
    }

    pub fn read_at(
        &mut self,
        path: &str,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, ExtError> {
        let inode = self.lookup_path(path)?;
        let stat = self.read_inode_stat(inode)?;
        if stat.kind == ExtFileKind::Directory {
            return Err(ExtError::IsDirectory);
        }
        self.read_inode_range(inode, offset, output)
    }

    pub fn write_at(&mut self, path: &str, offset: u64, input: &[u8]) -> Result<usize, ExtError> {
        if self.read_only {
            return Err(ExtError::ReadOnly);
        }
        if !self.dirty {
            self.set_superblock_state(EXT_ERROR_FS)?;
            self.device.flush()?;
            self.dirty = true;
        }
        let inode = self.lookup_path(path)?;
        let stat = self.read_inode_stat(inode)?;
        if stat.kind == ExtFileKind::Directory {
            return Err(ExtError::IsDirectory);
        }
        let end = offset
            .checked_add(input.len() as u64)
            .ok_or(ExtError::NoSpace)?;
        let mut written = 0usize;
        let mut block = [0u8; MAX_BLOCK_SIZE];
        while written < input.len() {
            let cursor = offset as usize + written;
            let logical = (cursor / self.block_size) as u32;
            let within = cursor % self.block_size;
            let physical = self
                .map_inode_block(inode, logical)?
                .ok_or(ExtError::NoSpace)?;
            self.read_block(physical as u64, &mut block)?;
            let count = min(input.len() - written, self.block_size - within);
            block[within..within + count].copy_from_slice(&input[written..written + count]);
            self.write_block(physical as u64, &block[..self.block_size])?;
            written += count;
        }
        if end > stat.size {
            self.update_inode_size(inode, end)?;
        }
        self.device.flush()?;
        Ok(written)
    }

    fn lookup_path(&mut self, path: &str) -> Result<u32, ExtError> {
        if path.is_empty() || !path.starts_with('/') {
            return Err(ExtError::InvalidPath);
        }
        let mut current = ROOT_INODE;
        let mut components = [0u8; MAX_COMPONENT];
        let mut start = 1usize;
        let bytes = path.as_bytes();
        if bytes.len() == 1 {
            return Ok(current);
        }
        for _ in 0..MAX_COMPONENTS {
            while start < bytes.len() && bytes[start] == b'/' {
                start += 1;
            }
            if start >= bytes.len() {
                return Ok(current);
            }
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'/' {
                end += 1;
            }
            let length = end - start;
            if length == 0 || length > MAX_COMPONENT {
                return Err(ExtError::InvalidPath);
            }
            components[..length].copy_from_slice(&bytes[start..end]);
            current = self.find_child(current, &components[..length])?;
            start = end;
        }
        Err(ExtError::InvalidPath)
    }

    fn find_child(&mut self, parent: u32, name: &[u8]) -> Result<u32, ExtError> {
        let stat = self.read_inode_stat(parent)?;
        if stat.kind != ExtFileKind::Directory {
            return Err(ExtError::NotDirectory);
        }
        let mut block = [0u8; MAX_BLOCK_SIZE];
        let blocks = stat.size.div_ceil(self.block_size as u64);
        for logical in 0..blocks as u32 {
            let Some(physical) = self.map_inode_block(parent, logical)? else {
                continue;
            };
            self.read_block(physical as u64, &mut block)?;
            let mut offset = 0usize;
            while offset + 8 <= self.block_size {
                let inode = read_u32(&block, offset);
                let record_length = read_u16(&block, offset + 4) as usize;
                let name_length = block[offset + 6] as usize;
                if record_length < 8 || offset + record_length > self.block_size {
                    return Err(ExtError::Corrupt);
                }
                if inode != 0
                    && name_length == name.len()
                    && &block[offset + 8..offset + 8 + name_length] == name
                {
                    return Ok(inode);
                }
                offset += record_length;
            }
        }
        Err(ExtError::NotFound)
    }

    fn read_inode_stat(&mut self, inode: u32) -> Result<ExtFileStat, ExtError> {
        let mut raw = [0u8; MAX_BLOCK_SIZE];
        self.read_inode_raw(inode, &mut raw)?;
        let mode = read_u16(&raw, 0);
        let kind = match mode & 0xf000 {
            0x4000 => ExtFileKind::Directory,
            0xa000 => ExtFileKind::Symlink,
            0x8000 => ExtFileKind::Regular,
            _ => return Err(ExtError::Unsupported),
        };
        let size = u64::from(read_u32(&raw, 4))
            | if kind == ExtFileKind::Regular {
                u64::from(read_u32(&raw, 108)) << 32
            } else {
                0
            };
        let uid = u32::from(read_u16(&raw, 2))
            | if self.inode_size >= 128 {
                u32::from(read_u16(&raw, 120)) << 16
            } else {
                0
            };
        let gid = u32::from(read_u16(&raw, 24))
            | if self.inode_size >= 128 {
                u32::from(read_u16(&raw, 122)) << 16
            } else {
                0
            };
        Ok(ExtFileStat {
            inode,
            kind,
            mode: mode & 0x0fff,
            uid,
            gid,
            size,
        })
    }

    fn read_inode_range(
        &mut self,
        inode: u32,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, ExtError> {
        let stat = self.read_inode_stat(inode)?;
        if offset >= stat.size || output.is_empty() {
            return Ok(0);
        }
        let available = min(output.len() as u64, stat.size - offset) as usize;
        let mut copied = 0usize;
        let mut block = [0u8; MAX_BLOCK_SIZE];
        while copied < available {
            let cursor = offset as usize + copied;
            let logical = (cursor / self.block_size) as u32;
            let within = cursor % self.block_size;
            let count = min(available - copied, self.block_size - within);
            let Some(physical) = self.map_inode_block(inode, logical)? else {
                output[copied..copied + count].fill(0);
                copied += count;
                continue;
            };
            self.read_block(physical as u64, &mut block)?;
            output[copied..copied + count].copy_from_slice(&block[within..within + count]);
            copied += count;
        }
        Ok(copied)
    }

    fn map_inode_block(&mut self, inode: u32, logical: u32) -> Result<Option<u64>, ExtError> {
        let mut raw = [0u8; MAX_BLOCK_SIZE];
        self.read_inode_raw(inode, &mut raw)?;
        let flags = read_u32(&raw, 32);
        if self.kind == ExtKind::Ext4 && flags & EXT4_EXTENTS != 0 {
            return self.map_extent(&raw[40..100], logical);
        }
        if logical < 12 {
            let block = read_u32(&raw, 40 + logical as usize * 4);
            return Ok((block != 0).then_some(u64::from(block)));
        }
        Err(ExtError::Unsupported)
    }

    fn map_extent(&mut self, root: &[u8], logical: u32) -> Result<Option<u64>, ExtError> {
        if read_u16(root, 0) != EXTENT_MAGIC {
            return Err(ExtError::Corrupt);
        }
        let entries = read_u16(root, 2) as usize;
        let depth = read_u16(root, 6);
        if depth != 0 {
            let mut selected = None;
            for index in 0..entries.min((root.len().saturating_sub(12)) / 12) {
                let entry = &root[12 + index * 12..24 + index * 12];
                let first = read_u32(entry, 0);
                if logical >= first {
                    selected = Some(entry);
                }
            }
            let entry = selected.ok_or(ExtError::Corrupt)?;
            let physical = (u64::from(read_u16(entry, 8)) << 32) | u64::from(read_u32(entry, 4));
            let mut block = [0u8; MAX_BLOCK_SIZE];
            self.read_block(physical, &mut block)?;
            return self.map_extent(&block[..self.block_size], logical);
        }
        for index in 0..entries.min(4) {
            let entry = &root[12 + index * 12..24 + index * 12];
            let first = read_u32(entry, 0);
            let length = u32::from(read_u16(entry, 4) & 0x7fff);
            if logical < first || logical >= first.saturating_add(length) {
                continue;
            }
            let start = (u64::from(read_u16(entry, 6)) << 32) | u64::from(read_u32(entry, 8));
            return Ok(Some(start + u64::from(logical - first)));
        }
        Ok(None)
    }

    fn update_inode_size(&mut self, inode: u32, size: u64) -> Result<(), ExtError> {
        let mut raw = [0u8; MAX_BLOCK_SIZE];
        self.read_inode_raw(inode, &mut raw)?;
        write_u32(&mut raw, 4, size as u32);
        write_u32(&mut raw, 108, (size >> 32) as u32);
        self.write_inode_raw(inode, &raw)
    }

    fn read_inode_raw(
        &mut self,
        inode: u32,
        output: &mut [u8; MAX_BLOCK_SIZE],
    ) -> Result<(), ExtError> {
        if inode == 0 {
            return Err(ExtError::Corrupt);
        }
        let group = (inode - 1) / self.inodes_per_group;
        let index = (inode - 1) % self.inodes_per_group;
        if group >= self.groups {
            return Err(ExtError::Corrupt);
        }
        let table = self.inode_table_block(group)?;
        let byte_offset = index as usize * self.inode_size;
        let block = table + (byte_offset / self.block_size) as u64;
        let within = byte_offset % self.block_size;
        output.fill(0);
        let mut data = [0u8; MAX_BLOCK_SIZE];
        self.read_block(block, &mut data)?;
        let first = min(self.inode_size, self.block_size - within);
        output[..first].copy_from_slice(&data[within..within + first]);
        if first < self.inode_size {
            self.read_block(block + 1, &mut data)?;
            output[first..self.inode_size].copy_from_slice(&data[..self.inode_size - first]);
        }
        Ok(())
    }

    fn write_inode_raw(
        &mut self,
        inode: u32,
        input: &[u8; MAX_BLOCK_SIZE],
    ) -> Result<(), ExtError> {
        if self.read_only {
            return Err(ExtError::ReadOnly);
        }
        let group = (inode - 1) / self.inodes_per_group;
        let index = (inode - 1) % self.inodes_per_group;
        let table = self.inode_table_block(group)?;
        let byte_offset = index as usize * self.inode_size;
        let block = table + (byte_offset / self.block_size) as u64;
        let within = byte_offset % self.block_size;
        let mut data = [0u8; MAX_BLOCK_SIZE];
        self.read_block(block, &mut data)?;
        let first = min(self.inode_size, self.block_size - within);
        data[within..within + first].copy_from_slice(&input[..first]);
        self.write_block(block, &data[..self.block_size])?;
        if first < self.inode_size {
            self.read_block(block + 1, &mut data)?;
            data[..self.inode_size - first].copy_from_slice(&input[first..self.inode_size]);
            self.write_block(block + 1, &data[..self.block_size])?;
        }
        Ok(())
    }

    fn inode_table_block(&mut self, group: u32) -> Result<u64, ExtError> {
        let descriptors_per_block = self.block_size / self.descriptor_size;
        let base = if self.block_size == 1024 { 2 } else { 1 };
        let descriptor_block = base + (group as usize / descriptors_per_block) as u32;
        let descriptor_offset = (group as usize % descriptors_per_block) * self.descriptor_size;
        let mut block = [0u8; MAX_BLOCK_SIZE];
        self.read_block(u64::from(descriptor_block), &mut block)?;
        let low = read_u32(&block, descriptor_offset + 8);
        let high = if self.descriptor_size >= 64 {
            u64::from(read_u32(&block, descriptor_offset + 40)) << 32
        } else {
            0
        };
        Ok(u64::from(low) | high)
    }

    fn read_block(
        &mut self,
        block: u64,
        output: &mut [u8; MAX_BLOCK_SIZE],
    ) -> Result<(), ExtError> {
        if block >= self.total_blocks {
            return Err(ExtError::Corrupt);
        }
        let sectors = self.block_size / SECTOR_SIZE;
        self.device
            .read_sectors(block * sectors as u64, &mut output[..self.block_size])?;
        Ok(())
    }

    fn write_block(&mut self, block: u64, input: &[u8]) -> Result<(), ExtError> {
        if self.read_only || input.len() != self.block_size {
            return Err(if self.read_only {
                ExtError::ReadOnly
            } else {
                ExtError::Corrupt
            });
        }
        let sectors = self.block_size / SECTOR_SIZE;
        self.device.write_sectors(block * sectors as u64, input)?;
        Ok(())
    }

    fn set_superblock_state(&mut self, state: u16) -> Result<(), ExtError> {
        let mut superblock = [0u8; 1024];
        self.device.read_sectors(2, &mut superblock)?;
        write_u16(&mut superblock, 58, state);
        self.device.write_sectors(2, &superblock)?;
        Ok(())
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub fn self_test() -> bool {
    self_test_kind(false) && self_test_kind(true)
}

fn self_test_kind(ext4: bool) -> bool {
    let mut disk = crate::storage::MEMORY_DISK.lock();
    let bytes = disk.bytes_mut();
    bytes.fill(0);
    write_u32(bytes, 1024, 16);
    write_u32(bytes, 1028, 64);
    write_u32(bytes, 1044, 1);
    write_u32(bytes, 1048, 0);
    write_u32(bytes, 1056, 64);
    write_u32(bytes, 1064, 16);
    write_u16(bytes, 1080, EXT_MAGIC);
    write_u32(bytes, 1100, 1);
    write_u16(bytes, 1112, 128);
    if ext4 {
        write_u32(bytes, 1120, EXT4_EXTENTS | EXT4_64BIT);
        write_u16(bytes, 1278, 64);
    }
    write_u32(bytes, 2048 + 8, 5);

    let root = 5 * 1024 + 128;
    write_u16(bytes, root, 0x4000 | 0o755);
    write_u32(bytes, root + 4, 1024);
    write_u16(bytes, root + 26, 2);
    let file = 5 * 1024 + 256;
    write_u16(bytes, file, 0x8000 | 0o644);
    write_u32(bytes, file + 4, 5);
    write_u16(bytes, file + 26, 1);
    if ext4 {
        write_u32(bytes, root + 32, EXT4_EXTENTS);
        write_extent(bytes, root + 40, 7);
        write_u32(bytes, file + 32, EXT4_EXTENTS);
        write_extent(bytes, file + 40, 8);
    } else {
        write_u32(bytes, root + 40, 7);
        write_u32(bytes, file + 40, 8);
    }

    let directory = 7 * 1024;
    write_directory_entry(bytes, directory, 2, 12, b".", 2);
    write_directory_entry(bytes, directory + 12, 2, 12, b"..", 2);
    write_directory_entry(bytes, directory + 24, 3, 1000, b"hello", 1);
    bytes[8 * 1024..8 * 1024 + 5].copy_from_slice(b"hello");

    let Ok(mut filesystem) = ExtFilesystem::mount(&mut *disk) else {
        return false;
    };
    if filesystem.kind() != if ext4 { ExtKind::Ext4 } else { ExtKind::Ext2 } {
        return false;
    }
    let mut output = [0u8; 5];
    if filesystem.read_file("/hello", &mut output) != Ok(5) || &output != b"hello" {
        return false;
    }
    if filesystem.write_at("/hello", 0, b"HELLO") != Ok(5) {
        return false;
    }
    output.fill(0);
    filesystem.read_file("/hello", &mut output) == Ok(5) && &output == b"HELLO"
}

fn write_extent(bytes: &mut [u8], offset: usize, physical: u32) {
    write_u16(bytes, offset, EXTENT_MAGIC);
    write_u16(bytes, offset + 2, 1);
    write_u16(bytes, offset + 4, 4);
    write_u16(bytes, offset + 6, 0);
    write_u32(bytes, offset + 12, 0);
    write_u16(bytes, offset + 16, 1);
    write_u16(bytes, offset + 18, 0);
    write_u32(bytes, offset + 20, physical);
}

fn write_directory_entry(
    bytes: &mut [u8],
    offset: usize,
    inode: u32,
    record_length: u16,
    name: &[u8],
    file_type: u8,
) {
    write_u32(bytes, offset, inode);
    write_u16(bytes, offset + 4, record_length);
    bytes[offset + 6] = name.len() as u8;
    bytes[offset + 7] = file_type;
    bytes[offset + 8..offset + 8 + name.len()].copy_from_slice(name);
}
