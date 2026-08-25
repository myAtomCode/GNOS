use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};

use super::{ClockTime, ConsoleInput, PlatformInfo};
use crate::fixed::InlineString;

#[path = "arch/aarch64/board/mod.rs"]
mod board;
#[path = "arch/aarch64/gic.rs"]
mod gic;
#[path = "arch/aarch64/platform.rs"]
mod platform;
#[path = "arch/aarch64/rp1.rs"]
mod rp1;
#[path = "arch/aarch64/system_timer.rs"]
mod system_timer;
#[path = "arch/aarch64/timer.rs"]
mod timer;

const PL011_DR: usize = 0x00;
const PL011_FR: usize = 0x18;
const PL011_IBRD: usize = 0x24;
const PL011_FBRD: usize = 0x28;
const PL011_LCRH: usize = 0x2C;
const PL011_CR: usize = 0x30;
const PL011_ICR: usize = 0x44;

const PL011_FR_RXFE: u32 = 1 << 4;
const PL011_FR_TXFF: u32 = 1 << 5;
const PL011_CR_UARTEN: u32 = 1 << 0;
const PL011_CR_TXE: u32 = 1 << 8;
const PL011_CR_RXE: u32 = 1 << 9;

const CONSOLE_COLS: usize = 80;
const CONSOLE_ROWS: usize = 25;
const MAX_CPUS: usize = 4;
const MAX_LOCK_DEPTH: usize = 8;

struct PerCpuState {
    irq_disable_depth: AtomicU32,
    lock_depth: AtomicU8,
    lock_ranks: [AtomicU8; MAX_LOCK_DEPTH],
}

impl PerCpuState {
    const fn new() -> Self {
        Self {
            irq_disable_depth: AtomicU32::new(0),
            lock_depth: AtomicU8::new(0),
            lock_ranks: [const { AtomicU8::new(0) }; MAX_LOCK_DEPTH],
        }
    }
}

static PER_CPU: [PerCpuState; MAX_CPUS] = [const { PerCpuState::new() }; MAX_CPUS];
static ONLINE_MASK: AtomicU32 = AtomicU32::new(1);
static ONLINE_CPUS: AtomicUsize = AtomicUsize::new(1);
static GIC_INTERRUPT_LINES: AtomicUsize = AtomicUsize::new(0);
static TIMER_AVAILABLE: AtomicBool = AtomicBool::new(false);
static IRQ_LOCK_OK: AtomicBool = AtomicBool::new(false);
static TLB_IPI_COUNT: AtomicUsize = AtomicUsize::new(0);
const TLB_SHOOTDOWN_IPI: u32 = 1;

pub const PLATFORM_INFO: PlatformInfo = PlatformInfo {
    name: board::NAME,
    shell_banner: board::SHELL_BANNER,
    uname: board::UNAME,
    proc_version: board::PROC_VERSION,
    ipc_trace_supported: false,
    backspace_echo: "\x08 \x08",
};

pub fn configure_boot(protocol: u64, information: u64) {
    if protocol == crate::boot::PROTOCOL_DEVICE_TREE {
        platform::configure(crate::boot::discover_hardware(information));
    }
}

pub fn init(_protocol: u64, _information: u64) {
    unsafe {
        asm!(
            "msr daifset, #0xf",
            options(nomem, nostack, preserves_flags)
        );
    }
    install_exception_vectors();
    init_uart();
    clear_screen();
    let gic_info = gic::init_primary();
    GIC_INTERRUPT_LINES.store(gic_info.interrupt_lines, Ordering::Relaxed);
    IRQ_LOCK_OK.store(crate::sync::irq_lock_self_test(), Ordering::Relaxed);
    TIMER_AVAILABLE.store(timer::init(), Ordering::Release);
    rp1::probe();
    start_secondary_cpus();
    unsafe {
        asm!(
            "msr daifclr, #2",
            "isb",
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[repr(C)]
struct ExceptionFrame {
    registers: [u64; 31],
    sp_el0: u64,
    elr_el1: u64,
    spsr_el1: u64,
}

fn install_exception_vectors() {
    unsafe extern "C" {
        static vkernel_aarch64_vectors: u8;
    }
    unsafe {
        asm!(
            "msr vbar_el1, {}",
            "isb",
            in(reg) core::ptr::addr_of!(vkernel_aarch64_vectors),
            options(nostack, preserves_flags)
        );
    }
}

#[no_mangle]
extern "C" fn vkernel_aarch64_svc_dispatch(frame: &mut ExceptionFrame) {
    let syndrome: u64;
    unsafe {
        asm!("mrs {}, esr_el1", out(reg) syndrome, options(nomem, nostack, preserves_flags));
    }
    if syndrome >> 26 != 0x15 {
        loop {
            unsafe { asm!("wfi", options(nomem, nostack)) };
        }
    }
    let arguments = [
        frame.registers[0],
        frame.registers[1],
        frame.registers[2],
        frame.registers[3],
        frame.registers[4],
        frame.registers[5],
    ];
    frame.registers[0] = crate::linux::syscall::dispatch(frame.registers[8], arguments);
}

#[no_mangle]
extern "C" fn vkernel_aarch64_unexpected_dispatch() -> ! {
    let syndrome: u64;
    let fault_address: u64;
    let return_address: u64;
    unsafe {
        asm!("mrs {}, esr_el1", out(reg) syndrome, options(nomem, nostack, preserves_flags));
        asm!("mrs {}, far_el1", out(reg) fault_address, options(nomem, nostack, preserves_flags));
        asm!("mrs {}, elr_el1", out(reg) return_address, options(nomem, nostack, preserves_flags));
    }
    crate::println!(
        "[exception] ESR={:#x} FAR={:#x} ELR={:#x}",
        syndrome,
        fault_address,
        return_address
    );
    loop {
        unsafe { asm!("wfi", options(nomem, nostack)) };
    }
}

#[no_mangle]
extern "C" fn vkernel_aarch64_irq_dispatch() {
    let Some(acknowledge) = gic::acknowledge() else {
        return;
    };
    if acknowledge & 0x3ff == timer::interrupt_id() {
        timer::handle_interrupt();
    } else if acknowledge & 0x3ff == TLB_SHOOTDOWN_IPI {
        invalidate_local_tlb();
        TLB_IPI_COUNT.fetch_add(1, Ordering::Release);
    }
    gic::end_interrupt(acknowledge);
}

#[no_mangle]
pub extern "C" fn _secondary_start_rust(cpu_id: u64) -> ! {
    install_exception_vectors();
    gic::init_secondary();
    timer::init_secondary();
    let bit = 1u32 << (cpu_id as u32).min((MAX_CPUS - 1) as u32);
    if ONLINE_MASK.fetch_or(bit, Ordering::AcqRel) & bit == 0 {
        ONLINE_CPUS.fetch_add(1, Ordering::AcqRel);
    }
    unsafe {
        asm!(
            "msr daifclr, #2",
            "isb",
            options(nomem, nostack, preserves_flags)
        );
    }
    loop {
        unsafe { asm!("wfi", options(nomem, nostack)) };
    }
}

pub struct Aarch64HardwareInfo {
    pub mmu: bool,
    pub gic_interrupt_lines: usize,
    pub timer: bool,
    pub timer_ticks: u64,
    pub cpus_online: usize,
    pub irq_lock: bool,
    pub timer_control: u64,
    pub timer_irq_enabled: bool,
    pub irq_masked: bool,
    pub gic_state: gic::GicState,
    pub tlb_ipi: bool,
    pub per_cpu_timer: bool,
    pub rp1: rp1::Rp1Info,
}

pub fn hardware_info() -> Aarch64HardwareInfo {
    let control: u64;
    unsafe {
        asm!("mrs {}, sctlr_el1", out(reg) control, options(nomem, nostack, preserves_flags));
    }
    let interrupt_state: u64;
    unsafe {
        asm!("mrs {}, daif", out(reg) interrupt_state, options(nomem, nostack, preserves_flags));
    }
    let start = monotonic_time_ns();
    while timer::ticks() == 0 && monotonic_time_ns().saturating_sub(start) < 50_000_000 {
        core::hint::spin_loop();
    }
    let online = ONLINE_CPUS.load(Ordering::Acquire);
    let previous_ipis = TLB_IPI_COUNT.load(Ordering::Acquire);
    if online > 1 {
        gic::send_sgi_all_others(TLB_SHOOTDOWN_IPI);
    }
    let ipi_start = monotonic_time_ns();
    while TLB_IPI_COUNT
        .load(Ordering::Acquire)
        .saturating_sub(previous_ipis)
        < online.saturating_sub(1)
        && monotonic_time_ns().saturating_sub(ipi_start) < 50_000_000
    {
        core::hint::spin_loop();
    }
    let timer_start = monotonic_time_ns();
    while !(0..online.min(MAX_CPUS)).all(|cpu| timer::cpu_ticks(cpu) != 0)
        && monotonic_time_ns().saturating_sub(timer_start) < 50_000_000
    {
        core::hint::spin_loop();
    }
    let per_cpu_timer = (0..online.min(MAX_CPUS)).all(|cpu| timer::cpu_ticks(cpu) != 0);
    Aarch64HardwareInfo {
        mmu: control & 1 != 0,
        gic_interrupt_lines: GIC_INTERRUPT_LINES.load(Ordering::Relaxed),
        timer: TIMER_AVAILABLE.load(Ordering::Acquire),
        timer_ticks: timer::ticks(),
        cpus_online: online,
        irq_lock: IRQ_LOCK_OK.load(Ordering::Relaxed),
        timer_control: timer::control(),
        timer_irq_enabled: gic::interrupt_enabled(timer::interrupt_id()),
        irq_masked: interrupt_state & (1 << 7) != 0,
        gic_state: gic::state(),
        tlb_ipi: online <= 1
            || TLB_IPI_COUNT
                .load(Ordering::Acquire)
                .saturating_sub(previous_ipis)
                >= online - 1,
        per_cpu_timer,
        rp1: rp1::info(),
    }
}

pub fn shootdown_tlb_all() {
    unsafe {
        asm!("dsb ishst", options(nostack, preserves_flags));
    }
    gic::send_sgi_all_others(TLB_SHOOTDOWN_IPI);
    invalidate_local_tlb();
}

pub fn write_xor_execute_enabled() -> bool {
    unsafe extern "C" {
        static text_start: u8;
        static data_start: u8;
    }
    let text = page_descriptor(core::ptr::addr_of!(text_start) as u64);
    let data = page_descriptor(core::ptr::addr_of!(data_start) as u64);
    let control: u64;
    unsafe {
        asm!("mrs {}, sctlr_el1", out(reg) control, options(nomem, nostack, preserves_flags));
    }
    let text_read_only = text & (1 << 7) != 0;
    let text_privileged_executable = text & (1 << 53) == 0;
    let data_writable = data & (1 << 7) == 0;
    let data_execute_never = data & (1 << 53) != 0 && data & (1 << 54) != 0;
    text & 3 == 3
        && data & 3 == 3
        && text_read_only
        && text_privileged_executable
        && data_writable
        && data_execute_never
        && control & (1 << 19) != 0
}

pub fn stack_guard_enabled() -> bool {
    unsafe extern "C" {
        static stack_guard_start: u8;
    }
    page_descriptor(core::ptr::addr_of!(stack_guard_start) as u64) == 0
}

fn page_descriptor(address: u64) -> u64 {
    unsafe extern "C" {
        static vkernel_aarch64_kernel_l3_tables: u64;
        static kernel_start: u8;
    }
    let window = (core::ptr::addr_of!(kernel_start) as u64) & !((1u64 << 30) - 1);
    let Some(offset) = address.checked_sub(window) else {
        return 0;
    };
    let index = (offset / 4096) as usize;
    if index >= 2048 {
        return 0;
    }
    unsafe {
        core::ptr::addr_of!(vkernel_aarch64_kernel_l3_tables)
            .add(index)
            .read_volatile()
    }
}

fn invalidate_local_tlb() {
    unsafe {
        asm!(
            "dsb ish",
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            options(nostack, preserves_flags)
        );
    }
}

fn start_secondary_cpus() {
    unsafe extern "C" {
        static vkernel_aarch64_secondary_release: AtomicU32;
        static vkernel_aarch64_secondary_entry: u8;
    }
    unsafe {
        vkernel_aarch64_secondary_release.store(1, Ordering::Release);
        asm!("dsb ishst", "sev", options(nomem, nostack, preserves_flags));
    }
    if board::SECONDARIES_ENTER_IMAGE {
        let start = monotonic_time_ns();
        while ONLINE_CPUS.load(Ordering::Acquire) < MAX_CPUS
            && monotonic_time_ns().saturating_sub(start) < 100_000_000
        {
            core::hint::spin_loop();
        }
    }
    let entry = core::ptr::addr_of!(vkernel_aarch64_secondary_entry) as u64;
    for cpu_id in 1..MAX_CPUS as u64 {
        if ONLINE_MASK.load(Ordering::Acquire) & (1 << cpu_id) == 0 {
            let _ = platform::start_cpu(cpu_id as usize, entry);
        }
    }
}

fn per_cpu() -> &'static PerCpuState {
    &PER_CPU[(current_cpu_id() as usize).min(MAX_CPUS - 1)]
}

pub fn lock_order_allows(rank: u8) -> bool {
    let cpu = per_cpu();
    let depth = cpu.lock_depth.load(Ordering::Relaxed) as usize;
    rank != 0
        && depth < cpu.lock_ranks.len()
        && (depth == 0 || cpu.lock_ranks[depth - 1].load(Ordering::Relaxed) < rank)
}

pub fn push_lock_rank(rank: u8) -> bool {
    if !lock_order_allows(rank) {
        return false;
    }
    let cpu = per_cpu();
    let depth = cpu.lock_depth.load(Ordering::Relaxed) as usize;
    cpu.lock_ranks[depth].store(rank, Ordering::Relaxed);
    cpu.lock_depth.store((depth + 1) as u8, Ordering::Relaxed);
    true
}

pub fn pop_lock_rank(rank: u8) -> bool {
    let cpu = per_cpu();
    let depth = cpu.lock_depth.load(Ordering::Relaxed) as usize;
    if depth == 0 || cpu.lock_ranks[depth - 1].load(Ordering::Relaxed) != rank {
        return false;
    }
    cpu.lock_ranks[depth - 1].store(0, Ordering::Relaxed);
    cpu.lock_depth.store((depth - 1) as u8, Ordering::Relaxed);
    true
}

pub fn irq_disabled_enter() {
    per_cpu().irq_disable_depth.fetch_add(1, Ordering::Relaxed);
}

pub fn irq_disabled_exit() -> bool {
    per_cpu()
        .irq_disable_depth
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |depth| {
            depth.checked_sub(1)
        })
        .is_ok()
}

pub fn current_cpu_id() -> u32 {
    let affinity: u64;
    unsafe {
        asm!("mrs {}, mpidr_el1", out(reg) affinity, options(nomem, nostack, preserves_flags));
    }
    (affinity & 0xff) as u32
}

pub fn online_cpu_count() -> usize {
    ONLINE_CPUS.load(Ordering::Acquire)
}

pub fn irq_disable_depth() -> u32 {
    per_cpu().irq_disable_depth.load(Ordering::Relaxed)
}

pub fn console_write(text: &str) {
    for byte in text.bytes() {
        if byte == b'\n' {
            uart_write_byte(b'\r');
        }
        uart_write_byte(byte);
    }
}

pub fn console_read_byte() -> Option<u8> {
    if uart_read_ready() {
        let byte = unsafe { mmio_read32(board::UART_BASE + PL011_DR) as u8 };
        Some(match byte {
            b'\r' => b'\n',
            0x7f => 0x08,
            other => other,
        })
    } else {
        None
    }
}

pub fn poll_console_input() -> Option<ConsoleInput> {
    console_read_byte().map(ConsoleInput::Byte)
}

pub fn clear_screen() {
    console_write("\x1b[2J\x1b[H");
}

pub fn graphics_mode_enabled() -> bool {
    false
}

pub fn screen_width() -> i32 {
    0
}

pub fn screen_height() -> i32 {
    0
}

pub fn gfx_clear(_color: u8) {}

pub fn gfx_fill_rect(_x: i32, _y: i32, _width: i32, _height: i32, _color: u8) {}

pub fn gfx_fill_rect_alpha(_x: i32, _y: i32, _width: i32, _height: i32, _color: u8, _alpha: u8) {}

pub fn gfx_blit_fullscreen(_pixels: &[u8]) {}

pub fn gfx_present() {}

pub fn gfx_move_cursor(
    _old_x: i32,
    _old_y: i32,
    _new_x: i32,
    _new_y: i32,
    _outline: u8,
    _fill: u8,
) {
}

pub fn gfx_fill_circle(_cx: i32, _cy: i32, _radius: i32, _color: u8) {}

pub fn gfx_fill_circle_alpha(_cx: i32, _cy: i32, _radius: i32, _color: u8, _alpha: u8) {}

pub fn gfx_fill_round_rect(_x: i32, _y: i32, _width: i32, _height: i32, _radius: i32, _color: u8) {}

pub fn gfx_fill_round_rect_alpha(
    _x: i32,
    _y: i32,
    _width: i32,
    _height: i32,
    _radius: i32,
    _color: u8,
    _alpha: u8,
) {
}

pub fn draw_text(_x: i32, _y: i32, _text: &str, _color: u8, _scale: i32) {}

pub fn text_width(text: &str, scale: i32) -> i32 {
    if scale <= 0 {
        0
    } else {
        (text.chars().count() as i32) * 6 * scale
    }
}

pub fn draw_text_compact(_x: i32, _y: i32, _text: &str, _color: u8, _scale: i32) {}

pub fn gfx_console_reset() {}

pub fn gfx_set_console_origin(_x: i32, _y: i32) {}

pub fn gfx_set_console_visible(_visible: bool) {}

pub fn gfx_console_rows() -> usize {
    CONSOLE_ROWS
}

pub fn gfx_console_cols() -> usize {
    CONSOLE_COLS
}

pub fn gfx_console_line<const N: usize>(_row: usize, out: &mut InlineString<N>) {
    out.clear();
}

pub fn gfx_console_cursor() -> (usize, usize) {
    (0, 0)
}

pub fn gfx_console_cell(_row: usize, _col: usize) -> u16 {
    0
}

pub fn gfx_console_redraw() {}

pub fn set_palette_entry(_index: u8, _red: u8, _green: u8, _blue: u8) {}

pub fn delay(cycles: u32) {
    for _ in 0..cycles {
        unsafe {
            asm!("nop", options(nomem, nostack, preserves_flags));
        }
    }
}

pub fn monotonic_time_ns() -> u64 {
    if let Some(time) = system_timer::counter_ns() {
        return time;
    }
    let counter: u64;
    let frequency: u64;
    unsafe {
        asm!("mrs {}, cntvct_el0", out(reg) counter, options(nomem, nostack, preserves_flags));
        asm!("mrs {}, cntfrq_el0", out(reg) frequency, options(nomem, nostack, preserves_flags));
    }
    if frequency == 0 {
        return 0;
    }
    let seconds = counter / frequency;
    let remainder = counter % frequency;
    seconds.saturating_mul(1_000_000_000).saturating_add(
        remainder
            .saturating_mul(1_000_000_000)
            .checked_div(frequency)
            .unwrap_or(0),
    )
}

pub fn wait_for_interrupt() {
    unsafe { asm!("wfi", options(nomem, nostack, preserves_flags)) };
}

pub fn read_clock_time() -> Option<ClockTime> {
    None
}

pub fn speaker_play(_frequency_hz: u32) {}

pub fn speaker_stop() {}

pub fn shutdown() -> ! {
    board::power_off()
}

pub fn port_out8(_port: u16, _value: u8) {}

pub fn port_out16(_port: u16, _value: u16) {}

pub fn port_out32(_port: u16, _value: u32) {}

pub fn port_in8(_port: u16) -> u8 {
    0
}

pub fn port_in16(_port: u16) -> u16 {
    0
}

pub fn port_in32(_port: u16) -> u32 {
    0
}

fn init_uart() {
    if !board::UART_REQUIRES_INIT {
        return;
    }
    unsafe {
        mmio_write32(board::UART_BASE + PL011_CR, 0);
        mmio_write32(board::UART_BASE + PL011_ICR, 0x7ff);
        mmio_write32(board::UART_BASE + PL011_IBRD, board::UART_IBRD);
        mmio_write32(board::UART_BASE + PL011_FBRD, board::UART_FBRD);
        mmio_write32(board::UART_BASE + PL011_LCRH, (1 << 4) | (3 << 5));
        mmio_write32(
            board::UART_BASE + PL011_CR,
            PL011_CR_UARTEN | PL011_CR_TXE | PL011_CR_RXE,
        );
    }
}

fn uart_read_ready() -> bool {
    unsafe { mmio_read32(board::UART_BASE + PL011_FR) & PL011_FR_RXFE == 0 }
}

fn uart_write_byte(byte: u8) {
    while unsafe { mmio_read32(board::UART_BASE + PL011_FR) } & PL011_FR_TXFF != 0 {}
    unsafe {
        mmio_write32(board::UART_BASE + PL011_DR, byte as u32);
    }
}

unsafe fn mmio_write32(addr: usize, value: u32) {
    (addr as *mut u32).write_volatile(value);
}

unsafe fn mmio_read32(addr: usize) -> u32 {
    (addr as *const u32).read_volatile()
}
