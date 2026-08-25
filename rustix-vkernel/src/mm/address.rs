use core::fmt;

pub const PAGE_SIZE: u64 = 4096;
#[cfg(target_arch = "x86_64")]
pub const MAX_DIRECT_MAPPED_PHYSICAL: u64 = 4 * 1024 * 1024 * 1024;
#[cfg(target_arch = "aarch64")]
pub const MAX_DIRECT_MAPPED_PHYSICAL: u64 = 16 * 1024 * 1024 * 1024;

#[cfg(target_arch = "x86_64")]
pub const KERNEL_BASE: u64 = 0xffff_ffff_8000_0000;
#[cfg(target_arch = "x86_64")]
pub const PHYSICAL_MEMORY_OFFSET: u64 = 0xffff_8000_0000_0000;
#[cfg(target_arch = "x86_64")]
pub const USER_ADDRESS_LIMIT: u64 = 0x0000_8000_0000_0000;

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct PhysicalAddress(u64);

impl PhysicalAddress {
    pub const fn new(address: u64) -> Option<Self> {
        if address & (PAGE_SIZE - 1) == 0 {
            Some(Self(address))
        } else {
            None
        }
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    #[cfg(target_arch = "x86_64")]
    pub const fn direct_mapped(self) -> u64 {
        PHYSICAL_MEMORY_OFFSET + self.0
    }
}

impl fmt::Debug for PhysicalAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "PhysAddr({:#x})", self.0)
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct VirtualAddress(u64);

impl VirtualAddress {
    pub const fn new(address: u64) -> Option<Self> {
        let canonical = address <= 0x0000_7fff_ffff_ffff || address >= 0xffff_8000_0000_0000;
        if address & (PAGE_SIZE - 1) == 0 && canonical {
            Some(Self(address))
        } else {
            None
        }
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub(crate) const fn page_index(self, level: u8) -> usize {
        ((self.0 >> (12 + 9 * (level as u64 - 1))) & 0x1ff) as usize
    }
}

impl fmt::Debug for VirtualAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "VirtAddr({:#x})", self.0)
    }
}
