use crate::mm::address::PHYSICAL_MEMORY_OFFSET;

const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";
const SDT_HEADER_SIZE: usize = 36;
const MAX_TABLE_SIZE: usize = 1024 * 1024;
const MAX_PHYSICAL_ADDRESS: u64 = 0x1_0000_0000;
pub const MAX_CPUS: usize = 8;
pub const MAX_NUMA_RANGES: usize = 8;

#[derive(Clone, Copy)]
pub struct AcpiProcessor {
    pub apic_id: u32,
    pub acpi_id: u32,
    pub enabled: bool,
    pub numa_node: usize,
}

impl AcpiProcessor {
    const fn empty() -> Self {
        Self {
            apic_id: 0,
            acpi_id: 0,
            enabled: false,
            numa_node: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct AcpiNumaMemory {
    pub node: usize,
    pub start: u64,
    pub end: u64,
}

impl AcpiNumaMemory {
    const fn empty() -> Self {
        Self {
            node: 0,
            start: 0,
            end: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct CpuAffinity {
    apic_id: u32,
    domain: u32,
}

impl CpuAffinity {
    const fn empty() -> Self {
        Self {
            apic_id: 0,
            domain: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct MemoryAffinity {
    domain: u32,
    start: u64,
    end: u64,
}

impl MemoryAffinity {
    const fn empty() -> Self {
        Self {
            domain: 0,
            start: 0,
            end: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct AcpiInfo {
    pub revision: u8,
    pub used_xsdt: bool,
    pub lapic_address: u64,
    pub ioapic_address: Option<u64>,
    pub ioapic_gsi_base: u32,
    pub hpet_address: Option<u64>,
    pub legacy_pic: bool,
    pub processors: [AcpiProcessor; MAX_CPUS],
    pub processor_count: usize,
    pub numa_memory: [AcpiNumaMemory; MAX_NUMA_RANGES],
    pub numa_memory_count: usize,
    cpu_affinity: [CpuAffinity; MAX_CPUS],
    cpu_affinity_count: usize,
    memory_affinity: [MemoryAffinity; MAX_NUMA_RANGES],
    memory_affinity_count: usize,
}

impl AcpiInfo {
    const fn fallback() -> Self {
        Self {
            revision: 0,
            used_xsdt: false,
            lapic_address: 0xfee0_0000,
            ioapic_address: None,
            ioapic_gsi_base: 0,
            hpet_address: None,
            legacy_pic: true,
            processors: [AcpiProcessor::empty(); MAX_CPUS],
            processor_count: 0,
            numa_memory: [AcpiNumaMemory::empty(); MAX_NUMA_RANGES],
            numa_memory_count: 0,
            cpu_affinity: [CpuAffinity::empty(); MAX_CPUS],
            cpu_affinity_count: 0,
            memory_affinity: [MemoryAffinity::empty(); MAX_NUMA_RANGES],
            memory_affinity_count: 0,
        }
    }
}

pub fn discover() -> AcpiInfo {
    let mut info = AcpiInfo::fallback();
    let Some(rsdp) = find_rsdp() else {
        return info;
    };
    let revision = read_u8(rsdp + 15);
    info.revision = revision;
    let root = if revision >= 2 {
        let length = read_u32(rsdp + 20) as usize;
        let xsdt = read_u64(rsdp + 24);
        if length >= 36 && length <= 4096 && valid_range(rsdp, length) && checksum(rsdp, length) {
            validate_sdt(xsdt, b"XSDT").map(|length| (xsdt, length, 8usize))
        } else {
            None
        }
    } else {
        None
    }
    .or_else(|| {
        let rsdt = read_u32(rsdp + 16) as u64;
        validate_sdt(rsdt, b"RSDT").map(|length| (rsdt, length, 4usize))
    });

    let Some((root_address, root_length, entry_size)) = root else {
        return info;
    };
    info.used_xsdt = entry_size == 8;
    let entries = (root_length - SDT_HEADER_SIZE) / entry_size;
    for index in 0..entries {
        let entry = root_address + SDT_HEADER_SIZE as u64 + (index * entry_size) as u64;
        let table = if entry_size == 8 {
            read_u64(entry)
        } else {
            read_u32(entry) as u64
        };
        if table == 0 || table >= MAX_PHYSICAL_ADDRESS {
            continue;
        }
        match read_signature(table) {
            signature if &signature == b"APIC" => parse_madt(table, &mut info),
            signature if &signature == b"HPET" => parse_hpet(table, &mut info),
            signature if &signature == b"SRAT" => parse_srat(table, &mut info),
            _ => {}
        }
    }
    normalize_numa(&mut info);
    info
}

fn find_rsdp() -> Option<u64> {
    let ebda_segment = read_u16(0x40e) as u64;
    let ebda = ebda_segment << 4;
    if (0x80000..0xa0000).contains(&ebda) {
        if let Some(address) = scan_rsdp(ebda, (ebda + 1024).min(0xa0000)) {
            return Some(address);
        }
    }
    scan_rsdp(0xe0000, 0x100000)
}

fn scan_rsdp(start: u64, end: u64) -> Option<u64> {
    for address in (start..end).step_by(16) {
        if read_bytes::<8>(address) != *RSDP_SIGNATURE || !checksum(address, 20) {
            continue;
        }
        let revision = read_u8(address + 15);
        if revision < 2 {
            return Some(address);
        }
        let length = read_u32(address + 20) as usize;
        if (36..=4096).contains(&length)
            && valid_range(address, length)
            && checksum(address, length)
        {
            return Some(address);
        }
    }
    None
}

fn parse_madt(address: u64, info: &mut AcpiInfo) {
    let Some(length) = validate_sdt(address, b"APIC") else {
        return;
    };
    if length < 44 {
        return;
    }
    info.lapic_address = read_u32(address + 36) as u64;
    info.legacy_pic = read_u32(address + 40) & 1 != 0;
    let mut offset = 44usize;
    while offset + 2 <= length {
        let entry = address + offset as u64;
        let kind = read_u8(entry);
        let entry_length = read_u8(entry + 1) as usize;
        if entry_length < 2 || offset + entry_length > length {
            break;
        }
        match (kind, entry_length) {
            (0, 8..) => {
                let flags = read_u32(entry + 4);
                add_processor(
                    info,
                    read_u8(entry + 3) as u32,
                    read_u8(entry + 2) as u32,
                    flags & 3 != 0,
                );
            }
            (1, 12..) if info.ioapic_address.is_none() => {
                info.ioapic_address = Some(read_u32(entry + 4) as u64);
                info.ioapic_gsi_base = read_u32(entry + 8);
            }
            (5, 12..) => info.lapic_address = read_u64(entry + 4),
            (9, 16..) => {
                let apic_id = read_u32(entry + 4);
                let flags = read_u32(entry + 8);
                add_processor(info, apic_id, read_u32(entry + 12), flags & 3 != 0);
            }
            _ => {}
        }
        offset += entry_length;
    }
}

fn add_processor(info: &mut AcpiInfo, apic_id: u32, acpi_id: u32, enabled: bool) {
    if !enabled
        || apic_id > u8::MAX as u32
        || info.processors[..info.processor_count]
            .iter()
            .any(|processor| processor.apic_id == apic_id)
        || info.processor_count >= MAX_CPUS
    {
        return;
    }
    info.processors[info.processor_count] = AcpiProcessor {
        apic_id,
        acpi_id,
        enabled,
        numa_node: 0,
    };
    info.processor_count += 1;
}

fn parse_srat(address: u64, info: &mut AcpiInfo) {
    let Some(length) = validate_sdt(address, b"SRAT") else {
        return;
    };
    if length < 48 {
        return;
    }
    let mut offset = 48usize;
    while offset + 2 <= length {
        let entry = address + offset as u64;
        let kind = read_u8(entry);
        let entry_length = read_u8(entry + 1) as usize;
        if entry_length < 2 || offset + entry_length > length {
            break;
        }
        match (kind, entry_length) {
            (0, 16..) if read_u32(entry + 4) & 1 != 0 => {
                let domain = u32::from(read_u8(entry + 2))
                    | (u32::from(read_u8(entry + 9)) << 8)
                    | (u32::from(read_u8(entry + 10)) << 16)
                    | (u32::from(read_u8(entry + 11)) << 24);
                add_cpu_affinity(info, read_u8(entry + 3) as u32, domain);
            }
            (1, 40..) if read_u32(entry + 28) & 1 != 0 => {
                let start =
                    u64::from(read_u32(entry + 8)) | (u64::from(read_u32(entry + 12)) << 32);
                let size =
                    u64::from(read_u32(entry + 16)) | (u64::from(read_u32(entry + 20)) << 32);
                if let Some(end) = start.checked_add(size) {
                    add_memory_affinity(info, read_u32(entry + 2), start, end);
                }
            }
            (2, 24..) if read_u32(entry + 12) & 1 != 0 => {
                add_cpu_affinity(info, read_u32(entry + 8), read_u32(entry + 4));
            }
            _ => {}
        }
        offset += entry_length;
    }
}

fn add_cpu_affinity(info: &mut AcpiInfo, apic_id: u32, domain: u32) {
    if info.cpu_affinity_count >= MAX_CPUS {
        return;
    }
    info.cpu_affinity[info.cpu_affinity_count] = CpuAffinity { apic_id, domain };
    info.cpu_affinity_count += 1;
}

fn add_memory_affinity(info: &mut AcpiInfo, domain: u32, start: u64, end: u64) {
    if start >= end {
        return;
    }
    if let Some(range) = info.memory_affinity[..info.memory_affinity_count]
        .iter_mut()
        .find(|range| range.domain == domain)
    {
        range.start = range.start.min(start);
        range.end = range.end.max(end);
        return;
    }
    if info.memory_affinity_count >= MAX_NUMA_RANGES {
        return;
    }
    info.memory_affinity[info.memory_affinity_count] = MemoryAffinity { domain, start, end };
    info.memory_affinity_count += 1;
}

fn normalize_numa(info: &mut AcpiInfo) {
    for index in 0..info.memory_affinity_count {
        let range = info.memory_affinity[index];
        info.numa_memory[index] = AcpiNumaMemory {
            node: index,
            start: range.start,
            end: range.end,
        };
    }
    info.numa_memory_count = info.memory_affinity_count;
    for processor in &mut info.processors[..info.processor_count] {
        let Some(affinity) = info.cpu_affinity[..info.cpu_affinity_count]
            .iter()
            .find(|affinity| affinity.apic_id == processor.apic_id)
        else {
            continue;
        };
        if let Some(node) = info.memory_affinity[..info.memory_affinity_count]
            .iter()
            .position(|range| range.domain == affinity.domain)
        {
            processor.numa_node = node;
        }
    }
}

fn parse_hpet(address: u64, info: &mut AcpiInfo) {
    let Some(length) = validate_sdt(address, b"HPET") else {
        return;
    };
    if length < 56 || read_u8(address + 40) != 0 {
        return;
    }
    let base = read_u64(address + 44);
    if base < MAX_PHYSICAL_ADDRESS && base & 7 == 0 {
        info.hpet_address = Some(base);
    }
}

fn validate_sdt(address: u64, signature: &[u8; 4]) -> Option<usize> {
    if !valid_range(address, SDT_HEADER_SIZE) || &read_signature(address) != signature {
        return None;
    }
    let length = read_u32(address + 4) as usize;
    if !(SDT_HEADER_SIZE..=MAX_TABLE_SIZE).contains(&length)
        || !valid_range(address, length)
        || !checksum(address, length)
    {
        return None;
    }
    Some(length)
}

fn checksum(address: u64, length: usize) -> bool {
    unsafe {
        crate::c_fastpath::checksum8_is_zero(
            (PHYSICAL_MEMORY_OFFSET + address) as *const u8,
            length,
        )
    }
}

fn valid_range(address: u64, length: usize) -> bool {
    address != 0
        && address < MAX_PHYSICAL_ADDRESS
        && address
            .checked_add(length as u64)
            .is_some_and(|end| end <= MAX_PHYSICAL_ADDRESS)
}

fn read_signature(address: u64) -> [u8; 4] {
    read_bytes(address)
}

fn read_bytes<const N: usize>(address: u64) -> [u8; N] {
    let mut bytes = [0u8; N];
    for (offset, byte) in bytes.iter_mut().enumerate() {
        *byte = read_u8(address + offset as u64);
    }
    bytes
}

fn read_u8(address: u64) -> u8 {
    unsafe { core::ptr::read_volatile((PHYSICAL_MEMORY_OFFSET + address) as *const u8) }
}

fn read_u16(address: u64) -> u16 {
    unsafe { core::ptr::read_unaligned((PHYSICAL_MEMORY_OFFSET + address) as *const u16) }
}

fn read_u32(address: u64) -> u32 {
    unsafe { core::ptr::read_unaligned((PHYSICAL_MEMORY_OFFSET + address) as *const u32) }
}

fn read_u64(address: u64) -> u64 {
    unsafe { core::ptr::read_unaligned((PHYSICAL_MEMORY_OFFSET + address) as *const u64) }
}
