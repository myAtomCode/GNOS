use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::mm::address::{VirtualAddress, PAGE_SIZE};
use crate::mm::frame;
use crate::mm::paging::{ActivePageTable, MapFlags};
use crate::sync::SpinMutex;

const HEAP_START: u64 = 0xffff_9000_0000_0000;
const HEAP_SIZE: usize = 4 * 1024 * 1024;
const ALLOCATION_MAGIC: usize = 0x5255_5354_4845_4150;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeapError {
    AlreadyInitialized,
    MapFailed,
    OutOfMemory,
}

#[derive(Clone, Copy)]
pub struct HeapStats {
    pub size: usize,
    pub used: usize,
    pub peak: usize,
    pub allocations: usize,
    pub oom_events: usize,
    pub self_test: bool,
}

#[repr(C)]
struct FreeRegion {
    size: usize,
    next: *mut FreeRegion,
}

#[repr(C)]
struct AllocationHeader {
    block_start: usize,
    block_size: usize,
    magic: usize,
}

struct KernelHeap {
    head: FreeRegion,
    size: usize,
    used: usize,
    peak: usize,
    allocations: usize,
    initialized: bool,
}

unsafe impl Send for KernelHeap {}

impl KernelHeap {
    const fn new() -> Self {
        Self {
            head: FreeRegion {
                size: 0,
                next: ptr::null_mut(),
            },
            size: 0,
            used: 0,
            peak: 0,
            allocations: 0,
            initialized: false,
        }
    }

    unsafe fn initialize(&mut self, start: usize, size: usize) -> Result<(), HeapError> {
        if self.initialized {
            return Err(HeapError::AlreadyInitialized);
        }
        self.size = size;
        self.initialized = true;
        self.insert_region(start, size);
        Ok(())
    }

    unsafe fn allocate(&mut self, layout: Layout) -> *mut u8 {
        if !self.initialized || layout.size() == 0 {
            return ptr::null_mut();
        }

        let requested = layout.size().max(1);
        let alignment = layout.align().max(core::mem::align_of::<FreeRegion>());
        let header_size = core::mem::size_of::<AllocationHeader>();
        let minimum_region = core::mem::size_of::<FreeRegion>();
        let mut previous = &mut self.head as *mut FreeRegion;

        while !(*previous).next.is_null() {
            let region = (*previous).next;
            let region_start = region as usize;
            let Some(region_end) = region_start.checked_add((*region).size) else {
                return ptr::null_mut();
            };
            let Some(base) = region_start.checked_add(header_size) else {
                return ptr::null_mut();
            };
            let Some(mut user_start) = align_up(base, alignment) else {
                return ptr::null_mut();
            };
            let mut block_start = user_start - header_size;
            let mut prefix = block_start - region_start;
            if prefix != 0 && prefix < minimum_region {
                let Some(next_user) = user_start.checked_add(alignment) else {
                    return ptr::null_mut();
                };
                user_start = next_user;
                block_start = user_start - header_size;
                prefix = block_start - region_start;
            }
            let Some(raw_end) = user_start.checked_add(requested) else {
                return ptr::null_mut();
            };
            let Some(requested_end) = align_up(raw_end, core::mem::align_of::<FreeRegion>()) else {
                return ptr::null_mut();
            };
            if requested_end > region_end {
                previous = region;
                continue;
            }

            let mut block_end = requested_end;
            let suffix = region_end - requested_end;
            if suffix != 0 && suffix < minimum_region {
                block_end = region_end;
            }
            let block_size = block_end - block_start;
            (*previous).next = (*region).next;

            if prefix >= minimum_region {
                self.insert_region(region_start, prefix);
            }
            if block_end < region_end {
                self.insert_region(block_end, region_end - block_end);
            }

            let header = (user_start - header_size) as *mut AllocationHeader;
            ptr::write(
                header,
                AllocationHeader {
                    block_start,
                    block_size,
                    magic: ALLOCATION_MAGIC ^ user_start,
                },
            );
            self.used += block_size;
            self.peak = self.peak.max(self.used);
            self.allocations += 1;
            return user_start as *mut u8;
        }
        ptr::null_mut()
    }

    unsafe fn deallocate(&mut self, pointer: *mut u8) {
        if pointer.is_null() || !self.initialized {
            return;
        }
        let header_size = core::mem::size_of::<AllocationHeader>();
        let address = pointer as usize;
        let heap_end = HEAP_START as usize + HEAP_SIZE;
        let Some(header_address) = address.checked_sub(header_size) else {
            return;
        };
        if header_address < HEAP_START as usize || address >= heap_end {
            return;
        }
        let header = header_address as *mut AllocationHeader;
        let metadata = ptr::read(header);
        if metadata.magic != (ALLOCATION_MAGIC ^ address)
            || metadata.block_start < HEAP_START as usize
            || metadata.block_start.saturating_add(metadata.block_size)
                > HEAP_START as usize + HEAP_SIZE
        {
            return;
        }
        // The C primitive uses volatile stores so released kernel data cannot
        // be retained by dead-store elimination.
        crate::c_fastpath::zero_explicit(metadata.block_start as *mut u8, metadata.block_size);
        self.used = self.used.saturating_sub(metadata.block_size);
        self.allocations = self.allocations.saturating_sub(1);
        self.insert_region(metadata.block_start, metadata.block_size);
    }

    unsafe fn insert_region(&mut self, address: usize, size: usize) {
        let minimum = core::mem::size_of::<FreeRegion>();
        if size < minimum || address % core::mem::align_of::<FreeRegion>() != 0 {
            return;
        }
        let Some(end) = address.checked_add(size) else {
            return;
        };
        if address < HEAP_START as usize || end > HEAP_START as usize + HEAP_SIZE {
            return;
        }

        let mut previous = &mut self.head as *mut FreeRegion;
        while !(*previous).next.is_null() && ((*previous).next as usize) < address {
            previous = (*previous).next;
        }
        let next = (*previous).next;
        if previous != &mut self.head as *mut FreeRegion
            && (previous as usize).saturating_add((*previous).size) > address
        {
            return;
        }
        if !next.is_null() && end > next as usize {
            return;
        }
        let node = address as *mut FreeRegion;
        ptr::write(node, FreeRegion { size, next });
        (*previous).next = node;

        if !next.is_null() && address + (*node).size == next as usize {
            (*node).size += (*next).size;
            (*node).next = (*next).next;
        }
        if previous != &mut self.head as *mut FreeRegion
            && previous as usize + (*previous).size == node as usize
        {
            (*previous).size += (*node).size;
            (*previous).next = (*node).next;
        }
    }
}

pub struct GlobalKernelAllocator(SpinMutex<KernelHeap>);

impl GlobalKernelAllocator {
    const fn new() -> Self {
        Self(SpinMutex::new(KernelHeap::new()))
    }
}

unsafe impl GlobalAlloc for GlobalKernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = self.0.lock().allocate(layout);
        if pointer.is_null() {
            OOM_EVENTS.fetch_add(1, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, _layout: Layout) {
        self.0.lock().deallocate(pointer);
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: GlobalKernelAllocator = GlobalKernelAllocator::new();
static OOM_EVENTS: AtomicUsize = AtomicUsize::new(0);
static OOM_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

pub fn initialize() -> Result<HeapStats, HeapError> {
    let mut mapper = ActivePageTable::current().map_err(|_| HeapError::MapFailed)?;
    let flags = MapFlags::PRESENT | MapFlags::WRITABLE | MapFlags::GLOBAL | MapFlags::NO_EXECUTE;
    let mut mapped = 0usize;

    for offset in (0..HEAP_SIZE).step_by(PAGE_SIZE as usize) {
        let virtual_address =
            VirtualAddress::new(HEAP_START + offset as u64).ok_or(HeapError::MapFailed)?;
        let frame = match frame::allocate() {
            Ok(frame) => frame,
            Err(_) => {
                rollback(&mut mapper, mapped);
                return Err(HeapError::OutOfMemory);
            }
        };
        if mapper.map_4k(virtual_address, frame, flags).is_err() {
            let _ = frame::deallocate(frame);
            rollback(&mut mapper, mapped);
            return Err(HeapError::MapFailed);
        }
        mapped += 1;
    }

    unsafe {
        GLOBAL_ALLOCATOR
            .0
            .lock()
            .initialize(HEAP_START as usize, HEAP_SIZE)?;
    }
    let self_test = self_test();
    Ok(stats_with_test(self_test))
}

pub fn try_allocate(layout: Layout) -> Result<NonNull<u8>, HeapError> {
    let pointer = unsafe { GLOBAL_ALLOCATOR.alloc(layout) };
    NonNull::new(pointer).ok_or(HeapError::OutOfMemory)
}

pub unsafe fn deallocate(pointer: NonNull<u8>, layout: Layout) {
    GLOBAL_ALLOCATOR.dealloc(pointer.as_ptr(), layout);
}

pub fn stats() -> HeapStats {
    stats_with_test(false)
}

pub fn handle_oom(layout: Layout) -> ! {
    if !OOM_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        let stats = stats();
        crate::println!(
            "\n[oom] kernel heap exhausted: request={} align={} used={}/{} peak={} events={}",
            layout.size(),
            layout.align(),
            stats.used,
            stats.size,
            stats.peak,
            stats.oom_events,
        );
    }
    crate::arch::halt()
}

fn self_test() -> bool {
    let before = stats();
    let first_layout = match Layout::from_size_align(73, 64) {
        Ok(layout) => layout,
        Err(_) => return false,
    };
    let second_layout = match Layout::from_size_align(8192, 4096) {
        Ok(layout) => layout,
        Err(_) => return false,
    };
    let Ok(first) = try_allocate(first_layout) else {
        return false;
    };
    let Ok(second) = try_allocate(second_layout) else {
        unsafe { deallocate(first, first_layout) };
        return false;
    };
    unsafe {
        ptr::write_bytes(first.as_ptr(), 0xa5, first_layout.size());
        ptr::write_bytes(second.as_ptr(), 0x5a, second_layout.size());
        deallocate(second, second_layout);
        deallocate(first, first_layout);
    }
    let after = stats();
    let impossible = match Layout::from_size_align(HEAP_SIZE * 2, 8) {
        Ok(layout) => layout,
        Err(_) => return false,
    };
    let recoverable_oom = try_allocate(impossible) == Err(HeapError::OutOfMemory);
    let final_stats = stats();
    recoverable_oom
        && after.used == before.used
        && after.allocations == before.allocations
        && final_stats.used == before.used
        && final_stats.allocations == before.allocations
}

fn stats_with_test(self_test: bool) -> HeapStats {
    let heap = GLOBAL_ALLOCATOR.0.lock();
    HeapStats {
        size: heap.size,
        used: heap.used,
        peak: heap.peak,
        allocations: heap.allocations,
        oom_events: OOM_EVENTS.load(Ordering::Relaxed),
        self_test,
    }
}

fn rollback(mapper: &mut ActivePageTable, mapped: usize) {
    for page in 0..mapped {
        if let Some(address) = VirtualAddress::new(HEAP_START + page as u64 * PAGE_SIZE) {
            if let Ok(frame) = mapper.unmap_4k(address) {
                let _ = frame::deallocate(frame);
            }
        }
    }
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    value
        .checked_add(alignment - 1)
        .map(|sum| sum & !(alignment - 1))
}
