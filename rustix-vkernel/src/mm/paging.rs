use core::arch::asm;
use core::ops::{BitOr, BitOrAssign};

use crate::mm::address::{
    PhysicalAddress, VirtualAddress, KERNEL_BASE, MAX_DIRECT_MAPPED_PHYSICAL, PAGE_SIZE,
    PHYSICAL_MEMORY_OFFSET, USER_ADDRESS_LIMIT,
};
use crate::mm::frame;
use crate::mm::user::{checked_user_range, UserAccess, UserRange};
use crate::sync::IrqSpinMutex;

const ENTRY_COUNT: usize = 512;
const ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;
const PAGE_2M_MASK: u64 = 0x000f_ffff_ffe0_0000;
const PAGE_1G_MASK: u64 = 0x000f_ffff_c000_0000;
const ENTRY_PRESENT: u64 = 1 << 0;
const ENTRY_WRITABLE: u64 = 1 << 1;
const ENTRY_USER: u64 = 1 << 2;
const ENTRY_WRITE_THROUGH: u64 = 1 << 3;
const ENTRY_CACHE_DISABLE: u64 = 1 << 4;
const ENTRY_HUGE: u64 = 1 << 7;
const ENTRY_GLOBAL: u64 = 1 << 8;
const ENTRY_NO_EXECUTE: u64 = 1 << 63;
const KERNEL_PML4_START: usize = 256;
const HIGH_KERNEL_P3_INDEX: usize = 510;
const EARLY_STACK_GUARD: u64 = KERNEL_BASE + 0x00fe_f000;
const EARLY_STACK_START: u64 = KERNEL_BASE + 0x00ff_0000;
const EARLY_STACK_END: u64 = KERNEL_BASE + 0x0100_0000;

// Page-table mutation precedes frame allocation in the global lock order.
static PAGE_TABLE_LOCK: IrqSpinMutex<(), 5> = IrqSpinMutex::new(());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MapFlags(u64);

impl MapFlags {
    pub const PRESENT: Self = Self(ENTRY_PRESENT);
    pub const WRITABLE: Self = Self(ENTRY_WRITABLE);
    pub const USER: Self = Self(ENTRY_USER);
    pub const WRITE_THROUGH: Self = Self(ENTRY_WRITE_THROUGH);
    pub const CACHE_DISABLE: Self = Self(ENTRY_CACHE_DISABLE);
    pub const GLOBAL: Self = Self(ENTRY_GLOBAL);
    pub const NO_EXECUTE: Self = Self(ENTRY_NO_EXECUTE);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for MapFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for MapFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MapError {
    OutOfMemory,
    AlreadyMapped,
    NotMapped,
    HugePageConflict,
    InvalidTable,
    InvalidAddress,
    PermissionDenied,
    AddressSpaceBusy,
}

#[repr(C, align(4096))]
struct PageTable {
    entries: [u64; ENTRY_COUNT],
}

pub struct ActivePageTable {
    root: PhysicalAddress,
}

impl ActivePageTable {
    pub fn current() -> Result<Self, MapError> {
        let cr3: u64;
        unsafe {
            asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
        }
        PhysicalAddress::new(cr3 & ADDRESS_MASK)
            .map(|root| Self { root })
            .ok_or(MapError::InvalidTable)
    }

    pub fn map_4k(
        &mut self,
        virtual_address: VirtualAddress,
        physical_address: PhysicalAddress,
        flags: MapFlags,
    ) -> Result<(), MapError> {
        let _guard = PAGE_TABLE_LOCK.lock();
        self.map_4k_locked(virtual_address, physical_address, flags)
    }

    fn map_4k_locked(
        &mut self,
        virtual_address: VirtualAddress,
        physical_address: PhysicalAddress,
        flags: MapFlags,
    ) -> Result<(), MapError> {
        if physical_address.as_u64() >= MAX_DIRECT_MAPPED_PHYSICAL {
            return Err(MapError::InvalidAddress);
        }
        // A writable executable page creates a trivial code-injection primitive.
        if flags.contains(MapFlags::WRITABLE) && !flags.contains(MapFlags::NO_EXECUTE) {
            return Err(MapError::PermissionDenied);
        }
        let mut table_address = self.root;
        for level in (2..=4).rev() {
            let index = virtual_address.page_index(level);
            let table = unsafe { table_mut(table_address)? };
            let entry = &mut table.entries[index];
            if *entry & ENTRY_PRESENT != 0 && *entry & ENTRY_HUGE != 0 {
                return Err(MapError::HugePageConflict);
            }
            if *entry & ENTRY_PRESENT == 0 {
                let frame = frame::allocate().map_err(|_| MapError::OutOfMemory)?;
                unsafe { zero_frame(frame) };
                let user = if flags.contains(MapFlags::USER) {
                    ENTRY_USER
                } else {
                    0
                };
                *entry = frame.as_u64() | ENTRY_PRESENT | ENTRY_WRITABLE | user;
            } else if flags.contains(MapFlags::USER) {
                *entry |= ENTRY_USER;
            }
            table_address =
                PhysicalAddress::new(*entry & ADDRESS_MASK).ok_or(MapError::InvalidTable)?;
        }

        let table = unsafe { table_mut(table_address)? };
        let entry = &mut table.entries[virtual_address.page_index(1)];
        if *entry & ENTRY_PRESENT != 0 {
            return Err(MapError::AlreadyMapped);
        }
        *entry = physical_address.as_u64() | flags.0 | ENTRY_PRESENT;
        invalidate(virtual_address);
        Ok(())
    }

    pub fn unmap_4k(
        &mut self,
        virtual_address: VirtualAddress,
    ) -> Result<PhysicalAddress, MapError> {
        let _guard = PAGE_TABLE_LOCK.lock();
        let mut parents = [self.root; 3];
        let mut parent_indices = [0usize; 3];
        let mut table_address = self.root;
        for (depth, level) in (2..=4).rev().enumerate() {
            let table = unsafe { table_ref(table_address)? };
            let index = virtual_address.page_index(level);
            let entry = table.entries[index];
            if entry & ENTRY_PRESENT == 0 {
                return Err(MapError::NotMapped);
            }
            if entry & ENTRY_HUGE != 0 {
                return Err(MapError::HugePageConflict);
            }
            parents[depth] = table_address;
            parent_indices[depth] = index;
            table_address =
                PhysicalAddress::new(entry & ADDRESS_MASK).ok_or(MapError::InvalidTable)?;
        }
        let table = unsafe { table_mut(table_address)? };
        let entry = &mut table.entries[virtual_address.page_index(1)];
        if *entry & ENTRY_PRESENT == 0 {
            return Err(MapError::NotMapped);
        }
        let physical = PhysicalAddress::new(*entry & ADDRESS_MASK).ok_or(MapError::InvalidTable)?;
        *entry = 0;
        invalidate(virtual_address);

        // Reclaim now-empty intermediate tables. Without this, every temporary
        // mapping permanently leaked up to three physical frames.
        let mut child = table_address;
        for depth in (0..3).rev() {
            if !unsafe { table_ref(child)? }
                .entries
                .iter()
                .all(|entry| *entry == 0)
            {
                break;
            }
            frame::deallocate(child).map_err(|_| MapError::InvalidTable)?;
            let parent = unsafe { table_mut(parents[depth])? };
            parent.entries[parent_indices[depth]] = 0;
            child = parents[depth];
        }
        Ok(physical)
    }

    pub fn map_range(
        &mut self,
        virtual_start: VirtualAddress,
        physical_start: PhysicalAddress,
        pages: usize,
        flags: MapFlags,
    ) -> Result<(), MapError> {
        if pages == 0 {
            return Err(MapError::InvalidAddress);
        }
        let mut mapped = 0usize;
        while mapped < pages {
            let offset = (mapped as u64)
                .checked_mul(PAGE_SIZE)
                .ok_or(MapError::InvalidAddress)?;
            let virtual_address = VirtualAddress::new(
                virtual_start
                    .as_u64()
                    .checked_add(offset)
                    .ok_or(MapError::InvalidAddress)?,
            )
            .ok_or(MapError::InvalidAddress)?;
            let physical_address = PhysicalAddress::new(
                physical_start
                    .as_u64()
                    .checked_add(offset)
                    .ok_or(MapError::InvalidAddress)?,
            )
            .ok_or(MapError::InvalidAddress)?;
            if let Err(error) = self.map_4k(virtual_address, physical_address, flags) {
                for rollback in 0..mapped {
                    if let Some(address) =
                        VirtualAddress::new(virtual_start.as_u64() + rollback as u64 * PAGE_SIZE)
                    {
                        let _ = self.unmap_4k(address);
                    }
                }
                return Err(error);
            }
            mapped += 1;
        }
        Ok(())
    }

    pub fn allocate_range(
        &mut self,
        virtual_start: VirtualAddress,
        pages: usize,
        flags: MapFlags,
    ) -> Result<(), MapError> {
        if pages == 0 {
            return Err(MapError::InvalidAddress);
        }
        let mut mapped = 0usize;
        while mapped < pages {
            let address = VirtualAddress::new(
                virtual_start
                    .as_u64()
                    .checked_add(mapped as u64 * PAGE_SIZE)
                    .ok_or(MapError::InvalidAddress)?,
            )
            .ok_or(MapError::InvalidAddress)?;
            let physical = match frame::allocate() {
                Ok(physical) => physical,
                Err(_) => {
                    let _ = self.release_range(virtual_start, mapped, true);
                    return Err(MapError::OutOfMemory);
                }
            };
            // POSIX：匿名映射与新 brk 页必须为零填充
            unsafe {
                core::ptr::write_bytes(physical.direct_mapped() as *mut u8, 0, PAGE_SIZE as usize);
            }
            if let Err(error) = self.map_4k(address, physical, flags) {
                let _ = frame::deallocate(physical);
                let _ = self.release_range(virtual_start, mapped, true);
                return Err(error);
            }
            mapped += 1;
        }
        Ok(())
    }

    pub fn release_range(
        &mut self,
        virtual_start: VirtualAddress,
        pages: usize,
        release_frames: bool,
    ) -> Result<(), MapError> {
        for index in 0..pages {
            let address = VirtualAddress::new(
                virtual_start
                    .as_u64()
                    .checked_add(index as u64 * PAGE_SIZE)
                    .ok_or(MapError::InvalidAddress)?,
            )
            .ok_or(MapError::InvalidAddress)?;
            let physical = self.unmap_4k(address)?;
            if release_frames {
                frame::deallocate(physical).map_err(|_| MapError::InvalidTable)?;
            }
        }
        Ok(())
    }

    pub fn translate(&self, virtual_address: VirtualAddress) -> Option<PhysicalAddress> {
        let mut table_address = self.root;
        for level in (1..=4).rev() {
            let table = unsafe { table_ref(table_address).ok()? };
            let entry = table.entries[virtual_address.page_index(level)];
            if entry & ENTRY_PRESENT == 0 {
                return None;
            }
            if level == 3 && entry & ENTRY_HUGE != 0 {
                let base = entry & PAGE_1G_MASK;
                let offset = virtual_address.as_u64() & ((1 << 30) - 1);
                return PhysicalAddress::new((base + offset) & !(PAGE_SIZE - 1));
            }
            if level == 2 && entry & ENTRY_HUGE != 0 {
                let base = entry & PAGE_2M_MASK;
                let offset = virtual_address.as_u64() & ((1 << 21) - 1);
                return PhysicalAddress::new((base + offset) & !(PAGE_SIZE - 1));
            }
            if level == 1 {
                return PhysicalAddress::new(entry & ADDRESS_MASK);
            }
            table_address = PhysicalAddress::new(entry & ADDRESS_MASK)?;
        }
        None
    }

    fn user_mapping(&self, address: u64, write: bool) -> Result<PhysicalAddress, UserCopyError> {
        if address < PAGE_SIZE || address >= USER_ADDRESS_LIMIT {
            return Err(UserCopyError::InvalidRange);
        }
        let page =
            VirtualAddress::new(address & !(PAGE_SIZE - 1)).ok_or(UserCopyError::InvalidRange)?;
        let mut table_address = self.root;
        for level in (1..=4).rev() {
            let table =
                unsafe { table_ref(table_address) }.map_err(|_| UserCopyError::NotMapped)?;
            let entry = table.entries[page.page_index(level)];
            if entry & ENTRY_PRESENT == 0 {
                return Err(UserCopyError::NotMapped);
            }
            // Every level must permit user access. Checking only the leaf would let a
            // malformed upper-level entry bypass the address-space boundary.
            if entry & ENTRY_USER == 0 {
                return Err(UserCopyError::PermissionDenied);
            }
            if write && entry & ENTRY_WRITABLE == 0 {
                return Err(UserCopyError::PermissionDenied);
            }
            if level == 1 {
                let base = entry & ADDRESS_MASK;
                return PhysicalAddress::new(base).ok_or(UserCopyError::NotMapped);
            }
            if entry & ENTRY_HUGE != 0 {
                return Err(UserCopyError::PermissionDenied);
            }
            table_address =
                PhysicalAddress::new(entry & ADDRESS_MASK).ok_or(UserCopyError::NotMapped)?;
        }
        Err(UserCopyError::NotMapped)
    }

    pub fn set_flags_4k(
        &mut self,
        virtual_address: VirtualAddress,
        flags: MapFlags,
    ) -> Result<(), MapError> {
        if flags.contains(MapFlags::WRITABLE) && !flags.contains(MapFlags::NO_EXECUTE) {
            return Err(MapError::PermissionDenied);
        }
        let _guard = PAGE_TABLE_LOCK.lock();
        let (table_address, index) = self.walk_to_leaf(virtual_address)?;
        let table = unsafe { table_mut(table_address)? };
        let entry = &mut table.entries[index];
        if *entry & ENTRY_PRESENT == 0 {
            return Err(MapError::NotMapped);
        }
        let address = *entry & ADDRESS_MASK;
        *entry = address | flags.0 | ENTRY_PRESENT;
        invalidate(virtual_address);
        Ok(())
    }

    pub fn split_2m(&mut self, virtual_address: VirtualAddress) -> Result<(), MapError> {
        let _guard = PAGE_TABLE_LOCK.lock();
        let p3 = self.next_table(self.root, virtual_address.page_index(4))?;
        let p2 = self.next_table(p3, virtual_address.page_index(3))?;
        let table = unsafe { table_mut(p2)? };
        let entry = &mut table.entries[virtual_address.page_index(2)];
        if *entry & ENTRY_PRESENT == 0 {
            return Err(MapError::NotMapped);
        }
        if *entry & ENTRY_HUGE == 0 {
            return Ok(());
        }

        let old_entry = *entry;
        let base = old_entry & PAGE_2M_MASK;
        let flags = old_entry & !ADDRESS_MASK & !ENTRY_HUGE;
        let new_table = frame::allocate().map_err(|_| MapError::OutOfMemory)?;
        unsafe { zero_frame(new_table) };
        let leaf = unsafe { table_mut(new_table)? };
        for index in 0..ENTRY_COUNT {
            leaf.entries[index] = base + (index as u64 * PAGE_SIZE) | flags | ENTRY_PRESENT;
        }
        *entry = new_table.as_u64() | ENTRY_PRESENT | ENTRY_WRITABLE | (old_entry & ENTRY_USER);
        reload_cr3(self.root);
        Ok(())
    }

    fn make_identity_and_direct_map_nx(&mut self) -> Result<(), MapError> {
        let root = unsafe { table_mut(self.root)? };
        if root.entries[0] & ENTRY_PRESENT != 0 {
            root.entries[0] |= ENTRY_NO_EXECUTE;
        }
        if root.entries[KERNEL_PML4_START] & ENTRY_PRESENT != 0 {
            root.entries[KERNEL_PML4_START] |= ENTRY_NO_EXECUTE;
        }
        reload_cr3(self.root);
        Ok(())
    }

    fn make_unused_high_kernel_pages_nx(
        &mut self,
        kernel_physical_end: u64,
    ) -> Result<(), MapError> {
        let p3 = self.next_table(self.root, 511)?;
        let p2 = self.next_table(p3, HIGH_KERNEL_P3_INDEX)?;
        let table = unsafe { table_mut(p2)? };
        let last_kernel_page = align_up(kernel_physical_end, 1 << 21) / (1 << 21);
        for (index, entry) in table.entries.iter_mut().enumerate() {
            if index as u64 >= last_kernel_page && *entry & ENTRY_PRESENT != 0 {
                *entry |= ENTRY_NO_EXECUTE;
            }
        }
        reload_cr3(self.root);
        Ok(())
    }

    fn walk_to_leaf(
        &self,
        virtual_address: VirtualAddress,
    ) -> Result<(PhysicalAddress, usize), MapError> {
        let mut table_address = self.root;
        for level in (2..=4).rev() {
            let table = unsafe { table_ref(table_address)? };
            let entry = table.entries[virtual_address.page_index(level)];
            if entry & ENTRY_PRESENT == 0 {
                return Err(MapError::NotMapped);
            }
            if entry & ENTRY_HUGE != 0 {
                return Err(MapError::HugePageConflict);
            }
            table_address =
                PhysicalAddress::new(entry & ADDRESS_MASK).ok_or(MapError::InvalidTable)?;
        }
        Ok((table_address, virtual_address.page_index(1)))
    }

    fn next_table(
        &self,
        table_address: PhysicalAddress,
        index: usize,
    ) -> Result<PhysicalAddress, MapError> {
        let table = unsafe { table_ref(table_address)? };
        let entry = table.entries[index];
        if entry & ENTRY_PRESENT == 0 {
            return Err(MapError::NotMapped);
        }
        if entry & ENTRY_HUGE != 0 {
            return Err(MapError::HugePageConflict);
        }
        PhysicalAddress::new(entry & ADDRESS_MASK).ok_or(MapError::InvalidTable)
    }
}

pub struct AddressSpace {
    root: PhysicalAddress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserCopyError {
    InvalidRange,
    NotMapped,
    PermissionDenied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UserStack {
    pub guard_page: u64,
    pub bottom: u64,
    pub top: u64,
}

impl AddressSpace {
    pub fn new_user() -> Result<Self, MapError> {
        let kernel = ActivePageTable::current()?;
        let root = frame::allocate().map_err(|_| MapError::OutOfMemory)?;
        unsafe { zero_frame(root) };

        let source = unsafe { table_ref(kernel.root)? };
        let target = unsafe { table_mut(root)? };
        // The lower canonical half starts empty; only supervisor-only high mappings are shared.
        target.entries[KERNEL_PML4_START..].copy_from_slice(&source.entries[KERNEL_PML4_START..]);
        Ok(Self { root })
    }

    pub const fn root(&self) -> PhysicalAddress {
        self.root
    }

    pub fn from_root(root: u64) -> Result<Self, MapError> {
        let root = PhysicalAddress::new(root & ADDRESS_MASK).ok_or(MapError::InvalidTable)?;
        unsafe { table_ref(root)? };
        Ok(Self { root })
    }

    pub fn clone_user(&self) -> Result<Self, MapError> {
        let mut target = Self::new_user()?;
        let source_p4 = unsafe { table_ref(self.root)? };
        for p4_index in 0..KERNEL_PML4_START {
            let p4_entry = source_p4.entries[p4_index];
            if p4_entry & ENTRY_PRESENT == 0 {
                continue;
            }
            if p4_entry & ENTRY_HUGE != 0 {
                let _ = target.destroy();
                return Err(MapError::HugePageConflict);
            }
            let p3_address =
                PhysicalAddress::new(p4_entry & ADDRESS_MASK).ok_or(MapError::InvalidTable)?;
            let source_p3 = unsafe { table_ref(p3_address)? };
            for p3_index in 0..ENTRY_COUNT {
                let p3_entry = source_p3.entries[p3_index];
                if p3_entry & ENTRY_PRESENT == 0 {
                    continue;
                }
                if p3_entry & ENTRY_HUGE != 0 {
                    let _ = target.destroy();
                    return Err(MapError::HugePageConflict);
                }
                let p2_address =
                    PhysicalAddress::new(p3_entry & ADDRESS_MASK).ok_or(MapError::InvalidTable)?;
                let source_p2 = unsafe { table_ref(p2_address)? };
                for p2_index in 0..ENTRY_COUNT {
                    let p2_entry = source_p2.entries[p2_index];
                    if p2_entry & ENTRY_PRESENT == 0 {
                        continue;
                    }
                    if p2_entry & ENTRY_HUGE != 0 {
                        let _ = target.destroy();
                        return Err(MapError::HugePageConflict);
                    }
                    let p1_address = PhysicalAddress::new(p2_entry & ADDRESS_MASK)
                        .ok_or(MapError::InvalidTable)?;
                    let source_p1 = unsafe { table_ref(p1_address)? };
                    for p1_index in 0..ENTRY_COUNT {
                        let leaf = source_p1.entries[p1_index];
                        if leaf & (ENTRY_PRESENT | ENTRY_USER) != (ENTRY_PRESENT | ENTRY_USER) {
                            continue;
                        }
                        let source_frame = PhysicalAddress::new(leaf & ADDRESS_MASK)
                            .ok_or(MapError::InvalidTable)?;
                        let target_frame = frame::allocate().map_err(|_| MapError::OutOfMemory)?;
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                source_frame.direct_mapped() as *const u8,
                                target_frame.direct_mapped() as *mut u8,
                                PAGE_SIZE as usize,
                            );
                        }
                        let raw_address = ((p4_index as u64) << 39)
                            | ((p3_index as u64) << 30)
                            | ((p2_index as u64) << 21)
                            | ((p1_index as u64) << 12);
                        let address =
                            VirtualAddress::new(raw_address).ok_or(MapError::InvalidAddress)?;
                        let flags = MapFlags(
                            leaf & (ENTRY_PRESENT
                                | ENTRY_WRITABLE
                                | ENTRY_USER
                                | ENTRY_WRITE_THROUGH
                                | ENTRY_CACHE_DISABLE
                                | ENTRY_NO_EXECUTE),
                        );
                        if let Err(error) = target.map_user(address, target_frame, flags) {
                            let _ = frame::deallocate(target_frame);
                            let _ = target.destroy();
                            return Err(error);
                        }
                    }
                }
            }
        }
        Ok(target)
    }

    pub fn map_user(
        &mut self,
        virtual_address: VirtualAddress,
        physical_address: PhysicalAddress,
        flags: MapFlags,
    ) -> Result<(), MapError> {
        if virtual_address.as_u64() >= USER_ADDRESS_LIMIT {
            return Err(MapError::InvalidAddress);
        }
        if !flags.contains(MapFlags::USER) {
            return Err(MapError::PermissionDenied);
        }
        ActivePageTable { root: self.root }.map_4k(virtual_address, physical_address, flags)
    }

    pub fn unmap_user(
        &mut self,
        virtual_address: VirtualAddress,
    ) -> Result<PhysicalAddress, MapError> {
        if virtual_address.as_u64() >= USER_ADDRESS_LIMIT {
            return Err(MapError::InvalidAddress);
        }
        ActivePageTable { root: self.root }.unmap_4k(virtual_address)
    }

    pub fn allocate_range(
        &mut self,
        virtual_start: VirtualAddress,
        pages: usize,
        flags: MapFlags,
    ) -> Result<(), MapError> {
        ActivePageTable { root: self.root }.allocate_range(virtual_start, pages, flags)
    }

    pub fn release_range(
        &mut self,
        virtual_start: VirtualAddress,
        pages: usize,
        release_frames: bool,
    ) -> Result<(), MapError> {
        ActivePageTable { root: self.root }.release_range(virtual_start, pages, release_frames)
    }

    pub fn set_user_flags(
        &mut self,
        virtual_address: VirtualAddress,
        flags: MapFlags,
    ) -> Result<(), MapError> {
        if virtual_address.as_u64() >= USER_ADDRESS_LIMIT || !flags.contains(MapFlags::USER) {
            return Err(MapError::PermissionDenied);
        }
        ActivePageTable { root: self.root }.set_flags_4k(virtual_address, flags)
    }

    pub fn translate(&self, virtual_address: VirtualAddress) -> Option<PhysicalAddress> {
        ActivePageTable { root: self.root }.translate(virtual_address)
    }

    pub fn copy_from_user(
        &self,
        user_address: u64,
        destination: &mut [u8],
    ) -> Result<(), UserCopyError> {
        self.copy_user(
            user_address,
            destination.as_mut_ptr(),
            destination.len(),
            false,
        )
    }

    pub fn copy_to_user(&self, user_address: u64, source: &[u8]) -> Result<(), UserCopyError> {
        self.copy_user(user_address, source.as_ptr() as *mut u8, source.len(), true)
    }

    pub fn validate_user_range(
        &self,
        user_address: u64,
        length: usize,
        write: bool,
    ) -> Result<(), UserCopyError> {
        let access = if write {
            UserAccess::Write
        } else {
            UserAccess::Read
        };
        let range = checked_user_range(user_address, length, access)
            .map_err(|_| UserCopyError::InvalidRange)?;
        let _guard = PAGE_TABLE_LOCK.lock();
        self.validate_user_range_locked(range)
    }

    fn validate_user_range_locked(&self, range: UserRange) -> Result<(), UserCopyError> {
        let mut current = range.start();
        while current < range.end() {
            ActivePageTable { root: self.root }
                .user_mapping(current, range.access() == UserAccess::Write)?;
            let page_offset = (current & (PAGE_SIZE - 1)) as usize;
            let remaining = (range.end() - current) as usize;
            current += (PAGE_SIZE as usize - page_offset).min(remaining) as u64;
        }
        Ok(())
    }

    pub fn map_guarded_stack(&mut self, top: u64, pages: usize) -> Result<UserStack, MapError> {
        if pages == 0 || pages > 64 || top & (PAGE_SIZE - 1) != 0 {
            return Err(MapError::InvalidAddress);
        }
        let size = (pages as u64)
            .checked_mul(PAGE_SIZE)
            .ok_or(MapError::InvalidAddress)?;
        let bottom = top.checked_sub(size).ok_or(MapError::InvalidAddress)?;
        let guard_page = bottom
            .checked_sub(PAGE_SIZE)
            .ok_or(MapError::InvalidAddress)?;
        if top > USER_ADDRESS_LIMIT || guard_page < PAGE_SIZE {
            return Err(MapError::InvalidAddress);
        }
        let guard = VirtualAddress::new(guard_page).ok_or(MapError::InvalidAddress)?;
        if self.translate(guard).is_some() {
            return Err(MapError::AlreadyMapped);
        }

        let flags = MapFlags::PRESENT | MapFlags::WRITABLE | MapFlags::USER | MapFlags::NO_EXECUTE;
        let mut mapped = 0usize;
        while mapped < pages {
            let address = bottom + mapped as u64 * PAGE_SIZE;
            let frame = match frame::allocate() {
                Ok(frame) => frame,
                Err(_) => {
                    self.release_user_pages(bottom, mapped);
                    return Err(MapError::OutOfMemory);
                }
            };
            unsafe {
                core::ptr::write_bytes(frame.direct_mapped() as *mut u8, 0, PAGE_SIZE as usize);
            }
            let virtual_address = VirtualAddress::new(address).ok_or(MapError::InvalidAddress)?;
            if let Err(error) = self.map_user(virtual_address, frame, flags) {
                let _ = frame::deallocate(frame);
                self.release_user_pages(bottom, mapped);
                return Err(error);
            }
            mapped += 1;
        }
        Ok(UserStack {
            guard_page,
            bottom,
            top,
        })
    }

    pub fn release_user_stack(&mut self, stack: UserStack) {
        let pages = ((stack.top - stack.bottom) / PAGE_SIZE) as usize;
        self.release_user_pages(stack.bottom, pages);
    }

    pub fn destroy_empty(self) -> Result<(), MapError> {
        if ActivePageTable::current().is_ok_and(|active| active.root == self.root) {
            return Err(MapError::AddressSpaceBusy);
        }
        let _guard = PAGE_TABLE_LOCK.lock();
        let root = unsafe { table_ref(self.root)? };
        for entry in &root.entries[..KERNEL_PML4_START] {
            if *entry & ENTRY_PRESENT != 0 {
                let p3 =
                    PhysicalAddress::new(*entry & ADDRESS_MASK).ok_or(MapError::InvalidTable)?;
                unsafe { release_empty_subtree(p3, 3)? };
            }
        }
        frame::deallocate(self.root).map_err(|_| MapError::InvalidTable)
    }

    /// Releases all allocator-owned user frames and the complete lower-half
    /// page-table tree. Device/shared frames must be unmapped by their owner
    /// before calling this method.
    pub fn destroy(self) -> Result<(), MapError> {
        if ActivePageTable::current().is_ok_and(|active| active.root == self.root) {
            return Err(MapError::AddressSpaceBusy);
        }
        let _guard = PAGE_TABLE_LOCK.lock();
        let root = unsafe { table_ref(self.root)? };
        for entry in &root.entries[..KERNEL_PML4_START] {
            if *entry & ENTRY_PRESENT == 0 {
                continue;
            }
            let p3 = PhysicalAddress::new(*entry & ADDRESS_MASK).ok_or(MapError::InvalidTable)?;
            unsafe { release_owned_subtree(p3, 3)? };
        }
        frame::deallocate(self.root).map_err(|_| MapError::InvalidTable)
    }

    fn copy_user(
        &self,
        user_address: u64,
        buffer: *mut u8,
        length: usize,
        write: bool,
    ) -> Result<(), UserCopyError> {
        let access = if write {
            UserAccess::Write
        } else {
            UserAccess::Read
        };
        let range = checked_user_range(user_address, length, access)
            .map_err(|_| UserCopyError::InvalidRange)?;
        let _guard = PAGE_TABLE_LOCK.lock();
        self.validate_user_range_locked(range)?;

        let active = ActivePageTable { root: self.root };
        let mut copied = 0usize;
        while copied < range.length() {
            let current = range.start() + copied as u64;
            let physical = active.user_mapping(current, write)?;
            let page_offset = (current & (PAGE_SIZE - 1)) as usize;
            let chunk = (PAGE_SIZE as usize - page_offset).min(range.length() - copied);
            let physical_pointer = (physical.direct_mapped() as usize + page_offset) as *mut u8;
            unsafe {
                if write {
                    core::ptr::copy(buffer.add(copied), physical_pointer, chunk);
                } else {
                    core::ptr::copy(physical_pointer, buffer.add(copied), chunk);
                }
            }
            copied += chunk;
        }
        Ok(())
    }

    fn release_user_pages(&mut self, bottom: u64, pages: usize) {
        for index in 0..pages {
            let address = bottom + index as u64 * PAGE_SIZE;
            if let Some(virtual_address) = VirtualAddress::new(address) {
                if let Ok(frame) = self.unmap_user(virtual_address) {
                    let _ = frame::deallocate(frame);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct PagingSecurity {
    pub write_xor_execute: bool,
    pub high_half: bool,
    pub user_isolation: bool,
    pub user_copy: bool,
    pub user_guard: bool,
    pub stack_guard: bool,
}

pub fn initialize_kernel_permissions() -> PagingSecurity {
    enable_nx_and_write_protect();
    let Ok(mut mapper) = ActivePageTable::current() else {
        return PagingSecurity::failed();
    };
    let ranges = kernel_ranges();
    let split_start = align_down(ranges.kernel_start, 1 << 21);
    let split_end = align_up(ranges.kernel_end, 1 << 21);
    for address in (split_start..split_end).step_by(1 << 21) {
        let Some(virtual_address) = VirtualAddress::new(address) else {
            return PagingSecurity::failed();
        };
        if mapper.split_2m(virtual_address).is_err() {
            return PagingSecurity::failed();
        }
    }

    let readonly = MapFlags::PRESENT | MapFlags::GLOBAL | MapFlags::NO_EXECUTE;
    let executable = MapFlags::PRESENT | MapFlags::GLOBAL;
    let writable = readonly | MapFlags::WRITABLE;
    if !protect_range(&mut mapper, split_start, ranges.text_start, readonly) {
        return PagingSecurity::failed();
    }
    if !protect_range(&mut mapper, ranges.text_start, ranges.text_end, executable) {
        return PagingSecurity::failed();
    }
    if !protect_range(&mut mapper, ranges.text_end, ranges.data_start, readonly) {
        return PagingSecurity::failed();
    }
    if !protect_range(&mut mapper, ranges.data_start, ranges.kernel_end, writable) {
        return PagingSecurity::failed();
    }
    if !protect_range(
        &mut mapper,
        align_up(ranges.kernel_end, PAGE_SIZE),
        split_end,
        readonly,
    ) {
        return PagingSecurity::failed();
    }

    let aliases_protected = protect_physical_aliases(&mut mapper, &ranges);
    let unused_pages_nx = mapper
        .make_unused_high_kernel_pages_nx(ranges.kernel_phys_end)
        .is_ok();
    let global_nx = mapper.make_identity_and_direct_map_nx().is_ok();
    let stack_guard = configure_stack_guard(&mut mapper, writable);
    let mapping_ok = mapping_self_test(&mut mapper);
    let (user_isolation, user_copy, user_guard) = address_space_self_test(&mapper, &ranges);
    let high_half = ranges.kernel_start >= KERNEL_BASE;

    PagingSecurity {
        write_xor_execute: aliases_protected && unused_pages_nx && global_nx && mapping_ok,
        high_half,
        user_isolation,
        user_copy,
        user_guard,
        stack_guard,
    }
}

impl PagingSecurity {
    const fn failed() -> Self {
        Self {
            write_xor_execute: false,
            high_half: false,
            user_isolation: false,
            user_copy: false,
            user_guard: false,
            stack_guard: false,
        }
    }
}

fn mapping_self_test(mapper: &mut ActivePageTable) -> bool {
    let free_before = frame::stats().free_frames;
    let Some(virtual_address) = VirtualAddress::new(0x4000_0000_0000) else {
        return false;
    };
    let Ok(frame) = frame::allocate() else {
        return false;
    };
    let flags = MapFlags::PRESENT | MapFlags::WRITABLE | MapFlags::NO_EXECUTE;
    if mapper.map_range(virtual_address, frame, 1, flags).is_err() {
        let _ = frame::deallocate(frame);
        return false;
    }
    let translated = mapper.translate(virtual_address) == Some(frame);
    let unmapped = mapper.unmap_4k(virtual_address) == Ok(frame);
    let released = frame::deallocate(frame).is_ok();
    let Some(range_start) = VirtualAddress::new(0x4000_0020_0000) else {
        return false;
    };
    let range_mapped = mapper.allocate_range(range_start, 2, flags).is_ok();
    let range_translated = range_mapped
        && mapper.translate(range_start).is_some()
        && VirtualAddress::new(range_start.as_u64() + PAGE_SIZE)
            .is_some_and(|address| mapper.translate(address).is_some());
    let range_released = range_mapped && mapper.release_range(range_start, 2, true).is_ok();
    translated
        && unmapped
        && released
        && range_translated
        && range_released
        && frame::stats().free_frames == free_before
}

fn address_space_self_test(mapper: &ActivePageTable, ranges: &KernelRanges) -> (bool, bool, bool) {
    let Some(user_address) = VirtualAddress::new(0x0000_0000_4000_0000) else {
        return (false, false, false);
    };
    let Some(kernel_address) = VirtualAddress::new(ranges.text_start) else {
        return (false, false, false);
    };
    let Ok(mut space) = AddressSpace::new_user() else {
        return (false, false, false);
    };
    let Ok(user_frame) = frame::allocate() else {
        let _ = space.destroy_empty();
        return (false, false, false);
    };
    let flags = MapFlags::PRESENT | MapFlags::WRITABLE | MapFlags::USER | MapFlags::NO_EXECUTE;
    if space.map_user(user_address, user_frame, flags).is_err() {
        let _ = frame::deallocate(user_frame);
        let _ = space.destroy_empty();
        return (false, false, false);
    }

    let pattern = [0x52, 0x75, 0x73, 0x74, 0x69, 0x78];
    let mut copied = [0u8; 6];
    let safe_copy = space
        .copy_to_user(user_address.as_u64() + 17, &pattern)
        .is_ok()
        && space
            .copy_from_user(user_address.as_u64() + 17, &mut copied)
            .is_ok()
        && copied == pattern
        && space.copy_from_user(PAGE_SIZE - 1, &mut copied).is_err();
    let guarded_stack = space.map_guarded_stack(0x0000_0000_5000_0000, 2).ok();
    let user_guard = guarded_stack.is_some_and(|stack| {
        let guard = VirtualAddress::new(stack.guard_page).expect("aligned guard address");
        let cross_page = stack.bottom + PAGE_SIZE - 3;
        let mut round_trip = [0u8; 6];
        space.translate(guard).is_none()
            && space.copy_to_user(cross_page, &pattern).is_ok()
            && space.copy_from_user(cross_page, &mut round_trip).is_ok()
            && round_trip == pattern
    });
    let isolated = space.translate(user_address) == Some(user_frame)
        && space.translate(kernel_address) == mapper.translate(kernel_address)
        && unsafe { table_ref(space.root()) }.is_ok_and(|root| {
            root.entries[0] & ENTRY_USER != 0 && root.entries[511] & ENTRY_USER == 0
        })
        && VirtualAddress::new(0).is_some_and(|zero| space.translate(zero).is_none());
    // Exercise the hardware CR3 boundary while executing only shared supervisor mappings.
    reload_cr3(space.root());
    let activated = ActivePageTable::current().is_ok_and(|active| active.root == space.root());
    reload_cr3(mapper.root);
    if let Some(stack) = guarded_stack {
        space.release_user_stack(stack);
    }
    let destroyed = space.destroy().is_ok();
    (isolated && activated && destroyed, safe_copy, user_guard)
}

pub fn map_mmio_uncached(physical_start: u64, length: u64) -> bool {
    if length == 0 || physical_start >= 0x1_0000_0000 {
        return false;
    }
    let Some(end) = physical_start.checked_add(length) else {
        return false;
    };
    if end > 0x1_0000_0000 {
        return false;
    }
    let Ok(mut mapper) = ActivePageTable::current() else {
        return false;
    };
    let flags = MapFlags::PRESENT
        | MapFlags::WRITABLE
        | MapFlags::NO_EXECUTE
        | MapFlags::WRITE_THROUGH
        | MapFlags::CACHE_DISABLE;
    split_and_protect(&mut mapper, physical_start, end, flags)
        && split_and_protect(
            &mut mapper,
            PHYSICAL_MEMORY_OFFSET + physical_start,
            PHYSICAL_MEMORY_OFFSET + end,
            flags,
        )
}

pub fn map_guarded_kernel_stack(bottom: u64, pages: usize) -> Option<u64> {
    if pages == 0 || pages > 64 || bottom & (PAGE_SIZE - 1) != 0 {
        return None;
    }
    let guard = VirtualAddress::new(bottom.checked_sub(PAGE_SIZE)?)?;
    let mut mapper = ActivePageTable::current().ok()?;
    if mapper.translate(guard).is_some() {
        return None;
    }
    let flags = MapFlags::PRESENT | MapFlags::WRITABLE | MapFlags::NO_EXECUTE;
    let mut mapped = 0usize;
    while mapped < pages {
        let address = VirtualAddress::new(bottom + mapped as u64 * PAGE_SIZE)?;
        let physical = match frame::allocate() {
            Ok(frame) => frame,
            Err(_) => {
                for rollback in 0..mapped {
                    if let Some(address) = VirtualAddress::new(bottom + rollback as u64 * PAGE_SIZE)
                    {
                        if let Ok(frame) = mapper.unmap_4k(address) {
                            let _ = frame::deallocate(frame);
                        }
                    }
                }
                return None;
            }
        };
        if mapper.map_4k(address, physical, flags).is_err() {
            let _ = frame::deallocate(physical);
            for rollback in 0..mapped {
                if let Some(address) = VirtualAddress::new(bottom + rollback as u64 * PAGE_SIZE) {
                    if let Ok(frame) = mapper.unmap_4k(address) {
                        let _ = frame::deallocate(frame);
                    }
                }
            }
            return None;
        }
        mapped += 1;
    }
    bottom.checked_add(pages as u64 * PAGE_SIZE)
}

pub fn unmap_guarded_kernel_stack(top: u64, pages: usize) -> bool {
    let Some(size) = (pages as u64).checked_mul(PAGE_SIZE) else {
        return false;
    };
    let Some(bottom) = top.checked_sub(size) else {
        return false;
    };
    let Ok(mut mapper) = ActivePageTable::current() else {
        return false;
    };
    for index in 0..pages {
        let Some(address) = VirtualAddress::new(bottom + index as u64 * PAGE_SIZE) else {
            return false;
        };
        let Ok(frame) = mapper.unmap_4k(address) else {
            return false;
        };
        if frame::deallocate(frame).is_err() {
            return false;
        }
    }
    true
}

pub fn set_low_identity_executable(executable: bool) -> bool {
    let Ok(mapper) = ActivePageTable::current() else {
        return false;
    };
    let Ok(root) = (unsafe { table_mut(mapper.root) }) else {
        return false;
    };
    if root.entries[0] & ENTRY_PRESENT == 0 {
        return false;
    }
    if executable {
        root.entries[0] &= !ENTRY_NO_EXECUTE;
    } else {
        root.entries[0] |= ENTRY_NO_EXECUTE;
    }
    reload_cr3(mapper.root);
    true
}

fn configure_stack_guard(mapper: &mut ActivePageTable, stack_flags: MapFlags) -> bool {
    let Some(guard) = VirtualAddress::new(EARLY_STACK_GUARD) else {
        return false;
    };
    if mapper.split_2m(guard).is_err() {
        return false;
    }
    if !protect_range(mapper, EARLY_STACK_START, EARLY_STACK_END, stack_flags) {
        return false;
    }
    mapper.unmap_4k(guard).is_ok()
}

fn protect_physical_aliases(mapper: &mut ActivePageTable, ranges: &KernelRanges) -> bool {
    let readonly = MapFlags::PRESENT | MapFlags::GLOBAL | MapFlags::NO_EXECUTE;
    let writable = readonly | MapFlags::WRITABLE;
    let regions = [
        (ranges.kernel_phys_start, ranges.text_phys_start, readonly),
        (ranges.text_phys_start, ranges.text_phys_end, readonly),
        (ranges.text_phys_end, ranges.data_phys_start, readonly),
        (ranges.data_phys_start, ranges.kernel_phys_end, writable),
    ];

    for &(start, end, flags) in &regions {
        if !split_and_protect(mapper, start, end, flags)
            || !split_and_protect(
                mapper,
                PHYSICAL_MEMORY_OFFSET + start,
                PHYSICAL_MEMORY_OFFSET + end,
                flags,
            )
        {
            return false;
        }
    }
    true
}

fn split_and_protect(mapper: &mut ActivePageTable, start: u64, end: u64, flags: MapFlags) -> bool {
    for address in (align_down(start, 1 << 21)..align_up(end, 1 << 21)).step_by(1 << 21) {
        let Some(virtual_address) = VirtualAddress::new(address) else {
            return false;
        };
        if mapper.split_2m(virtual_address).is_err() {
            return false;
        }
    }
    protect_range(mapper, start, end, flags)
}

fn protect_range(mapper: &mut ActivePageTable, start: u64, end: u64, flags: MapFlags) -> bool {
    let start = align_down(start, PAGE_SIZE);
    let end = align_up(end, PAGE_SIZE);
    for address in (start..end).step_by(PAGE_SIZE as usize) {
        let Some(virtual_address) = VirtualAddress::new(address) else {
            return false;
        };
        if mapper.set_flags_4k(virtual_address, flags).is_err() {
            return false;
        }
    }
    true
}

struct KernelRanges {
    kernel_start: u64,
    text_start: u64,
    text_end: u64,
    data_start: u64,
    kernel_end: u64,
    kernel_phys_start: u64,
    text_phys_start: u64,
    text_phys_end: u64,
    data_phys_start: u64,
    kernel_phys_end: u64,
}

fn kernel_ranges() -> KernelRanges {
    unsafe extern "C" {
        static kernel_start: u8;
        static text_start: u8;
        static text_end: u8;
        static data_start: u8;
        static kernel_end: u8;
        static kernel_phys_start: u8;
        static kernel_phys_end: u8;
    }
    KernelRanges {
        kernel_start: core::ptr::addr_of!(kernel_start) as u64,
        text_start: core::ptr::addr_of!(text_start) as u64,
        text_end: core::ptr::addr_of!(text_end) as u64,
        data_start: core::ptr::addr_of!(data_start) as u64,
        kernel_end: core::ptr::addr_of!(kernel_end) as u64,
        kernel_phys_start: core::ptr::addr_of!(kernel_phys_start) as u64,
        text_phys_start: core::ptr::addr_of!(text_start) as u64 - KERNEL_BASE,
        text_phys_end: core::ptr::addr_of!(text_end) as u64 - KERNEL_BASE,
        data_phys_start: core::ptr::addr_of!(data_start) as u64 - KERNEL_BASE,
        kernel_phys_end: core::ptr::addr_of!(kernel_phys_end) as u64,
    }
}

fn align_down(value: u64, alignment: u64) -> u64 {
    value & !(alignment - 1)
}

fn align_up(value: u64, alignment: u64) -> u64 {
    value.saturating_add(alignment - 1) & !(alignment - 1)
}

unsafe fn table_ref(address: PhysicalAddress) -> Result<&'static PageTable, MapError> {
    if address.as_u64() == 0 || address.as_u64() >= MAX_DIRECT_MAPPED_PHYSICAL {
        return Err(MapError::InvalidTable);
    }
    Ok(&*(address.direct_mapped() as *const PageTable))
}

unsafe fn table_mut(address: PhysicalAddress) -> Result<&'static mut PageTable, MapError> {
    if address.as_u64() == 0 || address.as_u64() >= MAX_DIRECT_MAPPED_PHYSICAL {
        return Err(MapError::InvalidTable);
    }
    Ok(&mut *(address.direct_mapped() as *mut PageTable))
}

unsafe fn zero_frame(address: PhysicalAddress) {
    let words = PAGE_SIZE / core::mem::size_of::<u64>() as u64;
    let pointer = address.direct_mapped() as *mut u64;
    for index in 0..words {
        core::ptr::write_volatile(pointer.add(index as usize), 0);
    }
}

unsafe fn release_empty_subtree(table_address: PhysicalAddress, level: u8) -> Result<(), MapError> {
    let table = table_ref(table_address)?;
    for entry in &table.entries {
        if *entry & ENTRY_PRESENT == 0 {
            continue;
        }
        if level == 1 || *entry & ENTRY_HUGE != 0 {
            return Err(MapError::AddressSpaceBusy);
        }
        let child = PhysicalAddress::new(*entry & ADDRESS_MASK).ok_or(MapError::InvalidTable)?;
        release_empty_subtree(child, level - 1)?;
    }
    frame::deallocate(table_address).map_err(|_| MapError::InvalidTable)
}

unsafe fn release_owned_subtree(table_address: PhysicalAddress, level: u8) -> Result<(), MapError> {
    let table = table_ref(table_address)?;
    for entry in &table.entries {
        if *entry & ENTRY_PRESENT == 0 {
            continue;
        }
        if *entry & ENTRY_HUGE != 0 {
            return Err(MapError::HugePageConflict);
        }
        let child = PhysicalAddress::new(*entry & ADDRESS_MASK).ok_or(MapError::InvalidTable)?;
        if level == 1 {
            frame::deallocate(child).map_err(|_| MapError::InvalidTable)?;
        } else {
            release_owned_subtree(child, level - 1)?;
        }
    }
    frame::deallocate(table_address).map_err(|_| MapError::InvalidTable)
}

fn invalidate(address: VirtualAddress) {
    unsafe {
        asm!("invlpg [{}]", in(reg) address.as_u64(), options(nostack, preserves_flags));
    }
}

fn reload_cr3(root: PhysicalAddress) {
    unsafe {
        asm!("mov cr3, {}", in(reg) root.as_u64(), options(nostack, preserves_flags));
    }
}

fn enable_nx_and_write_protect() {
    unsafe {
        let mut cr0: u64;
        asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
        cr0 |= 1 << 16;
        asm!("mov cr0, {}", in(reg) cr0, options(nostack, preserves_flags));

        let mut low: u32;
        let mut high: u32;
        asm!(
            "rdmsr",
            in("ecx") 0xC000_0080u32,
            out("eax") low,
            out("edx") high,
            options(nostack)
        );
        low |= 1 << 11;
        asm!(
            "wrmsr",
            in("ecx") 0xC000_0080u32,
            in("eax") low,
            in("edx") high,
            options(nostack)
        );
    }
}
