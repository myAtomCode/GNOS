use core::arch::asm;

use super::{board, platform};

const GICD_CTLR: usize = 0x000;
const GICD_TYPER: usize = 0x004;
const GICD_IGROUPR: usize = 0x080;
const GICD_ISENABLER: usize = 0x100;
const GICD_ICENABLER: usize = 0x180;
const GICD_ICPENDR: usize = 0x280;
const GICD_ISPENDR: usize = 0x200;
const GICD_IPRIORITYR: usize = 0x400;
const GICD_ITARGETSR: usize = 0x800;
const GICD_ICFGR: usize = 0xc00;
const GICD_SGIR: usize = 0xf00;

const GICC_CTLR: usize = 0x000;
const GICC_PMR: usize = 0x004;
const GICC_BPR: usize = 0x008;
const GICC_IAR: usize = 0x00c;
const GICC_EOIR: usize = 0x010;
const GICC_HPPIR: usize = 0x018;
const GICC_AIAR: usize = 0x020;
const GICC_AEOIR: usize = 0x024;

const SPURIOUS_INTERRUPT: u32 = 1023;
const GROUP1_PENDING: u32 = 1022;
const ALIAS_ACKNOWLEDGE: u32 = 1 << 31;

pub struct GicInfo {
    pub interrupt_lines: usize,
}

pub struct GicState {
    pub distributor_control: u32,
    pub cpu_control: u32,
    pub group: u32,
    pub pending: u32,
    pub highest_pending: u32,
}

pub fn init_primary() -> GicInfo {
    write_distributor(GICD_CTLR, 0);
    let interrupt_lines = (((read_distributor(GICD_TYPER) & 0x1f) + 1) * 32) as usize;
    let interrupt_lines = interrupt_lines.min(1020);

    for interrupt in (0..interrupt_lines).step_by(32) {
        let offset = (interrupt / 8) as usize;
        write_distributor(GICD_ICENABLER + offset, u32::MAX);
        write_distributor(GICD_ICPENDR + offset, u32::MAX);
        write_distributor(
            GICD_IGROUPR + offset,
            if board::GIC_USE_GROUP1 { u32::MAX } else { 0 },
        );
    }
    for interrupt in (0..interrupt_lines).step_by(4) {
        write_distributor(GICD_IPRIORITYR + interrupt, 0xa0a0_a0a0);
        if interrupt >= 32 {
            write_distributor(GICD_ITARGETSR + interrupt, 0x0101_0101);
        }
    }
    for interrupt in (32..interrupt_lines).step_by(16) {
        write_distributor(GICD_ICFGR + (interrupt / 4), 0);
    }

    write_distributor(GICD_CTLR, if board::GIC_USE_GROUP1 { 3 } else { 1 });
    barrier();
    init_cpu_interface();
    GicInfo { interrupt_lines }
}

pub fn init_secondary() {
    write_distributor(
        GICD_IGROUPR,
        if board::GIC_USE_GROUP1 { u32::MAX } else { 0 },
    );
    init_cpu_interface();
}

pub fn enable_interrupt(interrupt: u32) {
    let register = GICD_ISENABLER + ((interrupt as usize / 32) * 4);
    write_distributor(register, 1 << (interrupt % 32));
    barrier();
}

pub fn interrupt_enabled(interrupt: u32) -> bool {
    let register = GICD_ISENABLER + ((interrupt as usize / 32) * 4);
    read_distributor(register) & (1 << (interrupt % 32)) != 0
}

pub fn state() -> GicState {
    GicState {
        distributor_control: read_distributor(GICD_CTLR),
        cpu_control: read_cpu_interface(GICC_CTLR),
        group: read_distributor(GICD_IGROUPR),
        pending: read_distributor(GICD_ISPENDR),
        highest_pending: read_cpu_interface(GICC_HPPIR),
    }
}

pub fn acknowledge() -> Option<u32> {
    let mut value = read_cpu_interface(GICC_IAR);
    if value & 0x3ff == GROUP1_PENDING {
        value = read_cpu_interface(GICC_AIAR) | ALIAS_ACKNOWLEDGE;
    }
    if value & 0x3ff == SPURIOUS_INTERRUPT {
        None
    } else {
        Some(value)
    }
}

pub fn end_interrupt(acknowledge: u32) {
    if acknowledge & ALIAS_ACKNOWLEDGE != 0 {
        write_cpu_interface(GICC_AEOIR, acknowledge & !ALIAS_ACKNOWLEDGE);
    } else {
        write_cpu_interface(GICC_EOIR, acknowledge);
    }
}

pub fn send_sgi_all_others(interrupt: u32) {
    if interrupt < 16 {
        write_distributor(GICD_SGIR, (1 << 24) | interrupt);
        barrier();
    }
}

fn init_cpu_interface() {
    write_cpu_interface(GICC_CTLR, 0);
    write_cpu_interface(GICC_PMR, 0xff);
    write_cpu_interface(GICC_BPR, 0);
    write_cpu_interface(GICC_CTLR, if board::GIC_USE_GROUP1 { 3 } else { 1 });
    barrier();
}

fn barrier() {
    unsafe {
        asm!("dsb sy", "isb", options(nomem, nostack, preserves_flags));
    }
}

fn read_distributor(offset: usize) -> u32 {
    unsafe { ((platform::gic_distributor_base() + offset) as *const u32).read_volatile() }
}

fn write_distributor(offset: usize, value: u32) {
    unsafe { ((platform::gic_distributor_base() + offset) as *mut u32).write_volatile(value) }
}

fn read_cpu_interface(offset: usize) -> u32 {
    unsafe { ((platform::gic_cpu_interface_base() + offset) as *const u32).read_volatile() }
}

fn write_cpu_interface(offset: usize, value: u32) {
    unsafe { ((platform::gic_cpu_interface_base() + offset) as *mut u32).write_volatile(value) }
}
