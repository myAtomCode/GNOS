use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use super::{gic, platform};

const TICKS_PER_SECOND: u64 = 100;
static TICKS: AtomicU64 = AtomicU64::new(0);
static INTERVAL: AtomicU64 = AtomicU64::new(0);
static PER_CPU_TICKS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

pub fn init() -> bool {
    let frequency: u64;
    unsafe {
        asm!("mrs {}, cntfrq_el0", out(reg) frequency, options(nomem, nostack, preserves_flags));
    }
    let interval = frequency / TICKS_PER_SECOND;
    if interval == 0 || interval > u32::MAX as u64 {
        return false;
    }
    INTERVAL.store(interval, Ordering::Relaxed);
    init_local();
    true
}

pub fn init_secondary() {
    if INTERVAL.load(Ordering::Acquire) != 0 {
        init_local();
    }
}

fn init_local() {
    gic::enable_interrupt(interrupt_id());
    rearm(INTERVAL.load(Ordering::Relaxed));
}

pub fn interrupt_id() -> u32 {
    platform::timer_interrupt()
}

pub fn handle_interrupt() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    let cpu = (crate::arch::current_cpu_id() as usize).min(PER_CPU_TICKS.len() - 1);
    PER_CPU_TICKS[cpu].fetch_add(1, Ordering::Relaxed);
    rearm(INTERVAL.load(Ordering::Relaxed));
}

pub fn cpu_ticks(cpu: usize) -> u64 {
    PER_CPU_TICKS
        .get(cpu)
        .map_or(0, |ticks| ticks.load(Ordering::Relaxed))
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn control() -> u64 {
    let value: u64;
    unsafe {
        asm!("mrs {}, cntp_ctl_el0", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

fn rearm(interval: u64) {
    unsafe {
        asm!("msr cntp_tval_el0, {}", in(reg) interval, options(nomem, nostack, preserves_flags));
        asm!("msr cntp_ctl_el0, {}", in(reg) 1u64, options(nomem, nostack, preserves_flags));
        asm!("isb", options(nomem, nostack, preserves_flags));
    }
}
