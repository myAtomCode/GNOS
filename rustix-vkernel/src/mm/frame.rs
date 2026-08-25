use crate::boot::{MemoryMap, MemoryRegionKind};
use crate::mm::address::{PhysicalAddress, PAGE_SIZE};
use crate::sync::IrqSpinMutex;
use core::sync::atomic::{AtomicU8, Ordering};

#[cfg(target_arch = "x86_64")]
const MAX_PHYSICAL_MEMORY: u64 = 4 * 1024 * 1024 * 1024;
#[cfg(target_arch = "aarch64")]
const MAX_PHYSICAL_MEMORY: u64 = 16 * 1024 * 1024 * 1024;
const FRAME_COUNT: usize = (MAX_PHYSICAL_MEMORY / PAGE_SIZE) as usize;
const BITMAP_WORDS: usize = FRAME_COUNT / 64;
pub const MAX_NUMA_NODES: usize = 8;
const MAX_NUMA_CPUS: usize = 64;
const LOW_MEMORY_END: u64 = 0x10_0000;
#[cfg(target_arch = "x86_64")]
const EARLY_STACK_START: u64 = 0xff_0000;
#[cfg(target_arch = "x86_64")]
const EARLY_STACK_END: u64 = 0x100_1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    NotInitialized,
    OutOfMemory,
    InvalidFrame,
    DoubleFree,
    InvalidTopology,
}

#[derive(Clone, Copy)]
pub struct FrameStats {
    pub free_frames: usize,
    pub numa_nodes: usize,
    pub local_allocations: u64,
    pub fallback_allocations: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NumaRange {
    pub node: usize,
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Copy)]
struct NumaNode {
    online: bool,
    start_frame: usize,
    end_frame: usize,
    next_word: usize,
    free_frames: usize,
}

impl NumaNode {
    const fn empty() -> Self {
        Self {
            online: false,
            start_frame: 0,
            end_frame: 0,
            next_word: 0,
            free_frames: 0,
        }
    }
}

pub struct BitmapFrameAllocator {
    // A set bit means unavailable; zero means allocatable.
    bitmap: [u64; BITMAP_WORDS],
    allocated: [u64; BITMAP_WORDS],
    free_frames: usize,
    initialized: bool,
    nodes: [NumaNode; MAX_NUMA_NODES],
    local_allocations: u64,
    fallback_allocations: u64,
}

impl BitmapFrameAllocator {
    pub const fn new() -> Self {
        Self {
            bitmap: [0; BITMAP_WORDS],
            allocated: [0; BITMAP_WORDS],
            free_frames: 0,
            initialized: false,
            nodes: [NumaNode::empty(); MAX_NUMA_NODES],
            local_allocations: 0,
            fallback_allocations: 0,
        }
    }

    pub fn initialize(&mut self, map: &MemoryMap) {
        crate::c_fastpath::bitmap_fill(&mut self.bitmap, u64::MAX);
        crate::c_fastpath::bitmap_fill(&mut self.allocated, 0);
        self.free_frames = 0;
        self.nodes = [NumaNode::empty(); MAX_NUMA_NODES];
        self.local_allocations = 0;
        self.fallback_allocations = 0;

        for region in map.regions() {
            if matches!(
                region.kind,
                MemoryRegionKind::Usable | MemoryRegionKind::Bootloader
            ) {
                self.mark_range(region.start, region.end, false);
            }
        }
        // Reserved firmware entries win if a broken map contains overlaps.
        for region in map.regions() {
            if !matches!(
                region.kind,
                MemoryRegionKind::Usable | MemoryRegionKind::Bootloader
            ) {
                self.mark_range(region.start, region.end, true);
            }
        }

        self.mark_range(0, LOW_MEMORY_END, true);
        self.mark_range(kernel_start(), kernel_end(), true);
        #[cfg(target_arch = "x86_64")]
        self.mark_range(EARLY_STACK_START, EARLY_STACK_END, true);
        if let Some((start, end)) = map.boot_data_range() {
            self.mark_range(start, end, true);
        }
        self.free_frames = crate::c_fastpath::bitmap_count_clear(&self.bitmap);
        self.nodes[0] = NumaNode {
            online: true,
            start_frame: 0,
            end_frame: FRAME_COUNT,
            next_word: 0,
            free_frames: self.free_frames,
        };
        self.initialized = true;
    }

    pub fn allocate(&mut self) -> Result<PhysicalAddress, FrameError> {
        self.allocate_on_node(0)
    }

    pub fn allocate_on_node(&mut self, preferred: usize) -> Result<PhysicalAddress, FrameError> {
        if !self.initialized {
            return Err(FrameError::NotInitialized);
        }
        if self.free_frames == 0 {
            return Err(FrameError::OutOfMemory);
        }

        let preferred = preferred % MAX_NUMA_NODES;
        if self.nodes[preferred].online {
            if let Some(address) = self.allocate_from_node(preferred) {
                self.local_allocations = self.local_allocations.saturating_add(1);
                return Ok(address);
            }
        }
        for distance in 1..MAX_NUMA_NODES {
            let node = (preferred + distance) % MAX_NUMA_NODES;
            if self.nodes[node].online {
                if let Some(address) = self.allocate_from_node(node) {
                    self.fallback_allocations = self.fallback_allocations.saturating_add(1);
                    return Ok(address);
                }
            }
        }
        Err(FrameError::OutOfMemory)
    }

    fn allocate_from_node(&mut self, node: usize) -> Option<PhysicalAddress> {
        let topology = self.nodes[node];
        if !topology.online || topology.free_frames == 0 {
            return None;
        }
        let first_word = topology.start_frame / 64;
        let last_word = topology.end_frame.div_ceil(64);
        let word_count = last_word.saturating_sub(first_word);
        if word_count == 0 {
            return None;
        }
        let cursor = topology.next_word.clamp(first_word, last_word - 1);
        for step in 0..word_count {
            let word = first_word + (cursor - first_word + step) % word_count;
            let allowed = frame_word_mask(word, topology.start_frame, topology.end_frame);
            let available = !self.bitmap[word] & allowed;
            if available == 0 {
                continue;
            }
            let bit = available.trailing_zeros() as usize;
            let mask = 1u64 << bit;
            self.bitmap[word] |= mask;
            self.allocated[word] |= mask;
            self.nodes[node].next_word = word;
            self.nodes[node].free_frames -= 1;
            self.free_frames -= 1;
            let address = ((word * 64 + bit) as u64) * PAGE_SIZE;
            return PhysicalAddress::new(address);
        }
        None
    }

    pub fn deallocate(&mut self, frame: PhysicalAddress) -> Result<(), FrameError> {
        if !self.initialized {
            return Err(FrameError::NotInitialized);
        }
        let index = (frame.as_u64() / PAGE_SIZE) as usize;
        if index >= FRAME_COUNT || frame.as_u64() < LOW_MEMORY_END {
            return Err(FrameError::InvalidFrame);
        }
        let word = index / 64;
        let mask = 1u64 << (index % 64);
        if self.allocated[word] & mask == 0 {
            return Err(FrameError::DoubleFree);
        }
        let node = self
            .nodes
            .iter()
            .position(|node| node.online && index >= node.start_frame && index < node.end_frame)
            .ok_or(FrameError::InvalidFrame)?;
        self.allocated[word] &= !mask;
        self.bitmap[word] &= !mask;
        self.nodes[node].next_word = self.nodes[node].next_word.min(word);
        self.nodes[node].free_frames += 1;
        self.free_frames += 1;
        Ok(())
    }

    pub fn stats(&self) -> FrameStats {
        FrameStats {
            free_frames: self.free_frames,
            numa_nodes: self.nodes.iter().filter(|node| node.online).count(),
            local_allocations: self.local_allocations,
            fallback_allocations: self.fallback_allocations,
        }
    }

    pub fn configure_topology(&mut self, ranges: &[NumaRange]) -> Result<(), FrameError> {
        if !self.initialized || ranges.is_empty() {
            return Err(FrameError::InvalidTopology);
        }
        let mut nodes = [NumaNode::empty(); MAX_NUMA_NODES];
        for range in ranges {
            if range.node >= MAX_NUMA_NODES || range.start >= range.end {
                return Err(FrameError::InvalidTopology);
            }
            let start_frame = range.start.div_ceil(PAGE_SIZE).min(FRAME_COUNT as u64) as usize;
            let end_frame = (range.end / PAGE_SIZE).min(FRAME_COUNT as u64) as usize;
            if start_frame >= end_frame || nodes[range.node].online {
                return Err(FrameError::InvalidTopology);
            }
            if nodes.iter().any(|node| {
                node.online && start_frame < node.end_frame && end_frame > node.start_frame
            }) {
                return Err(FrameError::InvalidTopology);
            }
            nodes[range.node] = NumaNode {
                online: true,
                start_frame,
                end_frame,
                next_word: start_frame / 64,
                free_frames: count_clear_range(&self.bitmap, start_frame, end_frame),
            };
        }
        let covered = nodes
            .iter()
            .map(|node| node.free_frames)
            .fold(0usize, usize::saturating_add);
        if covered != self.free_frames {
            return Err(FrameError::InvalidTopology);
        }
        let covered_allocated = nodes
            .iter()
            .filter(|node| node.online)
            .map(|node| count_set_range(&self.allocated, node.start_frame, node.end_frame))
            .fold(0usize, usize::saturating_add);
        let allocated = self
            .allocated
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum::<usize>();
        if covered_allocated != allocated {
            return Err(FrameError::InvalidTopology);
        }
        self.nodes = nodes;
        self.local_allocations = 0;
        self.fallback_allocations = 0;
        Ok(())
    }

    fn node_online(&self, node: usize) -> bool {
        node < MAX_NUMA_NODES && self.nodes[node].online
    }

    fn mark_range(&mut self, start: u64, end: u64, unavailable: bool) {
        let first = if unavailable {
            start / PAGE_SIZE
        } else {
            start.saturating_add(PAGE_SIZE - 1) / PAGE_SIZE
        };
        let last = if unavailable {
            end.saturating_add(PAGE_SIZE - 1) / PAGE_SIZE
        } else {
            end / PAGE_SIZE
        };
        let first = first.min(FRAME_COUNT as u64) as usize;
        let last = last.min(FRAME_COUNT as u64) as usize;
        if first >= last {
            return;
        }

        let mut cursor = first;
        while cursor < last {
            let word = cursor / 64;
            let word_end = ((word + 1) * 64).min(last);
            let width = word_end - cursor;
            let mask = if width == 64 {
                u64::MAX
            } else {
                ((1u64 << width) - 1) << (cursor % 64)
            };
            self.apply_mask(word, mask, unavailable);
            cursor = word_end;
        }
    }

    fn apply_mask(&mut self, word: usize, mask: u64, unavailable: bool) {
        if unavailable {
            unsafe {
                let pointer = self.bitmap.as_mut_ptr().add(word);
                let current = core::ptr::read_volatile(pointer);
                core::ptr::write_volatile(pointer, current | mask);
            }
        } else {
            unsafe {
                let pointer = self.bitmap.as_mut_ptr().add(word);
                let current = core::ptr::read_volatile(pointer);
                core::ptr::write_volatile(pointer, current & !mask);
            }
        }
    }
}

fn frame_word_mask(word: usize, start_frame: usize, end_frame: usize) -> u64 {
    let word_start = word * 64;
    let first = start_frame.saturating_sub(word_start).min(64);
    let last = end_frame.saturating_sub(word_start).min(64);
    if first >= last {
        return 0;
    }
    let low = if first == 0 {
        u64::MAX
    } else {
        u64::MAX << first
    };
    let high = if last == 64 {
        u64::MAX
    } else {
        (1u64 << last) - 1
    };
    low & high
}

fn count_clear_range(bitmap: &[u64; BITMAP_WORDS], start_frame: usize, end_frame: usize) -> usize {
    let first_word = start_frame / 64;
    let last_word = end_frame.div_ceil(64);
    (first_word..last_word)
        .map(|word| {
            (!bitmap[word] & frame_word_mask(word, start_frame, end_frame)).count_ones() as usize
        })
        .sum()
}

fn count_set_range(bitmap: &[u64; BITMAP_WORDS], start_frame: usize, end_frame: usize) -> usize {
    let first_word = start_frame / 64;
    let last_word = end_frame.div_ceil(64);
    (first_word..last_word)
        .map(|word| {
            (bitmap[word] & frame_word_mask(word, start_frame, end_frame)).count_ones() as usize
        })
        .sum()
}

// Physical memory sits below page tables and heaps in the global lock order.
static FRAME_ALLOCATOR: IrqSpinMutex<BitmapFrameAllocator, 10> =
    IrqSpinMutex::new(BitmapFrameAllocator::new());
static CPU_NUMA_NODES: [AtomicU8; MAX_NUMA_CPUS] = [const { AtomicU8::new(0) }; MAX_NUMA_CPUS];

pub fn initialize(map: &MemoryMap) -> FrameStats {
    let mut allocator = FRAME_ALLOCATOR.lock();
    allocator.initialize(map);
    allocator.stats()
}

pub fn allocate() -> Result<PhysicalAddress, FrameError> {
    allocate_on_node(current_node())
}

pub fn allocate_on_node(preferred: usize) -> Result<PhysicalAddress, FrameError> {
    FRAME_ALLOCATOR.lock().allocate_on_node(preferred)
}

pub fn deallocate(frame: PhysicalAddress) -> Result<(), FrameError> {
    FRAME_ALLOCATOR.lock().deallocate(frame)
}

pub fn stats() -> FrameStats {
    FRAME_ALLOCATOR.lock().stats()
}

pub fn configure_topology(ranges: &[NumaRange]) -> Result<FrameStats, FrameError> {
    let mut allocator = FRAME_ALLOCATOR.lock();
    allocator.configure_topology(ranges)?;
    Ok(allocator.stats())
}

pub fn set_cpu_node(cpu: usize, node: usize) -> Result<(), FrameError> {
    if cpu >= MAX_NUMA_CPUS || !FRAME_ALLOCATOR.lock().node_online(node) {
        return Err(FrameError::InvalidTopology);
    }
    CPU_NUMA_NODES[cpu].store(node as u8, Ordering::Release);
    Ok(())
}

pub fn current_node() -> usize {
    let cpu = (crate::arch::current_cpu_id() as usize).min(MAX_NUMA_CPUS - 1);
    CPU_NUMA_NODES[cpu].load(Ordering::Acquire) as usize
}

#[cfg(target_arch = "x86_64")]
fn kernel_start() -> u64 {
    unsafe extern "C" {
        static boot_start: u8;
    }
    core::ptr::addr_of!(boot_start) as u64
}

#[cfg(not(target_arch = "x86_64"))]
fn kernel_start() -> u64 {
    unsafe extern "C" {
        static kernel_start: u8;
    }
    core::ptr::addr_of!(kernel_start) as u64
}

fn kernel_end() -> u64 {
    unsafe extern "C" {
        static kernel_phys_end: u8;
    }
    core::ptr::addr_of!(kernel_phys_end) as u64
}
