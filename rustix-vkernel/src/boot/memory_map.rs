use core::mem::size_of;
use core::ptr;

pub const HANDOFF_MAGIC: u64 = 0x5255_5354_4d45_4d31;
const HANDOFF_VERSION: u32 = 1;
const MAX_REGIONS: usize = 256;
const MAX_DESCRIPTOR_COUNT: usize = MAX_REGIONS;
const MAX_BOUNDARIES: usize = MAX_REGIONS * 2;
const MAX_BOOT_DATA_SIZE: u64 = 1024 * 1024;
#[cfg(target_arch = "x86_64")]
const IDENTITY_MAP_LIMIT: u64 = 4 * 1024 * 1024 * 1024;
#[cfg(target_arch = "aarch64")]
const IDENTITY_MAP_LIMIT: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemorySource {
    BiosE820,
    Multiboot2,
    Uefi,
    DeviceTree,
    Unknown,
}

impl MemorySource {
    pub const fn name(self) -> &'static str {
        match self {
            Self::BiosE820 => "BIOS E820",
            Self::Multiboot2 => "Multiboot2",
            Self::Uefi => "UEFI",
            Self::DeviceTree => "device tree",
            Self::Unknown => "unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRegionKind {
    Usable,
    AcpiReclaimable,
    AcpiNonVolatile,
    Bootloader,
    Reserved,
    Bad,
}

#[derive(Clone, Copy, Debug)]
pub struct MemoryRegion {
    pub start: u64,
    pub end: u64,
    pub kind: MemoryRegionKind,
}

impl MemoryRegion {
    const EMPTY: Self = Self {
        start: 0,
        end: 0,
        kind: MemoryRegionKind::Reserved,
    };

    pub const fn len(self) -> u64 {
        self.end.saturating_sub(self.start)
    }
}

#[derive(Clone, Copy)]
pub struct MemoryMap {
    source: MemorySource,
    regions: [MemoryRegion; MAX_REGIONS],
    len: usize,
    boot_data: Option<(u64, u64)>,
}

impl MemoryMap {
    pub const fn new(source: MemorySource) -> Self {
        Self {
            source,
            regions: [MemoryRegion::EMPTY; MAX_REGIONS],
            len: 0,
            boot_data: None,
        }
    }

    pub const fn source(&self) -> MemorySource {
        self.source
    }

    pub fn regions(&self) -> &[MemoryRegion] {
        &self.regions[..self.len]
    }

    pub const fn boot_data_range(&self) -> Option<(u64, u64)> {
        self.boot_data
    }

    pub fn usable_bytes(&self) -> u64 {
        self.regions()
            .iter()
            .filter(|region| region.kind == MemoryRegionKind::Usable)
            .fold(0, |total, region| total.saturating_add(region.len()))
    }

    fn set_boot_data(&mut self, start: u64, end: u64) {
        if start < end {
            self.boot_data = Some((start, end));
        }
    }

    fn push(&mut self, start: u64, length: u64, kind: MemoryRegionKind) -> bool {
        let Some(end) = start.checked_add(length) else {
            return false;
        };
        let end = end.min(IDENTITY_MAP_LIMIT);
        if start >= end {
            return true;
        }
        if self.len == MAX_REGIONS {
            return false;
        }
        self.regions[self.len] = MemoryRegion { start, end, kind };
        self.len += 1;
        true
    }

    fn normalize(&mut self) {
        let raw = self.regions;
        let raw_len = self.len;
        let mut boundaries = [0u64; MAX_BOUNDARIES];
        let mut boundary_len = 0usize;
        for region in &raw[..raw_len] {
            boundaries[boundary_len] = region.start;
            boundaries[boundary_len + 1] = region.end;
            boundary_len += 2;
        }

        let unique = crate::c_fastpath::sort_unique_u64(&mut boundaries[..boundary_len]);

        self.len = 0;
        for pair in boundaries[..unique].windows(2) {
            let start = pair[0];
            let end = pair[1];
            let mut selected = None;
            for region in &raw[..raw_len] {
                if region.start <= start && region.end >= end {
                    selected = match selected {
                        Some(current) if kind_priority(current) >= kind_priority(region.kind) => {
                            Some(current)
                        }
                        _ => Some(region.kind),
                    };
                }
            }
            let Some(kind) = selected else {
                continue;
            };
            if self.len > 0 {
                let previous = &mut self.regions[self.len - 1];
                if previous.kind == kind && previous.end == start {
                    previous.end = end;
                    continue;
                }
            }
            if self.len == MAX_REGIONS {
                // Refuse a partially normalized map: losing a reserved overlap
                // could otherwise make firmware or MMIO pages allocatable.
                self.len = 0;
                return;
            }
            self.regions[self.len] = MemoryRegion { start, end, kind };
            self.len += 1;
        }
    }
}

fn kind_priority(kind: MemoryRegionKind) -> u8 {
    match kind {
        MemoryRegionKind::Usable => 0,
        MemoryRegionKind::Bootloader => 1,
        MemoryRegionKind::AcpiReclaimable => 2,
        MemoryRegionKind::AcpiNonVolatile => 3,
        MemoryRegionKind::Reserved => 4,
        MemoryRegionKind::Bad => 5,
    }
}

#[repr(C)]
pub struct MemoryMapHandoff {
    pub magic: u64,
    pub version: u32,
    pub descriptor_size: u32,
    pub descriptor_count: u32,
    pub reserved: u32,
    pub descriptors: u64,
}

#[repr(C, packed)]
struct E820Descriptor {
    start: u64,
    length: u64,
    kind: u32,
    attributes: u32,
}

#[repr(C, packed)]
struct UefiDescriptor {
    kind: u32,
    _padding: u32,
    physical_start: u64,
    _virtual_start: u64,
    pages: u64,
    attributes: u64,
}

pub(super) fn parse_handoff(address: u64, source: MemorySource) -> MemoryMap {
    let mut map = MemoryMap::new(source);
    if !valid_pointer(address, size_of::<MemoryMapHandoff>() as u64) {
        return map;
    }

    let handoff = unsafe { ptr::read_unaligned(physical_pointer::<MemoryMapHandoff>(address)) };
    let descriptor_size = handoff.descriptor_size as usize;
    let descriptor_count = handoff.descriptor_count as usize;
    if descriptor_count > MAX_DESCRIPTOR_COUNT {
        return map;
    }
    let Some(data_size) = descriptor_size.checked_mul(descriptor_count) else {
        return map;
    };
    if handoff.magic != HANDOFF_MAGIC
        || handoff.version != HANDOFF_VERSION
        || descriptor_size == 0
        || !valid_pointer(handoff.descriptors, data_size as u64)
    {
        return map;
    }

    map.set_boot_data(
        address.min(handoff.descriptors),
        address
            .saturating_add(size_of::<MemoryMapHandoff>() as u64)
            .max(handoff.descriptors.saturating_add(data_size as u64)),
    );

    for index in 0..descriptor_count {
        let descriptor = handoff.descriptors + (index * descriptor_size) as u64;
        match source {
            MemorySource::BiosE820 if descriptor_size >= size_of::<E820Descriptor>() => {
                let entry =
                    unsafe { ptr::read_unaligned(physical_pointer::<E820Descriptor>(descriptor)) };
                if entry.attributes & 1 != 0 {
                    if !map.push(entry.start, entry.length, e820_kind(entry.kind)) {
                        return MemoryMap::new(source);
                    }
                }
            }
            MemorySource::Uefi if descriptor_size >= size_of::<UefiDescriptor>() => {
                let entry =
                    unsafe { ptr::read_unaligned(physical_pointer::<UefiDescriptor>(descriptor)) };
                if let Some(length) = entry.pages.checked_mul(4096) {
                    if !map.push(
                        entry.physical_start,
                        length,
                        uefi_kind(entry.kind, entry.attributes),
                    ) {
                        return MemoryMap::new(source);
                    }
                }
            }
            _ => return MemoryMap::new(source),
        }
    }
    map.normalize();
    map
}

const FDT_MAGIC: u32 = 0xd00d_feed;
const FDT_BEGIN_NODE: u32 = 1;
const FDT_END_NODE: u32 = 2;
const FDT_PROP: u32 = 3;
const FDT_NOP: u32 = 4;
const FDT_END: u32 = 9;

pub(super) fn parse_device_tree(address: u64) -> MemoryMap {
    let source = MemorySource::DeviceTree;
    let mut map = MemoryMap::new(source);
    if address & 7 != 0 || !valid_pointer(address, 40) || read_be_u32(address) != FDT_MAGIC {
        return map;
    }

    let total_size = read_be_u32(address + 4) as u64;
    if !(40..=MAX_BOOT_DATA_SIZE).contains(&total_size) || !valid_pointer(address, total_size) {
        return map;
    }
    let structure_offset = read_be_u32(address + 8) as u64;
    let strings_offset = read_be_u32(address + 12) as u64;
    let reservations_offset = read_be_u32(address + 16) as u64;
    let strings_size = read_be_u32(address + 32) as u64;
    let structure_size = read_be_u32(address + 36) as u64;
    if !fdt_range_valid(structure_offset, structure_size, total_size)
        || !fdt_range_valid(strings_offset, strings_size, total_size)
        || reservations_offset >= total_size
    {
        return map;
    }

    map.set_boot_data(address, address + total_size);
    if !parse_fdt_reservations(address, reservations_offset, total_size, &mut map)
        || !parse_fdt_structure(
            address,
            structure_offset,
            structure_size,
            strings_offset,
            strings_size,
            &mut map,
        )
    {
        return MemoryMap::new(source);
    }
    map.normalize();
    map
}

fn parse_fdt_reservations(base: u64, offset: u64, total_size: u64, map: &mut MemoryMap) -> bool {
    let mut cursor = offset;
    loop {
        if !fdt_range_valid(cursor, 16, total_size) {
            return false;
        }
        let start = read_be_u64(base + cursor);
        let size = read_be_u64(base + cursor + 8);
        cursor += 16;
        if start == 0 && size == 0 {
            return true;
        }
        if !map.push(start, size, MemoryRegionKind::Reserved) {
            return false;
        }
    }
}

fn parse_fdt_structure(
    base: u64,
    structure_offset: u64,
    structure_size: u64,
    strings_offset: u64,
    strings_size: u64,
    map: &mut MemoryMap,
) -> bool {
    let structure_end = structure_offset + structure_size;
    let mut cursor = structure_offset;
    let mut depth = 0usize;
    let mut address_cells = 2usize;
    let mut size_cells = 1usize;
    let mut memory_depth = None;
    let mut reserved_container_depth = None;
    let mut reserved_node_depth = None;

    while cursor + 4 <= structure_end {
        let token = read_be_u32(base + cursor);
        cursor += 4;
        match token {
            FDT_BEGIN_NODE => {
                let Some((name_length, name_is_memory, name_is_reserved)) =
                    fdt_node_name(base, cursor, structure_end)
                else {
                    return false;
                };
                depth += 1;
                if depth == 2 && name_is_memory {
                    memory_depth = Some(depth);
                }
                if depth == 2 && name_is_reserved {
                    reserved_container_depth = Some(depth);
                } else if reserved_container_depth.is_some_and(|level| depth == level + 1) {
                    reserved_node_depth = Some(depth);
                }
                cursor = align4(cursor + name_length + 1);
                if cursor > structure_end {
                    return false;
                }
            }
            FDT_END_NODE => {
                if depth == 0 {
                    return false;
                }
                if memory_depth == Some(depth) {
                    memory_depth = None;
                }
                if reserved_node_depth == Some(depth) {
                    reserved_node_depth = None;
                }
                if reserved_container_depth == Some(depth) {
                    reserved_container_depth = None;
                }
                depth -= 1;
            }
            FDT_PROP => {
                if cursor + 8 > structure_end {
                    return false;
                }
                let value_size = read_be_u32(base + cursor) as u64;
                let name_offset = read_be_u32(base + cursor + 4) as u64;
                cursor += 8;
                if cursor + value_size > structure_end || name_offset >= strings_size {
                    return false;
                }

                if depth == 1
                    && value_size == 4
                    && fdt_string_equals(
                        base,
                        strings_offset,
                        strings_size,
                        name_offset,
                        b"#address-cells",
                    )
                {
                    address_cells = read_be_u32(base + cursor) as usize;
                } else if depth == 1
                    && value_size == 4
                    && fdt_string_equals(
                        base,
                        strings_offset,
                        strings_size,
                        name_offset,
                        b"#size-cells",
                    )
                {
                    size_cells = read_be_u32(base + cursor) as usize;
                } else if (memory_depth == Some(depth) || reserved_node_depth == Some(depth))
                    && fdt_string_equals(base, strings_offset, strings_size, name_offset, b"reg")
                {
                    let kind = if memory_depth == Some(depth) {
                        MemoryRegionKind::Usable
                    } else {
                        MemoryRegionKind::Reserved
                    };
                    if !parse_fdt_reg(
                        base + cursor,
                        value_size,
                        address_cells,
                        size_cells,
                        kind,
                        map,
                    ) {
                        return false;
                    }
                }
                cursor = align4(cursor + value_size);
            }
            FDT_NOP => {}
            FDT_END => return depth == 0,
            _ => return false,
        }
    }
    false
}

fn parse_fdt_reg(
    address: u64,
    value_size: u64,
    address_cells: usize,
    size_cells: usize,
    kind: MemoryRegionKind,
    map: &mut MemoryMap,
) -> bool {
    if !(1..=2).contains(&address_cells) || !(1..=2).contains(&size_cells) {
        return false;
    }
    let tuple_cells = address_cells + size_cells;
    let tuple_size = (tuple_cells * 4) as u64;
    if value_size == 0 || value_size % tuple_size != 0 {
        return false;
    }
    let mut offset = 0u64;
    while offset < value_size {
        let start = read_fdt_cells(address + offset, address_cells);
        let size = read_fdt_cells(address + offset + (address_cells * 4) as u64, size_cells);
        if size != 0 && !map.push(start, size, kind) {
            return false;
        }
        offset += tuple_size;
    }
    true
}

fn fdt_node_name(base: u64, start: u64, end: u64) -> Option<(u64, bool, bool)> {
    let mut length = 0u64;
    let mut memory = true;
    let mut reserved = true;
    let memory_name = b"memory";
    let reserved_name = b"reserved-memory";
    while start + length < end {
        let byte = unsafe { ptr::read_volatile(physical_pointer::<u8>(base + start + length)) };
        if byte == 0 {
            return Some((length, memory, reserved));
        }
        if length < memory_name.len() as u64 {
            memory &= memory_name.get(length as usize).copied() == Some(byte);
        } else if length == memory_name.len() as u64 {
            memory &= byte == b'@';
        }
        reserved &= reserved_name.get(length as usize).copied() == Some(byte);
        length += 1;
    }
    None
}

fn fdt_string_equals(
    base: u64,
    strings_offset: u64,
    strings_size: u64,
    name_offset: u64,
    expected: &[u8],
) -> bool {
    if name_offset + expected.len() as u64 >= strings_size {
        return false;
    }
    for (index, expected_byte) in expected.iter().enumerate() {
        let byte = unsafe {
            ptr::read_volatile(physical_pointer::<u8>(
                base + strings_offset + name_offset + index as u64,
            ))
        };
        if byte != *expected_byte {
            return false;
        }
    }
    unsafe {
        ptr::read_volatile(physical_pointer::<u8>(
            base + strings_offset + name_offset + expected.len() as u64,
        )) == 0
    }
}

fn read_fdt_cells(address: u64, cells: usize) -> u64 {
    let mut value = 0u64;
    for index in 0..cells {
        value = (value << 32) | read_be_u32(address + (index * 4) as u64) as u64;
    }
    value
}

fn read_be_u32(address: u64) -> u32 {
    u32::from_be(unsafe { ptr::read_unaligned(physical_pointer::<u32>(address)) })
}

fn read_be_u64(address: u64) -> u64 {
    u64::from_be(unsafe { ptr::read_unaligned(physical_pointer::<u64>(address)) })
}

fn fdt_range_valid(offset: u64, size: u64, total_size: u64) -> bool {
    offset
        .checked_add(size)
        .is_some_and(|end| offset < total_size && end <= total_size)
}

const fn align4(value: u64) -> u64 {
    value.saturating_add(3) & !3
}

pub(super) fn parse_multiboot2(address: u64) -> MemoryMap {
    let mut map = MemoryMap::new(MemorySource::Multiboot2);
    if !valid_pointer(address, 8) || address & 7 != 0 {
        return map;
    }
    let total_size = unsafe { ptr::read_unaligned(physical_pointer::<u32>(address)) as u64 };
    if !(16..=MAX_BOOT_DATA_SIZE).contains(&total_size) || !valid_pointer(address, total_size) {
        return map;
    }
    map.set_boot_data(address, address + total_size);

    let mut offset = 8u64;
    while offset + 8 <= total_size {
        let tag = address + offset;
        let kind = unsafe { ptr::read_unaligned(physical_pointer::<u32>(tag)) };
        let size = unsafe { ptr::read_unaligned(physical_pointer::<u32>(tag + 4)) as u64 };
        if size < 8 || offset + size > total_size {
            return MemoryMap::new(MemorySource::Multiboot2);
        }
        if kind == 0 {
            break;
        }
        if kind == 6 && size >= 16 {
            if !parse_multiboot_mmap(tag, size, &mut map) {
                return MemoryMap::new(MemorySource::Multiboot2);
            }
        }
        let Some(next) = offset
            .checked_add(size)
            .and_then(|value| value.checked_add(7))
        else {
            return MemoryMap::new(MemorySource::Multiboot2);
        };
        offset = next & !7;
    }
    map.normalize();
    map
}

fn parse_multiboot_mmap(tag: u64, tag_size: u64, map: &mut MemoryMap) -> bool {
    let entry_size = unsafe { ptr::read_unaligned(physical_pointer::<u32>(tag + 8)) as u64 };
    if entry_size < 24 || entry_size > 256 {
        return false;
    }
    let mut offset = 16u64;
    while offset + entry_size <= tag_size {
        let entry = tag + offset;
        let start = unsafe { ptr::read_unaligned(physical_pointer::<u64>(entry)) };
        let length = unsafe { ptr::read_unaligned(physical_pointer::<u64>(entry + 8)) };
        let kind = unsafe { ptr::read_unaligned(physical_pointer::<u32>(entry + 16)) };
        if !map.push(start, length, e820_kind(kind)) {
            return false;
        }
        offset += entry_size;
    }
    true
}

fn valid_pointer(address: u64, size: u64) -> bool {
    address != 0
        && size <= MAX_BOOT_DATA_SIZE
        && address
            .checked_add(size)
            .is_some_and(|end| end <= IDENTITY_MAP_LIMIT)
}

#[cfg(target_arch = "x86_64")]
fn physical_pointer<T>(address: u64) -> *const T {
    (crate::mm::address::PHYSICAL_MEMORY_OFFSET + address) as *const T
}

#[cfg(not(target_arch = "x86_64"))]
fn physical_pointer<T>(address: u64) -> *const T {
    address as *const T
}

fn e820_kind(kind: u32) -> MemoryRegionKind {
    match kind {
        1 => MemoryRegionKind::Usable,
        3 => MemoryRegionKind::AcpiReclaimable,
        4 => MemoryRegionKind::AcpiNonVolatile,
        5 => MemoryRegionKind::Bad,
        _ => MemoryRegionKind::Reserved,
    }
}

fn uefi_kind(kind: u32, attributes: u64) -> MemoryRegionKind {
    const EFI_MEMORY_RUNTIME: u64 = 1 << 63;
    if attributes & EFI_MEMORY_RUNTIME != 0 {
        return MemoryRegionKind::Reserved;
    }
    match kind {
        1..=4 => MemoryRegionKind::Bootloader,
        7 => MemoryRegionKind::Usable,
        9 => MemoryRegionKind::AcpiReclaimable,
        10 => MemoryRegionKind::AcpiNonVolatile,
        8 => MemoryRegionKind::Bad,
        _ => MemoryRegionKind::Reserved,
    }
}
