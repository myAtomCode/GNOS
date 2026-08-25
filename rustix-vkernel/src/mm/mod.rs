pub mod address;
pub mod frame;
#[cfg(target_arch = "x86_64")]
pub mod user;

#[cfg(target_arch = "x86_64")]
pub mod heap;
#[cfg(target_arch = "x86_64")]
pub mod paging;

use crate::boot::{self, MemoryMap, MemorySource};
use crate::sync::SpinMutex;

static MEMORY_MAP: SpinMutex<MemoryMap> = SpinMutex::new(MemoryMap::new(MemorySource::Unknown));

#[derive(Clone, Copy)]
pub struct MemorySummary {
    pub source: MemorySource,
    pub regions: usize,
    pub usable_bytes: u64,
    pub free_frames: usize,
    pub numa_nodes: usize,
    pub numa_local_allocations: u64,
    pub numa_fallback_allocations: u64,
    pub allocator_self_test: bool,
    pub write_xor_execute: bool,
    pub high_half: bool,
    pub user_isolation: bool,
    pub user_copy: bool,
    pub user_guard: bool,
    pub stack_guard: bool,
    pub heap_bytes: usize,
    pub heap_self_test: bool,
    pub smep: bool,
    pub smap: bool,
}

pub fn initialize(protocol: u64, information: u64) -> MemorySummary {
    let map = boot::parse_memory_map(protocol, information);
    let _ = frame::initialize(&map);
    let allocator_self_test = match frame::allocate() {
        Ok(allocated) => frame::deallocate(allocated).is_ok(),
        Err(_) => false,
    };
    #[cfg(target_arch = "x86_64")]
    let paging_security = paging::initialize_kernel_permissions();
    #[cfg(target_arch = "x86_64")]
    let heap = heap::initialize().ok();
    #[cfg(target_arch = "x86_64")]
    let supervisor = crate::arch::enable_supervisor_protections();
    #[cfg(target_arch = "aarch64")]
    let paging_security = aarch64_paging_security();
    #[cfg(not(target_arch = "x86_64"))]
    let heap: Option<UnavailableHeap> = None;
    #[cfg(not(target_arch = "x86_64"))]
    let supervisor = UnavailableSupervisorProtections {
        smep: false,
        smap: false,
    };
    let stats = frame::stats();
    let summary = MemorySummary {
        source: map.source(),
        regions: map.regions().len(),
        usable_bytes: map.usable_bytes(),
        free_frames: stats.free_frames,
        numa_nodes: stats.numa_nodes,
        numa_local_allocations: stats.local_allocations,
        numa_fallback_allocations: stats.fallback_allocations,
        allocator_self_test,
        write_xor_execute: paging_security.write_xor_execute,
        high_half: paging_security.high_half,
        user_isolation: paging_security.user_isolation,
        user_copy: paging_security.user_copy,
        user_guard: paging_security.user_guard,
        stack_guard: paging_security.stack_guard,
        heap_bytes: heap.map_or(0, |heap| heap.size),
        heap_self_test: heap.is_some_and(|heap| heap.self_test),
        smep: supervisor.smep,
        smap: supervisor.smap,
    };
    *MEMORY_MAP.lock() = map;
    summary
}

#[cfg(not(target_arch = "x86_64"))]
#[derive(Clone, Copy)]
struct UnavailableHeap {
    size: usize,
    self_test: bool,
}

#[cfg(not(target_arch = "x86_64"))]
struct UnavailableSupervisorProtections {
    smep: bool,
    smap: bool,
}

#[cfg(not(target_arch = "x86_64"))]
struct UnavailablePagingSecurity {
    write_xor_execute: bool,
    high_half: bool,
    user_isolation: bool,
    user_copy: bool,
    user_guard: bool,
    stack_guard: bool,
}

#[cfg(target_arch = "aarch64")]
const fn paging_security_unavailable() -> UnavailablePagingSecurity {
    UnavailablePagingSecurity {
        write_xor_execute: cfg!(target_arch = "aarch64"),
        high_half: false,
        user_isolation: false,
        user_copy: false,
        user_guard: false,
        stack_guard: cfg!(target_arch = "aarch64"),
    }
}

#[cfg(target_arch = "aarch64")]
fn aarch64_paging_security() -> UnavailablePagingSecurity {
    UnavailablePagingSecurity {
        write_xor_execute: crate::arch::write_xor_execute_enabled(),
        high_half: false,
        user_isolation: false,
        user_copy: false,
        user_guard: false,
        stack_guard: crate::arch::stack_guard_enabled(),
    }
}

pub fn with_memory_map<R>(operation: impl FnOnce(&MemoryMap) -> R) -> R {
    let map = MEMORY_MAP.lock();
    operation(&map)
}
