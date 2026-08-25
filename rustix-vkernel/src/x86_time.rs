use core::arch::asm;
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};

use super::acpi::AcpiInfo;
use super::interrupts;
use crate::mm::address::PHYSICAL_MEMORY_OFFSET;

const HPET_CAPABILITIES: u64 = 0x000;
const HPET_CONFIGURATION: u64 = 0x010;
const HPET_MAIN_COUNTER: u64 = 0x0f0;
const FEMTOSECONDS_PER_SECOND: u64 = 1_000_000_000_000_000;
const CALIBRATION_NS: u64 = 10_000_000;
const CLOCK_NONE: u8 = 0;
const CLOCK_TSC: u8 = 1;
const CLOCK_HPET: u8 = 2;
const CLOCK_APIC: u8 = 3;

static HPET_BASE: AtomicU64 = AtomicU64::new(0);
static HPET_PERIOD_FS: AtomicU64 = AtomicU64::new(0);
static TSC_HZ: AtomicU64 = AtomicU64::new(0);
static CLOCK_SOURCE: AtomicU8 = AtomicU8::new(CLOCK_NONE);
static CLOCK_EPOCH: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub struct ClockInfo {
    pub source: &'static str,
    pub invariant_tsc: bool,
    pub tsc_hz: u64,
    pub hpet_hz: u64,
    pub apic_hz: u64,
    pub timer_hz: u32,
}

pub fn init(acpi: AcpiInfo, local_apic: bool) -> ClockInfo {
    let hpet_hz = acpi.hpet_address.and_then(init_hpet).unwrap_or(0);
    let invariant_tsc = has_invariant_tsc();
    let tsc_hz = detect_tsc_frequency(hpet_hz != 0);
    TSC_HZ.store(tsc_hz, Ordering::Release);

    let apic_hz = if local_apic {
        calibrate_apic_timer()
    } else {
        0
    };
    let timer_hz = if apic_hz != 0 {
        let reload = (apic_hz / 1000).clamp(1, u32::MAX as u64) as u32;
        if interrupts::start_periodic_timer(reload) {
            1000
        } else {
            0
        }
    } else {
        0
    };

    let (source, source_id, epoch) = if invariant_tsc && tsc_hz != 0 {
        ("invariant-tsc", CLOCK_TSC, ordered_tsc())
    } else if hpet_hz != 0 {
        ("hpet", CLOCK_HPET, hpet_counter())
    } else if timer_hz != 0 {
        ("apic-timer", CLOCK_APIC, interrupts::timer_ticks())
    } else {
        ("none", CLOCK_NONE, 0)
    };
    CLOCK_EPOCH.store(epoch, Ordering::Release);
    CLOCK_SOURCE.store(source_id, Ordering::Release);
    ClockInfo {
        source,
        invariant_tsc,
        tsc_hz,
        hpet_hz,
        apic_hz,
        timer_hz,
    }
}

pub fn monotonic_nanoseconds() -> u64 {
    let epoch = CLOCK_EPOCH.load(Ordering::Acquire);
    match CLOCK_SOURCE.load(Ordering::Acquire) {
        CLOCK_TSC => {
            let hz = TSC_HZ.load(Ordering::Acquire);
            scale_ticks(ordered_tsc().wrapping_sub(epoch), hz)
        }
        CLOCK_HPET => {
            let period = HPET_PERIOD_FS.load(Ordering::Acquire);
            ((hpet_counter().wrapping_sub(epoch) as u128 * period as u128) / 1_000_000) as u64
        }
        CLOCK_APIC => interrupts::timer_ticks().wrapping_sub(epoch) * 1_000_000,
        _ => 0,
    }
}

pub fn busy_wait_nanoseconds(nanoseconds: u64) -> bool {
    if nanoseconds == 0 {
        return true;
    }
    if HPET_BASE.load(Ordering::Acquire) != 0 {
        return hpet_wait(nanoseconds);
    }
    let frequency = TSC_HZ.load(Ordering::Acquire);
    if frequency == 0 {
        return false;
    }
    let target = ((nanoseconds as u128 * frequency as u128) / 1_000_000_000u128) as u64;
    let start = ordered_tsc();
    while ordered_tsc().wrapping_sub(start) < target {
        core::hint::spin_loop();
    }
    true
}

fn init_hpet(physical: u64) -> Option<u64> {
    if physical >= 0x1_0000_0000 || !crate::mm::paging::map_mmio_uncached(physical, 4096) {
        return None;
    }
    let base = PHYSICAL_MEMORY_OFFSET + physical;
    let capabilities =
        unsafe { core::ptr::read_volatile((base + HPET_CAPABILITIES) as *const u64) };
    let period_fs = capabilities >> 32;
    if period_fs == 0 || period_fs > 100_000_000 {
        return None;
    }
    unsafe {
        let configuration = (base + HPET_CONFIGURATION) as *mut u64;
        core::ptr::write_volatile(configuration, core::ptr::read_volatile(configuration) & !1);
        core::ptr::write_volatile((base + HPET_MAIN_COUNTER) as *mut u64, 0);
        core::ptr::write_volatile(configuration, core::ptr::read_volatile(configuration) | 1);
    }
    HPET_BASE.store(base, Ordering::Release);
    HPET_PERIOD_FS.store(period_fs, Ordering::Release);
    Some(FEMTOSECONDS_PER_SECOND / period_fs)
}

fn detect_tsc_frequency(hpet_available: bool) -> u64 {
    if hpet_available {
        let start = ordered_tsc();
        if hpet_wait(CALIBRATION_NS * 2) {
            return ordered_tsc().wrapping_sub(start).saturating_mul(50);
        }
    }
    let maximum = core::arch::x86_64::__cpuid(0).eax;
    if maximum >= 0x15 {
        let leaf = core::arch::x86_64::__cpuid(0x15);
        if leaf.eax != 0 && leaf.ebx != 0 && leaf.ecx != 0 {
            return (leaf.ecx as u64).saturating_mul(leaf.ebx as u64) / leaf.eax as u64;
        }
    }
    if maximum >= 0x16 {
        let mhz = core::arch::x86_64::__cpuid(0x16).eax;
        if mhz != 0 {
            return mhz as u64 * 1_000_000;
        }
    }
    let start = ordered_tsc();
    if pit_wait_10ms() {
        ordered_tsc().wrapping_sub(start).saturating_mul(100)
    } else {
        0
    }
}

fn calibrate_apic_timer() -> u64 {
    if !interrupts::apic_timer_begin_calibration() {
        return 0;
    }
    let waited = if HPET_BASE.load(Ordering::Acquire) != 0 {
        hpet_wait(CALIBRATION_NS)
    } else {
        pit_wait_10ms()
    };
    let elapsed = interrupts::apic_timer_elapsed();
    interrupts::apic_timer_stop();
    if waited && elapsed > 100 {
        elapsed as u64 * 100
    } else {
        0
    }
}

fn hpet_wait(nanoseconds: u64) -> bool {
    let period = HPET_PERIOD_FS.load(Ordering::Acquire);
    if period == 0 {
        return false;
    }
    let ticks =
        ((nanoseconds as u128 * 1_000_000u128 + period as u128 - 1) / period as u128) as u64;
    let start = hpet_counter();
    for _ in 0..50_000_000usize {
        if hpet_counter().wrapping_sub(start) >= ticks {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn hpet_counter() -> u64 {
    let base = HPET_BASE.load(Ordering::Acquire);
    if base == 0 {
        return 0;
    }
    unsafe { core::ptr::read_volatile((base + HPET_MAIN_COUNTER) as *const u64) }
}

fn pit_wait_10ms() -> bool {
    const COUNT: u16 = 11_932;
    let original = super::port_in8(0x61);
    super::port_out8(0x61, original & !1);
    super::port_out8(0x43, 0xb0);
    super::port_out8(0x42, COUNT as u8);
    super::port_out8(0x42, (COUNT >> 8) as u8);
    super::port_out8(0x61, (original & !1) | 1);
    let mut complete = false;
    for _ in 0..20_000_000usize {
        if super::port_in8(0x61) & 0x20 != 0 {
            complete = true;
            break;
        }
        core::hint::spin_loop();
    }
    super::port_out8(0x61, original);
    complete
}

fn has_invariant_tsc() -> bool {
    let maximum = core::arch::x86_64::__cpuid(0x8000_0000).eax;
    maximum >= 0x8000_0007 && core::arch::x86_64::__cpuid(0x8000_0007).edx & (1 << 8) != 0
}

fn ordered_tsc() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!("lfence", "rdtsc", out("eax") low, out("edx") high, options(nostack));
    }
    (high as u64) << 32 | low as u64
}

fn scale_ticks(ticks: u64, frequency: u64) -> u64 {
    if frequency == 0 {
        0
    } else {
        (ticks as u128 * 1_000_000_000u128 / frequency as u128) as u64
    }
}
