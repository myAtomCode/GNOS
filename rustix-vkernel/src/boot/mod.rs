mod memory_map;

#[cfg(target_arch = "aarch64")]
mod device_tree;

#[cfg(target_arch = "aarch64")]
pub use device_tree::{discover_hardware, CpuBootInfo, HardwareDiscovery, PsciConduit};
pub use memory_map::{MemoryMap, MemoryRegionKind, MemorySource};

pub const PROTOCOL_BIOS: u64 = 0x5258_4249;
pub const PROTOCOL_MULTIBOOT2: u64 = 0x36d7_6289;
pub const PROTOCOL_UEFI: u64 = 0x5545_4649;
pub const PROTOCOL_DEVICE_TREE: u64 = 0x5258_4454;

pub fn parse_memory_map(protocol: u64, information: u64) -> MemoryMap {
    match protocol {
        PROTOCOL_BIOS => memory_map::parse_handoff(information, MemorySource::BiosE820),
        PROTOCOL_MULTIBOOT2 => memory_map::parse_multiboot2(information),
        PROTOCOL_UEFI => memory_map::parse_handoff(information, MemorySource::Uefi),
        PROTOCOL_DEVICE_TREE => memory_map::parse_device_tree(information),
        _ => MemoryMap::new(MemorySource::Unknown),
    }
}
