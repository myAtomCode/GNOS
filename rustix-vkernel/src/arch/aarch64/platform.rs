use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use super::board;
use crate::boot::{CpuBootInfo, HardwareDiscovery, PsciConduit};

const MAX_CPUS: usize = 8;
const PSCI_UNKNOWN: u8 = 0;
const PSCI_HVC: u8 = 1;
const PSCI_SMC: u8 = 2;

static DISCOVERED: AtomicBool = AtomicBool::new(false);
static GIC_DISTRIBUTOR: AtomicU64 = AtomicU64::new(0);
static GIC_CPU_INTERFACE: AtomicU64 = AtomicU64::new(0);
static SYSTEM_TIMER: AtomicU64 = AtomicU64::new(0);
static PCIE_CONTROLLER: AtomicU64 = AtomicU64::new(0);
static TIMER_INTERRUPT: AtomicU32 = AtomicU32::new(30);
static PSCI: AtomicU8 = AtomicU8::new(PSCI_UNKNOWN);
static CPU_COUNT: AtomicUsize = AtomicUsize::new(0);
static CPU_MPIDR: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(u64::MAX) }; MAX_CPUS];
static CPU_RELEASE: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static CPU_SPIN_TABLE: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];

pub fn configure(discovery: HardwareDiscovery) {
    if !discovery.valid {
        return;
    }
    if discovery.gic_distributor_base != 0 && discovery.gic_cpu_interface_base != 0 {
        GIC_DISTRIBUTOR.store(discovery.gic_distributor_base, Ordering::Relaxed);
        GIC_CPU_INTERFACE.store(discovery.gic_cpu_interface_base, Ordering::Relaxed);
    }
    if discovery.system_timer_base != 0 {
        SYSTEM_TIMER.store(discovery.system_timer_base, Ordering::Relaxed);
    }
    if discovery.pcie_controller_base != 0 {
        PCIE_CONTROLLER.store(discovery.pcie_controller_base, Ordering::Relaxed);
    }
    TIMER_INTERRUPT.store(discovery.physical_timer_interrupt, Ordering::Relaxed);
    PSCI.store(
        match discovery.psci_conduit {
            PsciConduit::Unknown => PSCI_UNKNOWN,
            PsciConduit::Hvc => PSCI_HVC,
            PsciConduit::Smc => PSCI_SMC,
        },
        Ordering::Relaxed,
    );
    let cpu_count = discovery.cpu_count.min(MAX_CPUS);
    for index in 0..cpu_count {
        let cpu = discovery.cpus[index];
        if cpu.enabled {
            CPU_MPIDR[index].store(cpu.mpidr, Ordering::Relaxed);
            CPU_RELEASE[index].store(cpu.release_address, Ordering::Relaxed);
            CPU_SPIN_TABLE[index].store(cpu.spin_table, Ordering::Relaxed);
        }
    }
    CPU_COUNT.store(cpu_count, Ordering::Release);
    DISCOVERED.store(true, Ordering::Release);
}

pub fn gic_distributor_base() -> usize {
    let address = GIC_DISTRIBUTOR.load(Ordering::Relaxed);
    if address == 0 {
        board::GIC_DISTRIBUTOR_BASE
    } else {
        address as usize
    }
}

pub fn gic_cpu_interface_base() -> usize {
    let address = GIC_CPU_INTERFACE.load(Ordering::Relaxed);
    if address == 0 {
        board::GIC_CPU_INTERFACE_BASE
    } else {
        address as usize
    }
}

pub fn system_timer_base() -> Option<usize> {
    let address = SYSTEM_TIMER.load(Ordering::Relaxed);
    if address == 0 {
        board::SYSTEM_TIMER_BASE
    } else {
        Some(address as usize)
    }
}

pub fn timer_interrupt() -> u32 {
    TIMER_INTERRUPT.load(Ordering::Relaxed)
}

pub fn pcie_controller_base() -> Option<usize> {
    let address = PCIE_CONTROLLER.load(Ordering::Relaxed);
    (address != 0).then_some(address as usize)
}

pub fn cpu_count() -> usize {
    CPU_COUNT.load(Ordering::Acquire)
}

pub fn discovered() -> bool {
    DISCOVERED.load(Ordering::Acquire)
}

pub fn start_cpu(logical_id: usize, entry: u64) -> i64 {
    let cpu = cpu_info(logical_id);
    if cpu.spin_table && cpu.release_address != 0 {
        unsafe {
            (cpu.release_address as *mut u64).write_volatile(entry);
            asm!(
                "dc cvac, {}",
                "dsb sy",
                "sev",
                in(reg) cpu.release_address,
                options(nostack, preserves_flags)
            );
        }
        return 0;
    }
    let target = if cpu.mpidr == u64::MAX {
        logical_id as u64
    } else {
        cpu.mpidr
    };
    invoke_psci_cpu_on(target, entry, logical_id as u64)
}

fn cpu_info(logical_id: usize) -> CpuBootInfo {
    for index in 0..cpu_count() {
        let mpidr = CPU_MPIDR[index].load(Ordering::Relaxed);
        if mpidr & 0xff == logical_id as u64 {
            return CpuBootInfo {
                mpidr,
                release_address: CPU_RELEASE[index].load(Ordering::Relaxed),
                spin_table: CPU_SPIN_TABLE[index].load(Ordering::Relaxed),
                enabled: true,
            };
        }
    }
    CpuBootInfo {
        mpidr: u64::MAX,
        release_address: 0,
        spin_table: false,
        enabled: false,
    }
}

fn invoke_psci_cpu_on(target: u64, entry: u64, context: u64) -> i64 {
    let mut function = 0xc400_0003u64;
    let conduit = PSCI.load(Ordering::Relaxed);
    unsafe {
        if conduit == PSCI_HVC || (conduit == PSCI_UNKNOWN && board::DEFAULT_PSCI_HVC) {
            asm!(
                "hvc #0",
                inout("x0") function,
                in("x1") target,
                in("x2") entry,
                in("x3") context,
                options(nostack)
            );
        } else {
            asm!(
                "smc #0",
                inout("x0") function,
                in("x1") target,
                in("x2") entry,
                in("x3") context,
                options(nostack)
            );
        }
    }
    function as i64
}
