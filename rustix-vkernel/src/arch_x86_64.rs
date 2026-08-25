use core::arch::asm;

use super::{ClockTime, ConsoleInput, PlatformInfo};
use crate::fixed::InlineString;
use crate::mm::address::PHYSICAL_MEMORY_OFFSET;
use crate::sync::StaticCell;

#[path = "x86_acpi.rs"]
mod acpi;
#[path = "x86_interrupts.rs"]
mod interrupts;
#[path = "x86_smp.rs"]
mod smp;
#[path = "x86_time.rs"]
mod time;
const COM1: u16 = 0x3F8;
const VGA_BUFFER: *mut u8 = (PHYSICAL_MEMORY_OFFSET + 0xb8000) as *mut u8;
const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 25;
const QEMU_DEBUG_EXIT_PORT: u16 = 0xF4;
const VGA_DAC_INDEX: u16 = 0x3C8;
const VGA_DAC_DATA: u16 = 0x3C9;
const BOOT_VIDEO_INFO_ADDR: usize = 0x90000;
const BOOT_VIDEO_INFO_MAGIC: u32 = 0x5256_4944;
const MULTIBOOT2_BOOTLOADER_MAGIC: u64 = 0x36d7_6289;
const MULTIBOOT2_FRAMEBUFFER_TAG: u32 = 8;
const MULTIBOOT2_FRAMEBUFFER_RGB: u8 = 1;
const MAX_MULTIBOOT_INFO_SIZE: usize = 1024 * 1024;
const DEFAULT_FB_ADDR: usize = (PHYSICAL_MEMORY_OFFSET + 0xA0000) as usize;
const DEFAULT_FB_WIDTH: i32 = 320;
const DEFAULT_FB_HEIGHT: i32 = 200;
const DEFAULT_FB_PITCH: usize = 320;
const DEFAULT_FB_BPP: u8 = 8;
const MAX_FB_WIDTH: usize = 640;
const MAX_FB_HEIGHT: usize = 480;
const MAX_FB_PIXELS: usize = MAX_FB_WIDTH * MAX_FB_HEIGHT;
const GFX_CONSOLE_DEFAULT_X: i32 = 66;
const GFX_CONSOLE_DEFAULT_Y: i32 = 112;
const GFX_CONSOLE_WIDTH: i32 = 240;
const GFX_CONSOLE_HEIGHT: i32 = 152;
const GFX_CHAR_SCALE: i32 = 1;
const GFX_CHAR_WIDTH: i32 = 6;
const GFX_CHAR_HEIGHT: i32 = 8;
const GFX_CONSOLE_COLS: usize = (GFX_CONSOLE_WIDTH / GFX_CHAR_WIDTH) as usize;
const GFX_CONSOLE_ROWS: usize = (GFX_CONSOLE_HEIGHT / GFX_CHAR_HEIGHT) as usize;
const GFX_CONSOLE_BG: u8 = 8;
const GFX_CONSOLE_FG: u8 = 6;
const GFX_CONSOLE_EMPTY: u16 = 0;
const GFX_CONSOLE_WIDE_CONT: u16 = 0xFFFF;

pub const PLATFORM_INFO: PlatformInfo = PlatformInfo {
    name: "x86_64",
    shell_banner: "Rustix v-kernel (x86_64)",
    uname: "Rustix v-kernel x86_64",
    proc_version: "Rustix v-kernel 0.1.0 x86_64\n",
    ipc_trace_supported: true,
    backspace_echo: "\x08",
};

#[repr(C)]
struct BootVideoInfo {
    magic: u32,
    framebuffer_addr: u32,
    width: u16,
    height: u16,
    pitch: u16,
    bpp: u8,
    _reserved: u8,
    red_mask_size: u8,
    red_field_position: u8,
    green_mask_size: u8,
    green_field_position: u8,
    blue_mask_size: u8,
    blue_field_position: u8,
}

static CURSOR_ROW: StaticCell<usize> = StaticCell::new(0);
static CURSOR_COL: StaticCell<usize> = StaticCell::new(0);
static GRAPHICS_MODE: StaticCell<bool> = StaticCell::new(false);
static VIDEO_SOURCE: StaticCell<u8> = StaticCell::new(0);
static FRAMEBUFFER_ADDR: StaticCell<usize> = StaticCell::new(DEFAULT_FB_ADDR);
static FRAMEBUFFER_WIDTH: StaticCell<i32> = StaticCell::new(DEFAULT_FB_WIDTH);
static FRAMEBUFFER_HEIGHT: StaticCell<i32> = StaticCell::new(DEFAULT_FB_HEIGHT);
static FRAMEBUFFER_PITCH: StaticCell<usize> = StaticCell::new(DEFAULT_FB_PITCH);
static FRAMEBUFFER_BPP: StaticCell<u8> = StaticCell::new(DEFAULT_FB_BPP);
static FRAMEBUFFER_RED_MASK_SIZE: StaticCell<u8> = StaticCell::new(0);
static FRAMEBUFFER_RED_SHIFT: StaticCell<u8> = StaticCell::new(16);
static FRAMEBUFFER_GREEN_MASK_SIZE: StaticCell<u8> = StaticCell::new(0);
static FRAMEBUFFER_GREEN_SHIFT: StaticCell<u8> = StaticCell::new(8);
static FRAMEBUFFER_BLUE_MASK_SIZE: StaticCell<u8> = StaticCell::new(0);
static FRAMEBUFFER_BLUE_SHIFT: StaticCell<u8> = StaticCell::new(0);
static DRAW_BUFFER: StaticCell<[u32; MAX_FB_PIXELS]> = StaticCell::new([0; MAX_FB_PIXELS]);
static PALETTE_R: StaticCell<[u8; 256]> = StaticCell::new([0; 256]);
static PALETTE_G: StaticCell<[u8; 256]> = StaticCell::new([0; 256]);
static PALETTE_B: StaticCell<[u8; 256]> = StaticCell::new([0; 256]);
static GFX_CURSOR_ROW: StaticCell<usize> = StaticCell::new(0);
static GFX_CURSOR_COL: StaticCell<usize> = StaticCell::new(0);
static GFX_CONSOLE_X: StaticCell<i32> = StaticCell::new(GFX_CONSOLE_DEFAULT_X);
static GFX_CONSOLE_Y: StaticCell<i32> = StaticCell::new(GFX_CONSOLE_DEFAULT_Y);
static GFX_CONSOLE_VISIBLE: StaticCell<bool> = StaticCell::new(true);
static GFX_CONSOLE_BUFFER: StaticCell<[u16; GFX_CONSOLE_COLS * GFX_CONSOLE_ROWS]> =
    StaticCell::new([GFX_CONSOLE_EMPTY; GFX_CONSOLE_COLS * GFX_CONSOLE_ROWS]);
static KEYBOARD_SHIFT: StaticCell<bool> = StaticCell::new(false);
static KEYBOARD_EXTENDED: StaticCell<bool> = StaticCell::new(false);

#[derive(Clone, Copy)]
pub struct SupervisorProtections {
    pub smep: bool,
    pub smap: bool,
}

pub struct UserAccessGuard {
    smap: bool,
}

#[derive(Clone, Copy)]
pub struct HardwareSummary {
    pub acpi_revision: u8,
    pub acpi_xsdt: bool,
    pub idt: bool,
    pub syscall: bool,
    pub exception_guard: bool,
    pub local_apic: bool,
    pub ioapic: bool,
    pub apic_id: u8,
    pub ioapic_redirections: u8,
    pub clock_source: &'static str,
    pub invariant_tsc: bool,
    pub tsc_hz: u64,
    pub hpet_hz: u64,
    pub apic_timer_hz: u64,
    pub timer_hz: u32,
    pub timer_irq: bool,
    pub cpus_discovered: usize,
    pub cpus_attempted: usize,
    pub cpus_online: usize,
    pub per_cpu: bool,
    pub irq_lock: bool,
}

pub fn init(protocol: u64, information: u64) {
    unsafe {
        asm!("cli", "cld", options(nomem, nostack, preserves_flags));
    }
    initialize_fpu();
    init_serial();
    serial_write_byte(b'A');
    init_ps2_mouse();
    serial_write_byte(b'B');
    let graphics = load_boot_video_info(protocol, information);
    serial_write_byte(b'C');
    clear_screen();
    unsafe {
        (*GRAPHICS_MODE.get()) = graphics;
    }
    serial_write_byte(b'D');
}

pub fn initialize_fpu() {
    let mut cr0: u64;
    let mut cr4: u64;
    let default_mxcsr = 0x1f80u32;
    unsafe {
        asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
        cr0 = (cr0 | (1 << 1) | (1 << 5)) & !((1 << 2) | (1 << 3));
        asm!("mov cr0, {}", in(reg) cr0, options(nostack, preserves_flags));
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        cr4 |= (1 << 9) | (1 << 10);
        asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
        asm!("fninit", options(nomem, nostack));
        asm!("ldmxcsr [{}]", in(reg) &default_mxcsr, options(nostack));
    }
}

pub fn initialize_runtime() -> HardwareSummary {
    let acpi = acpi::discover();
    if acpi.numa_memory_count != 0 {
        let mut ranges = [crate::mm::frame::NumaRange {
            node: 0,
            start: 0,
            end: 0,
        }; crate::mm::frame::MAX_NUMA_NODES];
        for (target, source) in ranges
            .iter_mut()
            .zip(&acpi.numa_memory[..acpi.numa_memory_count])
        {
            *target = crate::mm::frame::NumaRange {
                node: source.node,
                start: source.start,
                end: source.end,
            };
        }
        let _ = crate::mm::frame::configure_topology(&ranges[..acpi.numa_memory_count]);
    }
    let interrupt = interrupts::init(acpi);
    let clock = time::init(acpi, interrupt.local_apic);
    let smp = smp::init(acpi);
    let before = interrupts::timer_ticks();
    interrupts::enable();
    let mut timer_irq = clock.timer_hz == 0;
    if clock.timer_hz != 0 {
        for _ in 0..20_000_000usize {
            if interrupts::timer_ticks().wrapping_sub(before) >= 2 {
                timer_irq = true;
                break;
            }
            core::hint::spin_loop();
        }
    }
    let _ = time::monotonic_nanoseconds();
    let irq_lock = crate::sync::irq_lock_self_test();
    HardwareSummary {
        acpi_revision: acpi.revision,
        acpi_xsdt: acpi.used_xsdt,
        idt: interrupt.idt,
        syscall: interrupt.syscall,
        exception_guard: interrupt.exception_guard,
        local_apic: interrupt.local_apic,
        ioapic: interrupt.ioapic,
        apic_id: interrupt.apic_id,
        ioapic_redirections: interrupt.ioapic_redirections,
        clock_source: clock.source,
        invariant_tsc: clock.invariant_tsc,
        tsc_hz: clock.tsc_hz,
        hpet_hz: clock.hpet_hz,
        apic_timer_hz: clock.apic_hz,
        timer_hz: clock.timer_hz,
        timer_irq,
        cpus_discovered: smp.discovered,
        cpus_attempted: smp.attempted,
        cpus_online: smp.online,
        per_cpu: smp.per_cpu,
        irq_lock,
    }
}

pub fn monotonic_time_ns() -> u64 {
    time::monotonic_nanoseconds()
}

pub fn wait_for_interrupt() {
    unsafe { asm!("hlt", options(nomem, nostack)) };
}

pub fn lock_order_allows(rank: u8) -> bool {
    smp::lock_order_allows(rank)
}

pub fn push_lock_rank(rank: u8) -> bool {
    smp::push_lock_rank(rank)
}

pub fn pop_lock_rank(rank: u8) -> bool {
    smp::pop_lock_rank(rank)
}

pub fn irq_disabled_enter() {
    smp::irq_disabled_enter();
}

pub fn irq_disabled_exit() -> bool {
    smp::irq_disabled_exit()
}

pub fn set_kernel_stack_top(stack_top: u64) {
    smp::set_kernel_stack_top(stack_top);
    interrupts::set_kernel_stack_top(stack_top)
}

pub fn current_cpu_id() -> u32 {
    smp::current().logical_id()
}

pub fn online_cpu_count() -> usize {
    smp::online_cpu_count()
}

pub fn irq_disable_depth() -> u32 {
    smp::current().irq_disable_depth()
}

pub fn enable_supervisor_protections() -> SupervisorProtections {
    let mut protections = SupervisorProtections {
        smep: false,
        smap: false,
    };
    let maximum_leaf = core::arch::x86_64::__cpuid(0).eax;
    if maximum_leaf < 7 {
        return protections;
    }

    let features = core::arch::x86_64::__cpuid_count(7, 0).ebx;
    let mut cr4: u64;
    unsafe {
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    }
    if features & (1 << 7) != 0 {
        cr4 |= 1 << 20;
        protections.smep = true;
    }
    if features & (1 << 20) != 0 {
        cr4 |= 1 << 21;
        protections.smap = true;
    }
    unsafe {
        asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
    }
    protections
}

pub fn begin_user_access() -> UserAccessGuard {
    let mut cr4: u64;
    unsafe {
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    }
    let smap = cr4 & (1 << 21) != 0;
    if smap {
        // Only explicit copy_to/from_user sections may bypass SMAP.
        unsafe { asm!("stac", options(nomem, nostack, preserves_flags)) };
    }
    UserAccessGuard { smap }
}

impl Drop for UserAccessGuard {
    fn drop(&mut self) {
        if self.smap {
            unsafe { asm!("clac", options(nomem, nostack, preserves_flags)) };
        }
    }
}

pub fn debug_byte(byte: u8) {
    serial_write_byte(byte);
}

fn load_boot_video_info(protocol: u64, information: u64) -> bool {
    if protocol == MULTIBOOT2_BOOTLOADER_MAGIC {
        if load_multiboot2_video_info(information) {
            unsafe { *VIDEO_SOURCE.get() = 2 };
            return true;
        }
        set_default_video_info();
        return false;
    }

    let info = unsafe { &*(BOOT_VIDEO_INFO_ADDR as *const BootVideoInfo) };
    if info.magic != BOOT_VIDEO_INFO_MAGIC || !matches!(info.bpp, 8 | 24 | 32) {
        set_default_video_info();
        return false;
    }

    unsafe {
        (*FRAMEBUFFER_ADDR.get()) =
            (PHYSICAL_MEMORY_OFFSET + u64::from(info.framebuffer_addr)) as usize;
        (*FRAMEBUFFER_WIDTH.get()) = (info.width as i32).clamp(1, MAX_FB_WIDTH as i32);
        (*FRAMEBUFFER_HEIGHT.get()) = (info.height as i32).clamp(1, MAX_FB_HEIGHT as i32);
        (*FRAMEBUFFER_PITCH.get()) = (info.pitch as usize).max((*FRAMEBUFFER_WIDTH.get()) as usize);
        (*FRAMEBUFFER_BPP.get()) = info.bpp;
        (*FRAMEBUFFER_RED_MASK_SIZE.get()) = info.red_mask_size;
        (*FRAMEBUFFER_RED_SHIFT.get()) = info.red_field_position;
        (*FRAMEBUFFER_GREEN_MASK_SIZE.get()) = info.green_mask_size;
        (*FRAMEBUFFER_GREEN_SHIFT.get()) = info.green_field_position;
        (*FRAMEBUFFER_BLUE_MASK_SIZE.get()) = info.blue_mask_size;
        (*FRAMEBUFFER_BLUE_SHIFT.get()) = info.blue_field_position;
        if (*FRAMEBUFFER_BPP.get()) == 8 {
            (*FRAMEBUFFER_PITCH.get()) =
                (*FRAMEBUFFER_PITCH.get()).max((*FRAMEBUFFER_WIDTH.get()) as usize);
        } else {
            (*FRAMEBUFFER_PITCH.get()) = (*FRAMEBUFFER_PITCH.get())
                .max(((*FRAMEBUFFER_WIDTH.get()) as usize) * framebuffer_bytes_per_pixel());
        }
        (*VIDEO_SOURCE.get()) = 1;
    }
    true
}

fn load_multiboot2_video_info(information: u64) -> bool {
    let Ok(base) = usize::try_from(information) else {
        return false;
    };
    if base == 0 || base & 7 != 0 {
        return false;
    }
    let total_size = unsafe { core::ptr::read_unaligned(base as *const u32) as usize };
    if !(16..=MAX_MULTIBOOT_INFO_SIZE).contains(&total_size) {
        return false;
    }

    let mut offset = 8usize;
    while offset.checked_add(8).is_some_and(|end| end <= total_size) {
        let Some(tag_address) = base.checked_add(offset) else {
            return false;
        };
        let kind = unsafe { core::ptr::read_unaligned(tag_address as *const u32) };
        let size = unsafe { core::ptr::read_unaligned((tag_address + 4) as *const u32) as usize };
        if size < 8 || offset.checked_add(size).is_none_or(|end| end > total_size) {
            return false;
        }
        if kind == 0 {
            break;
        }
        if kind == MULTIBOOT2_FRAMEBUFFER_TAG && size >= 38 {
            let address = unsafe { core::ptr::read_unaligned((tag_address + 8) as *const u64) };
            let pitch = unsafe { core::ptr::read_unaligned((tag_address + 16) as *const u32) };
            let width = unsafe { core::ptr::read_unaligned((tag_address + 20) as *const u32) };
            let height = unsafe { core::ptr::read_unaligned((tag_address + 24) as *const u32) };
            let bpp = unsafe { core::ptr::read_unaligned((tag_address + 28) as *const u8) };
            let framebuffer_type =
                unsafe { core::ptr::read_unaligned((tag_address + 29) as *const u8) };
            let byte_len = u64::from(pitch).checked_mul(u64::from(height));
            if framebuffer_type == MULTIBOOT2_FRAMEBUFFER_RGB
                && matches!(bpp, 24 | 32)
                && width != 0
                && height != 0
                && pitch >= width.saturating_mul(u32::from(bpp) / 8)
                && byte_len.and_then(|length| address.checked_add(length)) <= Some(1u64 << 32)
            {
                unsafe {
                    (*FRAMEBUFFER_ADDR.get()) = (PHYSICAL_MEMORY_OFFSET + address) as usize;
                    (*FRAMEBUFFER_WIDTH.get()) = (width as i32).clamp(1, MAX_FB_WIDTH as i32);
                    (*FRAMEBUFFER_HEIGHT.get()) = (height as i32).clamp(1, MAX_FB_HEIGHT as i32);
                    (*FRAMEBUFFER_PITCH.get()) = pitch as usize;
                    (*FRAMEBUFFER_BPP.get()) = bpp;
                    (*FRAMEBUFFER_RED_SHIFT.get()) =
                        core::ptr::read_unaligned((tag_address + 32) as *const u8);
                    (*FRAMEBUFFER_RED_MASK_SIZE.get()) =
                        core::ptr::read_unaligned((tag_address + 33) as *const u8);
                    (*FRAMEBUFFER_GREEN_SHIFT.get()) =
                        core::ptr::read_unaligned((tag_address + 34) as *const u8);
                    (*FRAMEBUFFER_GREEN_MASK_SIZE.get()) =
                        core::ptr::read_unaligned((tag_address + 35) as *const u8);
                    (*FRAMEBUFFER_BLUE_SHIFT.get()) =
                        core::ptr::read_unaligned((tag_address + 36) as *const u8);
                    (*FRAMEBUFFER_BLUE_MASK_SIZE.get()) =
                        core::ptr::read_unaligned((tag_address + 37) as *const u8);
                }
                return true;
            }
        }
        let Some(next) = offset
            .checked_add(size)
            .and_then(|value| value.checked_add(7))
        else {
            return false;
        };
        offset = next & !7;
    }
    false
}

fn set_default_video_info() {
    unsafe {
        (*FRAMEBUFFER_ADDR.get()) = DEFAULT_FB_ADDR;
        (*FRAMEBUFFER_WIDTH.get()) = DEFAULT_FB_WIDTH;
        (*FRAMEBUFFER_HEIGHT.get()) = DEFAULT_FB_HEIGHT;
        (*FRAMEBUFFER_PITCH.get()) = DEFAULT_FB_PITCH;
        (*FRAMEBUFFER_BPP.get()) = DEFAULT_FB_BPP;
        (*FRAMEBUFFER_RED_MASK_SIZE.get()) = 0;
        (*FRAMEBUFFER_RED_SHIFT.get()) = 16;
        (*FRAMEBUFFER_GREEN_MASK_SIZE.get()) = 0;
        (*FRAMEBUFFER_GREEN_SHIFT.get()) = 8;
        (*FRAMEBUFFER_BLUE_MASK_SIZE.get()) = 0;
        (*FRAMEBUFFER_BLUE_SHIFT.get()) = 0;
        (*VIDEO_SOURCE.get()) = 0;
    }
}

pub fn console_write(text: &str) {
    for ch in text.chars() {
        match ch {
            '\n' => {
                serial_write_byte(b'\r');
                serial_write_byte(b'\n');
                gfx_console_newline();
                unsafe {
                    (*CURSOR_ROW.get()) += 1;
                    (*CURSOR_COL.get()) = 0;
                }
                scroll_if_needed();
            }
            '\r' => unsafe {
                (*CURSOR_COL.get()) = 0;
                (*GFX_CURSOR_COL.get()) = 0;
            },
            '\u{8}' => unsafe {
                if (*CURSOR_COL.get()) > 0 {
                    (*CURSOR_COL.get()) -= 1;
                } else if (*CURSOR_ROW.get()) > 0 {
                    (*CURSOR_ROW.get()) -= 1;
                    (*CURSOR_COL.get()) = VGA_WIDTH - 1;
                }

                let pos = ((*CURSOR_ROW.get()) * VGA_WIDTH + (*CURSOR_COL.get())) * 2;
                *VGA_BUFFER.add(pos) = b' ';
                *VGA_BUFFER.add(pos + 1) = 0x07;

                serial_write_byte(0x08);
                serial_write_byte(b' ');
                serial_write_byte(0x08);

                gfx_console_backspace();
            },
            _ => {
                let mut utf8 = [0u8; 4];
                for &byte in ch.encode_utf8(&mut utf8).as_bytes() {
                    serial_write_byte(byte);
                }
                gfx_console_write_char(ch);
                unsafe {
                    let pos = ((*CURSOR_ROW.get()) * VGA_WIDTH + (*CURSOR_COL.get())) * 2;
                    *VGA_BUFFER.add(pos) = if ch.is_ascii() { ch as u8 } else { b'?' };
                    *VGA_BUFFER.add(pos + 1) = 0x0F;

                    (*CURSOR_COL.get()) += 1;
                    if (*CURSOR_COL.get()) >= VGA_WIDTH {
                        (*CURSOR_COL.get()) = 0;
                        (*CURSOR_ROW.get()) += 1;
                        scroll_if_needed();
                    }
                }
            }
        }
    }
}

pub fn poll_console_input() -> Option<ConsoleInput> {
    if let Some(byte) = serial_read_byte() {
        return Some(ConsoleInput::Byte(if byte == b'\r' { b'\n' } else { byte }));
    }

    let input = read_controller_byte()?;
    if input.is_mouse {
        return Some(ConsoleInput::MouseByte(input.value));
    }

    let scancode = input.value;
    unsafe {
        if scancode == 0xE0 {
            (*KEYBOARD_EXTENDED.get()) = true;
            return None;
        }

        let released = scancode & 0x80 != 0;
        let code = scancode & 0x7F;

        if code == 0x2A || code == 0x36 {
            (*KEYBOARD_SHIFT.get()) = !released;
            (*KEYBOARD_EXTENDED.get()) = false;
            return None;
        }

        if released {
            (*KEYBOARD_EXTENDED.get()) = false;
            return None;
        }

        let translated = if *KEYBOARD_EXTENDED.get() {
            None
        } else {
            scancode_to_ascii(code, *KEYBOARD_SHIFT.get())
        };
        (*KEYBOARD_EXTENDED.get()) = false;
        Some(ConsoleInput::Key { code, translated })
    }
}

pub fn clear_screen() {
    for row in 0..VGA_HEIGHT {
        for col in 0..VGA_WIDTH {
            let pos = (row * VGA_WIDTH + col) * 2;
            unsafe {
                *VGA_BUFFER.add(pos) = b' ';
                *VGA_BUFFER.add(pos + 1) = 0x07;
            }
        }
    }

    unsafe {
        (*CURSOR_ROW.get()) = 0;
        (*CURSOR_COL.get()) = 0;
    }
}

pub fn graphics_mode_enabled() -> bool {
    unsafe { *GRAPHICS_MODE.get() }
}

pub fn screen_width() -> i32 {
    unsafe { *FRAMEBUFFER_WIDTH.get() }
}

pub fn screen_height() -> i32 {
    unsafe { *FRAMEBUFFER_HEIGHT.get() }
}

pub fn screen_bpp() -> u8 {
    unsafe { *FRAMEBUFFER_BPP.get() }
}

pub fn video_source() -> &'static str {
    match unsafe { *VIDEO_SOURCE.get() } {
        1 => "BIOS/VBE",
        2 => "Multiboot2",
        _ => "none",
    }
}

pub fn gfx_clear(color: u8) {
    if !graphics_mode_enabled() {
        return;
    }

    let color = palette_color_u32(color);
    unsafe {
        let draw = core::ptr::addr_of_mut!((*DRAW_BUFFER.get())) as *mut u32;
        let len = framebuffer_draw_len().min(MAX_FB_PIXELS);
        for index in 0..len {
            *draw.add(index) = color;
        }
    }
}

pub fn gfx_fill_rect(x: i32, y: i32, width: i32, height: i32, color: u8) {
    if !graphics_mode_enabled() || width <= 0 || height <= 0 {
        return;
    }

    let fb_width = screen_width();
    let fb_height = screen_height();
    let x0 = x.clamp(0, fb_width);
    let y0 = y.clamp(0, fb_height);
    let x1 = (x + width).clamp(0, fb_width);
    let y1 = (y + height).clamp(0, fb_height);
    let color = palette_color_u32(color);

    for py in y0..y1 {
        for px in x0..x1 {
            put_pixel(px, py, color);
        }
    }
}

pub fn gfx_fill_rect_alpha(x: i32, y: i32, width: i32, height: i32, color: u8, alpha: u8) {
    if !graphics_mode_enabled() || width <= 0 || height <= 0 || alpha == 0 {
        return;
    }
    if alpha == u8::MAX {
        gfx_fill_rect(x, y, width, height, color);
        return;
    }

    let fb_width = screen_width();
    let fb_height = screen_height();
    let x0 = x.clamp(0, fb_width);
    let y0 = y.clamp(0, fb_height);
    let x1 = (x + width).clamp(0, fb_width);
    let y1 = (y + height).clamp(0, fb_height);

    let color = palette_color_u32(color);
    for py in y0..y1 {
        for px in x0..x1 {
            blend_pixel(px, py, color, alpha);
        }
    }
}

pub fn gfx_blit_fullscreen(pixels: &[u8]) {
    if !graphics_mode_enabled() {
        return;
    }

    let fb_width = screen_width() as usize;
    let fb_height = screen_height() as usize;
    let expected = fb_width * fb_height;
    let count = pixels.len().min(expected);
    let draw_len = framebuffer_draw_len().min(MAX_FB_PIXELS);
    let draw = core::ptr::addr_of_mut!((*DRAW_BUFFER.get())) as *mut u32;

    for y in 0..fb_height {
        let src_row = y * fb_width;
        if src_row >= count {
            break;
        }
        let row_len = (count - src_row).min(fb_width);
        let dst_row = y * fb_width;
        if dst_row + row_len > draw_len {
            break;
        }
        for x in 0..row_len {
            unsafe {
                *draw.add(dst_row + x) = palette_color_u32(pixels[src_row + x]);
            }
        }
    }
}

pub fn gfx_present() {
    if !graphics_mode_enabled() {
        return;
    }

    let fb_width = screen_width() as usize;
    let fb_height = screen_height() as usize;
    let pitch = unsafe { *FRAMEBUFFER_PITCH.get() };
    let draw = core::ptr::addr_of!((*DRAW_BUFFER.get())) as *const u32;
    let framebuffer = unsafe { (*FRAMEBUFFER_ADDR.get()) as *mut u8 };
    let bpp = unsafe { *FRAMEBUFFER_BPP.get() };

    match bpp {
        8 => {
            for y in 0..fb_height {
                let src_row = y * fb_width;
                let dst_row = y * pitch;
                for x in 0..fb_width {
                    unsafe {
                        let rgb = *draw.add(src_row + x);
                        let (r, g, b) = unpack_rgb(rgb);
                        *framebuffer.add(dst_row + x) = nearest_palette_index(r, g, b);
                    }
                }
            }
        }
        24 => {
            for y in 0..fb_height {
                let src_row = y * fb_width;
                let dst_row = y * pitch;
                for x in 0..fb_width {
                    unsafe {
                        let packed = pack_native_pixel(*draw.add(src_row + x));
                        let dst = framebuffer.add(dst_row + x * 3);
                        *dst = (packed & 0xFF) as u8;
                        *dst.add(1) = ((packed >> 8) & 0xFF) as u8;
                        *dst.add(2) = ((packed >> 16) & 0xFF) as u8;
                    }
                }
            }
        }
        32 => {
            for y in 0..fb_height {
                let src_row = y * fb_width;
                let dst_row = y * pitch;
                for x in 0..fb_width {
                    unsafe {
                        let packed = pack_native_pixel(*draw.add(src_row + x));
                        let dst = framebuffer.add(dst_row + x * 4) as *mut u32;
                        *dst = packed;
                    }
                }
            }
        }
        _ => {}
    }
}

pub fn gfx_move_cursor(old_x: i32, old_y: i32, new_x: i32, new_y: i32, outline: u8, fill: u8) {
    if !graphics_mode_enabled() {
        return;
    }
    if old_x >= 0 && old_y >= 0 {
        gfx_present_rect(old_x, old_y, 10, 14);
    }

    framebuffer_fill_palette(new_x, new_y, 1, 14, outline);
    framebuffer_fill_palette(new_x, new_y, 10, 1, outline);
    framebuffer_fill_palette(new_x + 1, new_y + 1, 1, 12, fill);
    framebuffer_fill_palette(new_x + 1, new_y + 1, 8, 1, fill);
    framebuffer_fill_palette(new_x + 2, new_y + 2, 1, 10, fill);
    framebuffer_fill_palette(new_x + 3, new_y + 3, 1, 8, fill);
    framebuffer_fill_palette(new_x + 4, new_y + 4, 1, 6, fill);
    framebuffer_fill_palette(new_x + 5, new_y + 5, 1, 4, fill);
}

fn gfx_present_rect(x: i32, y: i32, width: i32, height: i32) {
    let x0 = x.clamp(0, screen_width());
    let y0 = y.clamp(0, screen_height());
    let x1 = (x + width).clamp(0, screen_width());
    let y1 = (y + height).clamp(0, screen_height());
    let draw = core::ptr::addr_of!((*DRAW_BUFFER.get())) as *const u32;
    let stride = screen_width() as usize;

    for py in y0..y1 {
        for px in x0..x1 {
            let rgb = unsafe { *draw.add(py as usize * stride + px as usize) };
            framebuffer_write_rgb(px, py, rgb);
        }
    }
}

fn framebuffer_fill_palette(x: i32, y: i32, width: i32, height: i32, color: u8) {
    for py in y..(y + height) {
        for px in x..(x + width) {
            framebuffer_write_palette(px, py, color);
        }
    }
}

fn framebuffer_write_palette(x: i32, y: i32, color: u8) {
    if x < 0 || y < 0 || x >= screen_width() || y >= screen_height() {
        return;
    }
    if unsafe { *FRAMEBUFFER_BPP.get() } == 8 {
        let pitch = unsafe { *FRAMEBUFFER_PITCH.get() };
        let framebuffer = unsafe { (*FRAMEBUFFER_ADDR.get()) as *mut u8 };
        unsafe { *framebuffer.add(y as usize * pitch + x as usize) = color };
    } else {
        framebuffer_write_rgb(x, y, palette_color_u32(color));
    }
}

fn framebuffer_write_rgb(x: i32, y: i32, rgb: u32) {
    if x < 0 || y < 0 || x >= screen_width() || y >= screen_height() {
        return;
    }
    let pitch = unsafe { *FRAMEBUFFER_PITCH.get() };
    let framebuffer = unsafe { (*FRAMEBUFFER_ADDR.get()) as *mut u8 };
    let row = y as usize * pitch;
    match unsafe { *FRAMEBUFFER_BPP.get() } {
        8 => unsafe {
            let (red, green, blue) = unpack_rgb(rgb);
            *framebuffer.add(row + x as usize) = nearest_palette_index(red, green, blue);
        },
        24 => unsafe {
            let packed = pack_native_pixel(rgb);
            let dst = framebuffer.add(row + x as usize * 3);
            *dst = (packed & 0xff) as u8;
            *dst.add(1) = ((packed >> 8) & 0xff) as u8;
            *dst.add(2) = ((packed >> 16) & 0xff) as u8;
        },
        32 => unsafe {
            let packed = pack_native_pixel(rgb);
            *(framebuffer.add(row + x as usize * 4) as *mut u32) = packed;
        },
        _ => {}
    }
}

pub fn gfx_fill_circle(cx: i32, cy: i32, radius: i32, color: u8) {
    if !graphics_mode_enabled() || radius <= 0 {
        return;
    }

    let fb_width = screen_width();
    let fb_height = screen_height();
    let left = (cx - radius).clamp(0, fb_width - 1);
    let right = (cx + radius).clamp(0, fb_width - 1);
    let top = (cy - radius).clamp(0, fb_height - 1);
    let bottom = (cy + radius).clamp(0, fb_height - 1);
    let radius_sq = radius * radius;
    let color = palette_color_u32(color);

    for py in top..=bottom {
        for px in left..=right {
            let dx = px - cx;
            let dy = py - cy;
            if dx * dx + dy * dy <= radius_sq {
                put_pixel(px, py, color);
            }
        }
    }
}

pub fn gfx_fill_circle_alpha(cx: i32, cy: i32, radius: i32, color: u8, alpha: u8) {
    if !graphics_mode_enabled() || radius <= 0 || alpha == 0 {
        return;
    }
    if alpha == u8::MAX {
        gfx_fill_circle(cx, cy, radius, color);
        return;
    }

    let fb_width = screen_width();
    let fb_height = screen_height();
    let left = (cx - radius).clamp(0, fb_width - 1);
    let right = (cx + radius).clamp(0, fb_width - 1);
    let top = (cy - radius).clamp(0, fb_height - 1);
    let bottom = (cy + radius).clamp(0, fb_height - 1);
    let radius_sq = radius * radius;
    let color = palette_color_u32(color);

    for py in top..=bottom {
        for px in left..=right {
            let dx = px - cx;
            let dy = py - cy;
            if dx * dx + dy * dy <= radius_sq {
                blend_pixel(px, py, color, alpha);
            }
        }
    }
}

pub fn gfx_fill_round_rect(x: i32, y: i32, width: i32, height: i32, radius: i32, color: u8) {
    if !graphics_mode_enabled() || width <= 0 || height <= 0 {
        return;
    }

    let radius = radius.min(width / 2).min(height / 2).max(0);
    let fb_width = screen_width();
    let fb_height = screen_height();
    let x0 = x.clamp(0, fb_width);
    let y0 = y.clamp(0, fb_height);
    let x1 = (x + width).clamp(0, fb_width);
    let y1 = (y + height).clamp(0, fb_height);
    let inner_left = x + radius;
    let inner_right = x + width - radius - 1;
    let inner_top = y + radius;
    let inner_bottom = y + height - radius - 1;
    let radius_sq = radius * radius;
    let color = palette_color_u32(color);

    for py in y0..y1 {
        for px in x0..x1 {
            if px >= inner_left && px <= inner_right {
                put_pixel(px, py, color);
                continue;
            }
            if py >= inner_top && py <= inner_bottom {
                put_pixel(px, py, color);
                continue;
            }

            let nearest_x = if inner_left <= inner_right {
                px.clamp(inner_left, inner_right)
            } else {
                x + width / 2
            };
            let nearest_y = if inner_top <= inner_bottom {
                py.clamp(inner_top, inner_bottom)
            } else {
                y + height / 2
            };
            let dx = px - nearest_x;
            let dy = py - nearest_y;
            if dx * dx + dy * dy <= radius_sq {
                put_pixel(px, py, color);
            }
        }
    }
}

pub fn gfx_fill_round_rect_alpha(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    radius: i32,
    color: u8,
    alpha: u8,
) {
    if !graphics_mode_enabled() || width <= 0 || height <= 0 || alpha == 0 {
        return;
    }
    if alpha == u8::MAX {
        gfx_fill_round_rect(x, y, width, height, radius, color);
        return;
    }

    let radius = radius.min(width / 2).min(height / 2).max(0);
    let fb_width = screen_width();
    let fb_height = screen_height();
    let x0 = x.clamp(0, fb_width);
    let y0 = y.clamp(0, fb_height);
    let x1 = (x + width).clamp(0, fb_width);
    let y1 = (y + height).clamp(0, fb_height);
    let inner_left = x + radius;
    let inner_right = x + width - radius - 1;
    let inner_top = y + radius;
    let inner_bottom = y + height - radius - 1;
    let radius_sq = radius * radius;
    let color = palette_color_u32(color);

    for py in y0..y1 {
        for px in x0..x1 {
            let inside = if px >= inner_left && px <= inner_right {
                true
            } else if py >= inner_top && py <= inner_bottom {
                true
            } else {
                let nearest_x = if inner_left <= inner_right {
                    px.clamp(inner_left, inner_right)
                } else {
                    x + width / 2
                };
                let nearest_y = if inner_top <= inner_bottom {
                    py.clamp(inner_top, inner_bottom)
                } else {
                    y + height / 2
                };
                let dx = px - nearest_x;
                let dy = py - nearest_y;
                dx * dx + dy * dy <= radius_sq
            };

            if inside {
                blend_pixel(px, py, color, alpha);
            }
        }
    }
}

pub fn draw_text(x: i32, y: i32, text: &str, color: u8, scale: i32) {
    if !graphics_mode_enabled() || scale <= 0 {
        return;
    }

    let mut cursor_x = x;
    for ch in text.chars() {
        if ch == '\n' {
            cursor_x = x;
            continue;
        }
        cursor_x += draw_unicode_glyph_large(cursor_x, y, ch, color, scale);
    }
}

pub fn text_width(text: &str, scale: i32) -> i32 {
    if scale <= 0 {
        return 0;
    }

    let mut width = 0;
    for ch in text.chars() {
        if ch == '\n' {
            break;
        }
        width += large_glyph_advance(ch, scale);
    }
    width
}

pub fn draw_text_compact(x: i32, y: i32, text: &str, color: u8, scale: i32) {
    if !graphics_mode_enabled() || scale <= 0 {
        return;
    }

    let mut cursor_x = x;
    for ch in text.chars() {
        if ch == '\n' {
            cursor_x = x;
            continue;
        }
        cursor_x += draw_unicode_glyph_compact(cursor_x, y, ch, color, scale);
    }
}

pub fn gfx_console_reset() {
    if !graphics_mode_enabled() {
        return;
    }

    unsafe {
        (*GFX_CURSOR_ROW.get()) = 0;
        (*GFX_CURSOR_COL.get()) = 0;
        let buffer = core::ptr::addr_of_mut!((*GFX_CONSOLE_BUFFER.get())) as *mut u16;
        for index in 0..(GFX_CONSOLE_COLS * GFX_CONSOLE_ROWS) {
            *buffer.add(index) = GFX_CONSOLE_EMPTY;
        }
    }

    gfx_console_redraw();
}

pub fn gfx_set_console_origin(x: i32, y: i32) {
    unsafe {
        (*GFX_CONSOLE_X.get()) = x;
        (*GFX_CONSOLE_Y.get()) = y;
    }
}

pub fn gfx_set_console_visible(visible: bool) {
    unsafe {
        (*GFX_CONSOLE_VISIBLE.get()) = visible;
    }
}

pub fn gfx_console_rows() -> usize {
    GFX_CONSOLE_ROWS
}

pub fn gfx_console_cols() -> usize {
    GFX_CONSOLE_COLS
}

pub fn gfx_console_line<const N: usize>(row: usize, out: &mut InlineString<N>) {
    out.clear();
    if row >= GFX_CONSOLE_ROWS || N == 0 {
        return;
    }

    for col in 0..GFX_CONSOLE_COLS {
        let ch = unsafe { (*GFX_CONSOLE_BUFFER.get())[row * GFX_CONSOLE_COLS + col] };
        if ch == GFX_CONSOLE_EMPTY || ch == GFX_CONSOLE_WIDE_CONT {
            let _ = out.push_byte(b' ');
            continue;
        }
        let Some(ch) = char::from_u32(ch as u32) else {
            let _ = out.push_byte(b'?');
            continue;
        };
        let mut utf8 = [0u8; 4];
        let text = ch.encode_utf8(&mut utf8);
        let _ = out.push_str(text);
    }

    while out.as_str().ends_with(' ') {
        let _ = out.pop();
    }
}

pub fn gfx_console_cursor() -> (usize, usize) {
    unsafe { ((*GFX_CURSOR_ROW.get()), (*GFX_CURSOR_COL.get())) }
}

pub fn gfx_console_cell(row: usize, col: usize) -> u16 {
    if row >= GFX_CONSOLE_ROWS || col >= GFX_CONSOLE_COLS {
        return GFX_CONSOLE_EMPTY;
    }
    unsafe { (*GFX_CONSOLE_BUFFER.get())[row * GFX_CONSOLE_COLS + col] }
}

pub fn gfx_console_redraw() {
    if !graphics_mode_enabled() {
        return;
    }

    if unsafe { !(*GFX_CONSOLE_VISIBLE.get()) } {
        return;
    }

    let (origin_x, origin_y) = unsafe { ((*GFX_CONSOLE_X.get()), (*GFX_CONSOLE_Y.get())) };
    gfx_fill_rect(
        origin_x,
        origin_y,
        GFX_CONSOLE_WIDTH,
        GFX_CONSOLE_HEIGHT,
        GFX_CONSOLE_BG,
    );

    for row in 0..GFX_CONSOLE_ROWS {
        for col in 0..GFX_CONSOLE_COLS {
            let ch = unsafe { (*GFX_CONSOLE_BUFFER.get())[row * GFX_CONSOLE_COLS + col] };
            if ch == GFX_CONSOLE_EMPTY || ch == GFX_CONSOLE_WIDE_CONT {
                continue;
            }
            let x = origin_x + (col as i32) * GFX_CHAR_WIDTH;
            let y = origin_y + (row as i32) * GFX_CHAR_HEIGHT;
            if let Some(ch) = char::from_u32(ch as u32) {
                let _ = draw_unicode_glyph_compact(x, y, ch, GFX_CONSOLE_FG, GFX_CHAR_SCALE);
            }
        }
    }
}

pub fn set_palette_entry(index: u8, red: u8, green: u8, blue: u8) {
    unsafe {
        (*PALETTE_R.get())[index as usize] = expand_6bit(red);
        (*PALETTE_G.get())[index as usize] = expand_6bit(green);
        (*PALETTE_B.get())[index as usize] = expand_6bit(blue);
    }
    io_out8(VGA_DAC_INDEX, index);
    io_out8(VGA_DAC_DATA, red & 0x3F);
    io_out8(VGA_DAC_DATA, green & 0x3F);
    io_out8(VGA_DAC_DATA, blue & 0x3F);
}

pub fn delay(cycles: u32) {
    for _ in 0..cycles {
        unsafe {
            asm!("nop", options(nomem, nostack, preserves_flags));
        }
    }
}

pub fn read_clock_time() -> Option<ClockTime> {
    let first = rtc_snapshot()?;
    let second = rtc_snapshot()?;
    let (hour, minute, second_value) = if first == second { first } else { second };

    Some(ClockTime {
        hour,
        minute,
        second: second_value,
    })
}

pub fn speaker_play(frequency_hz: u32) {
    if frequency_hz == 0 {
        speaker_stop();
        return;
    }

    let divisor = (1_193_180u32 / frequency_hz.max(1)).clamp(1, u16::MAX as u32) as u16;
    io_out8(0x43, 0xB6);
    io_out8(0x42, (divisor & 0x00FF) as u8);
    io_out8(0x42, (divisor >> 8) as u8);

    let state = unsafe { io_in8(0x61) };
    if state & 0x03 != 0x03 {
        io_out8(0x61, state | 0x03);
    }
}

pub fn speaker_stop() {
    let state = unsafe { io_in8(0x61) };
    io_out8(0x61, state & !0x03);
}

fn put_pixel(x: i32, y: i32, color: u32) {
    let fb_width = screen_width();
    let fb_height = screen_height();
    if x < 0 || y < 0 || x >= fb_width || y >= fb_height {
        return;
    }

    let offset = (y as usize) * (fb_width as usize) + (x as usize);
    unsafe {
        *core::ptr::addr_of_mut!((*DRAW_BUFFER.get()))
            .cast::<u32>()
            .add(offset) = color;
    }
}

fn framebuffer_draw_len() -> usize {
    (screen_width() as usize) * (screen_height() as usize)
}

fn blend_pixel(x: i32, y: i32, color: u32, alpha: u8) {
    let fb_width = screen_width();
    let fb_height = screen_height();
    if x < 0 || y < 0 || x >= fb_width || y >= fb_height {
        return;
    }

    let offset = (y as usize) * (fb_width as usize) + (x as usize);
    unsafe {
        let draw = core::ptr::addr_of_mut!((*DRAW_BUFFER.get())).cast::<u32>();
        let dst_rgb = *draw.add(offset);
        let (sr, sg, sb) = unpack_rgb(color);
        let (dr, dg, db) = unpack_rgb(dst_rgb);
        let mixed_r = blend_component(dr, sr, alpha);
        let mixed_g = blend_component(dg, sg, alpha);
        let mixed_b = blend_component(db, sb, alpha);
        *draw.add(offset) = pack_rgb(mixed_r, mixed_g, mixed_b);
    }
}

fn blend_component(dst: u8, src: u8, alpha: u8) -> u8 {
    let src = src as u16;
    let dst = dst as u16;
    let alpha = alpha as u16;
    (((dst * (255 - alpha)) + (src * alpha) + 127) / 255) as u8
}

fn palette_rgb(index: u8) -> (u8, u8, u8) {
    unsafe {
        (
            (*PALETTE_R.get())[index as usize],
            (*PALETTE_G.get())[index as usize],
            (*PALETTE_B.get())[index as usize],
        )
    }
}

fn palette_color_u32(index: u8) -> u32 {
    let (r, g, b) = palette_rgb(index);
    pack_rgb(r, g, b)
}

fn pack_rgb(red: u8, green: u8, blue: u8) -> u32 {
    ((red as u32) << 16) | ((green as u32) << 8) | (blue as u32)
}

fn unpack_rgb(value: u32) -> (u8, u8, u8) {
    (
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
    )
}

fn expand_6bit(value: u8) -> u8 {
    (((value & 0x3F) as u16) * 255 / 63) as u8
}

fn framebuffer_bytes_per_pixel() -> usize {
    match unsafe { *FRAMEBUFFER_BPP.get() } {
        24 => 3,
        32 => 4,
        _ => 1,
    }
}

fn pack_native_pixel(rgb: u32) -> u32 {
    let (red, green, blue) = unpack_rgb(rgb);
    let (r_mask, r_shift, g_mask, g_shift, b_mask, b_shift) = unsafe {
        (
            (*FRAMEBUFFER_RED_MASK_SIZE.get()),
            (*FRAMEBUFFER_RED_SHIFT.get()),
            (*FRAMEBUFFER_GREEN_MASK_SIZE.get()),
            (*FRAMEBUFFER_GREEN_SHIFT.get()),
            (*FRAMEBUFFER_BLUE_MASK_SIZE.get()),
            (*FRAMEBUFFER_BLUE_SHIFT.get()),
        )
    };

    let red = scale_channel(red, r_mask) << r_shift;
    let green = scale_channel(green, g_mask) << g_shift;
    let blue = scale_channel(blue, b_mask) << b_shift;
    red | green | blue
}

fn scale_channel(value: u8, bits: u8) -> u32 {
    if bits == 0 {
        return value as u32;
    }
    let max = (1u32 << bits.min(24)) - 1;
    ((value as u32) * max + 127) / 255
}

fn nearest_palette_index(red: u8, green: u8, blue: u8) -> u8 {
    let mut best_index = 0u8;
    let mut best_distance = u32::MAX;

    for index in 0..=u8::MAX {
        let (pr, pg, pb) = palette_rgb(index);
        let dr = pr as i32 - red as i32;
        let dg = pg as i32 - green as i32;
        let db = pb as i32 - blue as i32;
        let distance = (dr * dr + dg * dg + db * db) as u32;
        if distance < best_distance {
            best_distance = distance;
            best_index = index;
            if distance == 0 {
                break;
            }
        }
    }

    best_index
}

fn draw_ascii_glyph(x: i32, y: i32, ch: u8, color: u8, scale: i32) {
    let glyph = glyph_bitmap(normalize_glyph(ch));
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5 {
            if (bits >> (4 - col)) & 1 == 0 {
                continue;
            }
            let px = x + (col as i32) * scale;
            let py = y + (row as i32) * scale;
            gfx_fill_rect(px, py, scale, scale, color);
        }
    }
}

fn draw_bitmap_glyph8(x: i32, y: i32, glyph: [u8; 8], color: u8, scale: i32) {
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8 {
            if (bits >> (7 - col)) & 1 == 0 {
                continue;
            }
            let px = x + (col as i32) * scale;
            let py = y + (row as i32) * scale;
            gfx_fill_rect(px, py, scale, scale, color);
        }
    }
}

fn draw_bitmap_glyph16_from8(x: i32, y: i32, glyph: [u8; 8], color: u8, scale: i32) {
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..8 {
            if (bits >> (7 - col)) & 1 == 0 {
                continue;
            }
            let px = x + (col as i32) * scale * 2;
            let py = y + (row as i32) * scale * 2;
            gfx_fill_rect(px, py, scale * 2, scale * 2, color);
        }
    }
}

fn draw_bitmap_glyph16(x: i32, y: i32, glyph: [u16; 16], color: u8, scale: i32) {
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..16 {
            if (bits >> (15 - col)) & 1 == 0 {
                continue;
            }
            let px = x + (col as i32) * scale;
            let py = y + (row as i32) * scale;
            gfx_fill_rect(px, py, scale, scale, color);
        }
    }
}

fn large_glyph_advance(ch: char, scale: i32) -> i32 {
    if ch.is_ascii() {
        6 * scale
    } else if cjk_glyph16(ch).is_some() {
        16 * scale
    } else if has_stroke_glyph(ch) {
        14 * scale
    } else if cjk_glyph(ch).is_some() {
        14 * scale
    } else {
        6 * scale
    }
}

fn draw_unicode_glyph_large(x: i32, y: i32, ch: char, color: u8, scale: i32) -> i32 {
    if ch.is_ascii() {
        draw_ascii_glyph(x, y, ch as u8, color, scale);
        return 6 * scale;
    }

    if let Some(glyph) = cjk_glyph16(ch) {
        draw_bitmap_glyph16(x, y, glyph, color, scale);
        return 16 * scale;
    }

    if draw_cjk_stroke_glyph(x, y, ch, color, scale) {
        return 14 * scale;
    }

    if let Some(glyph) = cjk_glyph(ch) {
        draw_bitmap_glyph16_from8(x, y, glyph, color, scale);
        return 14 * scale;
    }

    draw_ascii_glyph(x, y, b'?', color, scale);
    6 * scale
}

fn draw_unicode_glyph_compact(x: i32, y: i32, ch: char, color: u8, scale: i32) -> i32 {
    if ch.is_ascii() {
        draw_ascii_glyph(x, y, ch as u8, color, scale);
        return 6 * scale;
    }

    if let Some(glyph) = cjk_glyph(ch) {
        draw_bitmap_glyph8(x, y, glyph, color, scale);
        return 10 * scale;
    }

    draw_ascii_glyph(x, y, b'?', color, scale);
    6 * scale
}

fn gfx_console_write_char(ch: char) {
    if !graphics_mode_enabled() {
        return;
    }

    let width = if cjk_glyph(ch).is_some() { 2 } else { 1 };
    unsafe {
        if (*GFX_CURSOR_COL.get()) + width > GFX_CONSOLE_COLS {
            (*GFX_CURSOR_COL.get()) = 0;
            (*GFX_CURSOR_ROW.get()) += 1;
        }
    }
    gfx_console_scroll_if_needed();

    let (col, row) = unsafe { ((*GFX_CURSOR_COL.get()), (*GFX_CURSOR_ROW.get())) };
    let (origin_x, origin_y) = unsafe { ((*GFX_CONSOLE_X.get()), (*GFX_CONSOLE_Y.get())) };
    let x = origin_x + (col as i32) * GFX_CHAR_WIDTH;
    let y = origin_y + (row as i32) * GFX_CHAR_HEIGHT;

    unsafe {
        (*GFX_CONSOLE_BUFFER.get())[row * GFX_CONSOLE_COLS + col] = ch as u16;
        if width == 2 && col + 1 < GFX_CONSOLE_COLS {
            (*GFX_CONSOLE_BUFFER.get())[row * GFX_CONSOLE_COLS + col + 1] = GFX_CONSOLE_WIDE_CONT;
        }
    }

    if unsafe { *GFX_CONSOLE_VISIBLE.get() } {
        gfx_fill_rect(
            x,
            y,
            GFX_CHAR_WIDTH * width as i32,
            GFX_CHAR_HEIGHT,
            GFX_CONSOLE_BG,
        );
        let _ = draw_unicode_glyph_compact(x, y, ch, GFX_CONSOLE_FG, GFX_CHAR_SCALE);
    }

    unsafe {
        (*GFX_CURSOR_COL.get()) += width;
    }
}

fn gfx_console_newline() {
    if !graphics_mode_enabled() {
        return;
    }

    unsafe {
        (*GFX_CURSOR_COL.get()) = 0;
        (*GFX_CURSOR_ROW.get()) += 1;
    }
    gfx_console_scroll_if_needed();
}

fn gfx_console_backspace() {
    if !graphics_mode_enabled() {
        return;
    }

    unsafe {
        if (*GFX_CURSOR_COL.get()) > 0 {
            (*GFX_CURSOR_COL.get()) -= 1;
        } else if (*GFX_CURSOR_ROW.get()) > 0 {
            (*GFX_CURSOR_ROW.get()) -= 1;
            (*GFX_CURSOR_COL.get()) = GFX_CONSOLE_COLS.saturating_sub(1);
        } else {
            return;
        }

        let cell = (*GFX_CONSOLE_BUFFER.get())
            [(*GFX_CURSOR_ROW.get()) * GFX_CONSOLE_COLS + (*GFX_CURSOR_COL.get())];
        if cell == GFX_CONSOLE_WIDE_CONT && (*GFX_CURSOR_COL.get()) > 0 {
            (*GFX_CURSOR_COL.get()) -= 1;
        }

        let base = (*GFX_CURSOR_ROW.get()) * GFX_CONSOLE_COLS + (*GFX_CURSOR_COL.get());
        (*GFX_CONSOLE_BUFFER.get())[base] = GFX_CONSOLE_EMPTY;
        if (*GFX_CURSOR_COL.get()) + 1 < GFX_CONSOLE_COLS
            && (*GFX_CONSOLE_BUFFER.get())[base + 1] == GFX_CONSOLE_WIDE_CONT
        {
            (*GFX_CONSOLE_BUFFER.get())[base + 1] = GFX_CONSOLE_EMPTY;
        }

        if *GFX_CONSOLE_VISIBLE.get() {
            let x = (*GFX_CONSOLE_X.get()) + ((*GFX_CURSOR_COL.get()) as i32) * GFX_CHAR_WIDTH;
            let y = (*GFX_CONSOLE_Y.get()) + ((*GFX_CURSOR_ROW.get()) as i32) * GFX_CHAR_HEIGHT;
            gfx_fill_rect(x, y, GFX_CHAR_WIDTH * 2, GFX_CHAR_HEIGHT, GFX_CONSOLE_BG);
        }
    }
}

fn gfx_console_scroll_if_needed() {
    unsafe {
        if (*GFX_CURSOR_ROW.get()) < GFX_CONSOLE_ROWS {
            return;
        }
    }

    unsafe {
        for row in 1..GFX_CONSOLE_ROWS {
            for col in 0..GFX_CONSOLE_COLS {
                let src = row * GFX_CONSOLE_COLS + col;
                let dst = (row - 1) * GFX_CONSOLE_COLS + col;
                (*GFX_CONSOLE_BUFFER.get())[dst] = (*GFX_CONSOLE_BUFFER.get())[src];
            }
        }
        let base = (GFX_CONSOLE_ROWS - 1) * GFX_CONSOLE_COLS;
        for col in 0..GFX_CONSOLE_COLS {
            (*GFX_CONSOLE_BUFFER.get())[base + col] = GFX_CONSOLE_EMPTY;
        }
        (*GFX_CURSOR_ROW.get()) = GFX_CONSOLE_ROWS - 1;
    }

    gfx_console_redraw();
}

fn stroke_h(x: i32, y: i32, left: i32, right: i32, top: i32, thick: i32, color: u8, scale: i32) {
    let actual = ((thick * scale) / 2).max(1);
    gfx_fill_rect(
        x + left * scale,
        y + top * scale,
        (right - left + 1) * scale,
        actual,
        color,
    );
}

fn stroke_v(x: i32, y: i32, col: i32, top: i32, bottom: i32, thick: i32, color: u8, scale: i32) {
    let actual = ((thick * scale) / 2).max(1);
    gfx_fill_rect(
        x + col * scale,
        y + top * scale,
        actual,
        (bottom - top + 1) * scale,
        color,
    );
}

fn stroke_rect(
    x: i32,
    y: i32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    thick: i32,
    color: u8,
    scale: i32,
) {
    stroke_h(x, y, left, right, top, thick, color, scale);
    stroke_h(x, y, left, right, bottom - thick + 1, thick, color, scale);
    stroke_v(x, y, left, top, bottom, thick, color, scale);
    stroke_v(x, y, right - thick + 1, top, bottom, thick, color, scale);
}

fn stroke_diag(
    x: i32,
    y: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    thick: i32,
    color: u8,
    scale: i32,
) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let steps = dx.abs().max(dy.abs()).max(1);
    let actual = ((thick * scale) / 2).max(1);
    for step in 0..=steps {
        let px = x0 + dx * step / steps;
        let py = y0 + dy * step / steps;
        gfx_fill_rect(x + px * scale, y + py * scale, actual, actual, color);
    }
}

fn stroke_dot(x: i32, y: i32, col: i32, row: i32, size: i32, color: u8, scale: i32) {
    let actual = ((size * scale) / 2).max(1);
    gfx_fill_rect(x + col * scale, y + row * scale, actual, actual, color);
}

fn has_stroke_glyph(ch: char) -> bool {
    matches!(
        ch,
        '上' | '下'
            | '主'
            | '亮'
            | '文'
            | '件'
            | '命'
            | '令'
            | '保'
            | '编'
            | '辑'
            | '设'
            | '置'
            | '视'
            | '频'
            | '音'
            | '量'
            | '开'
            | '始'
            | '提'
            | '示'
            | '符'
            | '目'
            | '录'
            | '类'
            | '型'
            | '存'
            | '播'
            | '放'
            | '器'
            | '题'
            | '度'
    )
}

fn draw_cjk_stroke_glyph(x: i32, y: i32, ch: char, color: u8, scale: i32) -> bool {
    if scale <= 0 {
        return false;
    }

    match ch {
        '上' => {
            stroke_v(x, y, 6, 2, 11, 2, color, scale);
            stroke_h(x, y, 1, 12, 11, 2, color, scale);
        }
        '下' => {
            stroke_h(x, y, 1, 12, 3, 2, color, scale);
            stroke_v(x, y, 6, 3, 11, 2, color, scale);
            stroke_diag(x, y, 7, 11, 4, 13, 2, color, scale);
        }
        '主' => {
            stroke_h(x, y, 5, 8, 1, 2, color, scale);
            stroke_v(x, y, 6, 1, 12, 2, color, scale);
            stroke_h(x, y, 1, 12, 6, 2, color, scale);
            stroke_h(x, y, 1, 12, 12, 2, color, scale);
        }
        '亮' => {
            stroke_h(x, y, 1, 12, 1, 2, color, scale);
            stroke_h(x, y, 2, 11, 4, 2, color, scale);
            stroke_rect(x, y, 3, 6, 10, 9, 2, color, scale);
            stroke_h(x, y, 2, 11, 12, 2, color, scale);
            stroke_v(x, y, 3, 12, 14, 2, color, scale);
            stroke_v(x, y, 9, 12, 14, 2, color, scale);
        }
        '文' => {
            stroke_h(x, y, 3, 10, 1, 2, color, scale);
            stroke_v(x, y, 6, 2, 4, 2, color, scale);
            stroke_diag(x, y, 6, 5, 2, 13, 2, color, scale);
            stroke_diag(x, y, 7, 5, 11, 13, 2, color, scale);
        }
        '件' => {
            stroke_diag(x, y, 3, 1, 1, 5, 2, color, scale);
            stroke_v(x, y, 2, 5, 13, 2, color, scale);
            stroke_h(x, y, 6, 12, 2, 2, color, scale);
            stroke_h(x, y, 5, 12, 6, 2, color, scale);
            stroke_v(x, y, 8, 1, 13, 2, color, scale);
            stroke_h(x, y, 6, 12, 10, 2, color, scale);
        }
        '命' => {
            stroke_diag(x, y, 6, 1, 3, 4, 2, color, scale);
            stroke_diag(x, y, 7, 1, 10, 4, 2, color, scale);
            stroke_h(x, y, 3, 10, 5, 2, color, scale);
            stroke_rect(x, y, 3, 7, 10, 10, 2, color, scale);
            stroke_v(x, y, 6, 10, 14, 2, color, scale);
        }
        '令' => {
            stroke_diag(x, y, 6, 1, 3, 4, 2, color, scale);
            stroke_diag(x, y, 7, 1, 10, 4, 2, color, scale);
            stroke_h(x, y, 3, 10, 5, 2, color, scale);
            stroke_v(x, y, 6, 5, 8, 2, color, scale);
            stroke_diag(x, y, 6, 9, 4, 13, 2, color, scale);
            stroke_diag(x, y, 6, 9, 9, 13, 2, color, scale);
        }
        '保' => {
            stroke_diag(x, y, 3, 1, 1, 5, 2, color, scale);
            stroke_v(x, y, 2, 5, 13, 2, color, scale);
            stroke_rect(x, y, 6, 2, 11, 5, 2, color, scale);
            stroke_rect(x, y, 6, 7, 11, 10, 2, color, scale);
            stroke_v(x, y, 8, 10, 13, 2, color, scale);
        }
        '编' => {
            stroke_h(x, y, 1, 4, 2, 2, color, scale);
            stroke_h(x, y, 1, 4, 6, 2, color, scale);
            stroke_h(x, y, 1, 4, 10, 2, color, scale);
            stroke_v(x, y, 2, 2, 12, 2, color, scale);
            stroke_rect(x, y, 6, 2, 12, 5, 2, color, scale);
            stroke_rect(x, y, 6, 8, 12, 12, 2, color, scale);
            stroke_v(x, y, 8, 2, 12, 2, color, scale);
        }
        '辑' => {
            stroke_h(x, y, 1, 5, 2, 2, color, scale);
            stroke_h(x, y, 1, 5, 7, 2, color, scale);
            stroke_h(x, y, 1, 5, 12, 2, color, scale);
            stroke_v(x, y, 2, 1, 13, 2, color, scale);
            stroke_h(x, y, 8, 12, 2, 2, color, scale);
            stroke_rect(x, y, 8, 7, 12, 11, 2, color, scale);
            stroke_v(x, y, 9, 2, 13, 2, color, scale);
        }
        '设' => {
            stroke_dot(x, y, 1, 2, 2, color, scale);
            stroke_dot(x, y, 2, 5, 2, color, scale);
            stroke_diag(x, y, 2, 8, 1, 13, 2, color, scale);
            stroke_h(x, y, 7, 12, 2, 2, color, scale);
            stroke_diag(x, y, 7, 3, 10, 7, 2, color, scale);
            stroke_diag(x, y, 10, 7, 7, 13, 2, color, scale);
            stroke_diag(x, y, 10, 7, 12, 13, 2, color, scale);
        }
        '置' => {
            stroke_rect(x, y, 2, 1, 11, 5, 2, color, scale);
            stroke_h(x, y, 2, 11, 8, 2, color, scale);
            stroke_v(x, y, 6, 5, 13, 2, color, scale);
            stroke_h(x, y, 1, 12, 12, 2, color, scale);
        }
        '视' => {
            stroke_dot(x, y, 1, 2, 2, color, scale);
            stroke_v(x, y, 2, 5, 12, 2, color, scale);
            stroke_h(x, y, 1, 5, 7, 2, color, scale);
            stroke_rect(x, y, 7, 2, 12, 6, 2, color, scale);
            stroke_v(x, y, 8, 6, 12, 2, color, scale);
            stroke_diag(x, y, 8, 9, 5, 13, 2, color, scale);
            stroke_diag(x, y, 9, 9, 12, 13, 2, color, scale);
        }
        '频' => {
            stroke_h(x, y, 1, 4, 2, 2, color, scale);
            stroke_v(x, y, 2, 2, 9, 2, color, scale);
            stroke_h(x, y, 1, 4, 9, 2, color, scale);
            stroke_dot(x, y, 4, 12, 2, color, scale);
            stroke_rect(x, y, 7, 2, 12, 7, 2, color, scale);
            stroke_h(x, y, 7, 11, 10, 2, color, scale);
            stroke_diag(x, y, 8, 10, 6, 13, 2, color, scale);
            stroke_diag(x, y, 9, 10, 12, 13, 2, color, scale);
        }
        '音' => {
            stroke_h(x, y, 2, 11, 1, 2, color, scale);
            stroke_h(x, y, 3, 10, 4, 2, color, scale);
            stroke_v(x, y, 6, 1, 4, 2, color, scale);
            stroke_rect(x, y, 2, 7, 11, 13, 2, color, scale);
        }
        '量' => {
            stroke_rect(x, y, 2, 1, 11, 5, 2, color, scale);
            stroke_h(x, y, 1, 12, 7, 2, color, scale);
            stroke_v(x, y, 2, 7, 13, 2, color, scale);
            stroke_v(x, y, 6, 7, 13, 2, color, scale);
            stroke_v(x, y, 10, 7, 13, 2, color, scale);
            stroke_h(x, y, 1, 12, 12, 2, color, scale);
        }
        '开' => {
            stroke_v(x, y, 3, 1, 13, 2, color, scale);
            stroke_v(x, y, 9, 1, 13, 2, color, scale);
            stroke_h(x, y, 1, 12, 6, 2, color, scale);
        }
        '始' => {
            stroke_diag(x, y, 3, 2, 1, 6, 2, color, scale);
            stroke_h(x, y, 1, 5, 7, 2, color, scale);
            stroke_v(x, y, 3, 5, 13, 2, color, scale);
            stroke_rect(x, y, 7, 3, 12, 7, 2, color, scale);
            stroke_v(x, y, 8, 7, 13, 2, color, scale);
            stroke_h(x, y, 7, 12, 12, 2, color, scale);
        }
        '提' => {
            stroke_h(x, y, 1, 5, 3, 2, color, scale);
            stroke_h(x, y, 1, 5, 7, 2, color, scale);
            stroke_v(x, y, 2, 1, 13, 2, color, scale);
            stroke_h(x, y, 7, 12, 2, 2, color, scale);
            stroke_rect(x, y, 7, 5, 12, 9, 2, color, scale);
            stroke_v(x, y, 8, 9, 13, 2, color, scale);
            stroke_h(x, y, 7, 12, 12, 2, color, scale);
        }
        '示' => {
            stroke_h(x, y, 2, 11, 2, 2, color, scale);
            stroke_h(x, y, 1, 12, 5, 2, color, scale);
            stroke_v(x, y, 6, 2, 9, 2, color, scale);
            stroke_diag(x, y, 6, 9, 3, 13, 2, color, scale);
            stroke_diag(x, y, 7, 9, 10, 13, 2, color, scale);
        }
        '符' => {
            stroke_diag(x, y, 3, 1, 1, 4, 2, color, scale);
            stroke_diag(x, y, 6, 1, 8, 4, 2, color, scale);
            stroke_diag(x, y, 8, 1, 6, 4, 2, color, scale);
            stroke_diag(x, y, 11, 1, 13, 4, 2, color, scale);
            stroke_v(x, y, 6, 6, 13, 2, color, scale);
            stroke_h(x, y, 7, 12, 6, 2, color, scale);
            stroke_diag(x, y, 8, 8, 12, 13, 2, color, scale);
        }
        '目' => {
            stroke_rect(x, y, 2, 1, 11, 13, 2, color, scale);
            stroke_h(x, y, 3, 10, 5, 2, color, scale);
            stroke_h(x, y, 3, 10, 9, 2, color, scale);
        }
        '录' => {
            stroke_h(x, y, 2, 11, 1, 2, color, scale);
            stroke_diag(x, y, 2, 3, 6, 7, 2, color, scale);
            stroke_diag(x, y, 11, 3, 7, 7, 2, color, scale);
            stroke_v(x, y, 6, 7, 10, 2, color, scale);
            stroke_diag(x, y, 6, 10, 3, 13, 2, color, scale);
            stroke_diag(x, y, 7, 10, 10, 13, 2, color, scale);
        }
        '类' => {
            stroke_v(x, y, 6, 1, 8, 2, color, scale);
            stroke_h(x, y, 2, 10, 4, 2, color, scale);
            stroke_diag(x, y, 3, 1, 6, 4, 2, color, scale);
            stroke_diag(x, y, 9, 1, 6, 4, 2, color, scale);
            stroke_diag(x, y, 6, 8, 2, 13, 2, color, scale);
            stroke_diag(x, y, 7, 8, 11, 13, 2, color, scale);
        }
        '型' => {
            stroke_v(x, y, 2, 1, 8, 2, color, scale);
            stroke_h(x, y, 1, 4, 4, 2, color, scale);
            stroke_v(x, y, 8, 1, 8, 2, color, scale);
            stroke_h(x, y, 7, 10, 4, 2, color, scale);
            stroke_h(x, y, 1, 12, 10, 2, color, scale);
            stroke_v(x, y, 6, 10, 13, 2, color, scale);
            stroke_h(x, y, 1, 12, 12, 2, color, scale);
        }
        '存' => {
            stroke_h(x, y, 1, 12, 2, 2, color, scale);
            stroke_diag(x, y, 3, 2, 1, 6, 2, color, scale);
            stroke_v(x, y, 6, 2, 13, 2, color, scale);
            stroke_h(x, y, 3, 9, 7, 2, color, scale);
            stroke_diag(x, y, 6, 7, 10, 13, 2, color, scale);
        }
        '播' => {
            stroke_h(x, y, 1, 5, 3, 2, color, scale);
            stroke_h(x, y, 1, 5, 7, 2, color, scale);
            stroke_v(x, y, 2, 1, 13, 2, color, scale);
            stroke_h(x, y, 7, 12, 2, 2, color, scale);
            stroke_h(x, y, 7, 12, 6, 2, color, scale);
            stroke_h(x, y, 7, 12, 9, 2, color, scale);
            stroke_h(x, y, 7, 12, 12, 2, color, scale);
            stroke_v(x, y, 8, 2, 13, 2, color, scale);
        }
        '放' => {
            stroke_h(x, y, 1, 5, 2, 2, color, scale);
            stroke_v(x, y, 2, 2, 13, 2, color, scale);
            stroke_diag(x, y, 3, 7, 5, 10, 2, color, scale);
            stroke_diag(x, y, 5, 10, 3, 13, 2, color, scale);
            stroke_diag(x, y, 8, 2, 7, 8, 2, color, scale);
            stroke_diag(x, y, 8, 2, 12, 8, 2, color, scale);
            stroke_v(x, y, 9, 8, 13, 2, color, scale);
        }
        '器' => {
            stroke_rect(x, y, 1, 1, 4, 4, 2, color, scale);
            stroke_rect(x, y, 9, 1, 12, 4, 2, color, scale);
            stroke_rect(x, y, 1, 8, 4, 11, 2, color, scale);
            stroke_rect(x, y, 9, 8, 12, 11, 2, color, scale);
            stroke_v(x, y, 6, 4, 8, 2, color, scale);
            stroke_h(x, y, 4, 9, 12, 2, color, scale);
        }
        '题' => {
            stroke_rect(x, y, 1, 1, 5, 5, 2, color, scale);
            stroke_h(x, y, 1, 5, 7, 2, color, scale);
            stroke_v(x, y, 2, 7, 13, 2, color, scale);
            stroke_rect(x, y, 8, 1, 12, 6, 2, color, scale);
            stroke_h(x, y, 8, 12, 9, 2, color, scale);
            stroke_diag(x, y, 9, 9, 7, 13, 2, color, scale);
            stroke_diag(x, y, 10, 9, 12, 13, 2, color, scale);
        }
        '度' => {
            stroke_h(x, y, 1, 12, 1, 2, color, scale);
            stroke_v(x, y, 2, 1, 10, 2, color, scale);
            stroke_h(x, y, 4, 10, 4, 2, color, scale);
            stroke_h(x, y, 4, 10, 7, 2, color, scale);
            stroke_diag(x, y, 7, 7, 3, 13, 2, color, scale);
            stroke_diag(x, y, 8, 7, 11, 13, 2, color, scale);
        }
        _ => return false,
    }

    true
}

fn normalize_glyph(ch: u8) -> u8 {
    if ch.is_ascii_lowercase() {
        ch.to_ascii_uppercase()
    } else {
        ch
    }
}

fn glyph_bitmap(ch: u8) -> [u8; 7] {
    match ch {
        b'0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        b'1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        b'3' => [0x1E, 0x01, 0x01, 0x06, 0x01, 0x01, 0x1E],
        b'4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        b'5' => [0x1F, 0x10, 0x10, 0x1E, 0x01, 0x01, 0x1E],
        b'6' => [0x0E, 0x10, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        b'7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        b'9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x01, 0x0E],
        b'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        b'C' => [0x0F, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0F],
        b'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        b'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        b'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        b'G' => [0x0F, 0x10, 0x10, 0x17, 0x11, 0x11, 0x0F],
        b'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'I' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1F],
        b'J' => [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E],
        b'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        b'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        b'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        b'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        b'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        b'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        b'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        b'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        b'!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        b'#' => [0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A],
        b'(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        b')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        b'+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        b',' => [0x00, 0x00, 0x00, 0x00, 0x04, 0x04, 0x08],
        b'-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        b'.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        b'/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        b':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        b'=' => [0x00, 0x00, 0x1F, 0x00, 0x1F, 0x00, 0x00],
        b'?' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
        b'@' => [0x0E, 0x11, 0x17, 0x15, 0x17, 0x10, 0x0F],
        b'`' => [0x08, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00],
        b' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        _ => [0x00, 0x1F, 0x11, 0x05, 0x00, 0x04, 0x00],
    }
}

fn cjk_glyph16(ch: char) -> Option<[u16; 16]> {
    match ch {
        '文' => Some([
            0x0100, 0x0180, 0x0080, 0x7FFE, 0x1818, 0x0810, 0x0C30, 0x0420, 0x0660, 0x03C0, 0x0180,
            0x03C0, 0x0660, 0x1C38, 0x781E, 0x4002,
        ]),
        '件' => Some([
            0x0820, 0x0960, 0x1B60, 0x1360, 0x33FE, 0x3660, 0x7660, 0xF460, 0x3060, 0x37FE, 0x3060,
            0x3060, 0x3060, 0x3060, 0x3060, 0x1020,
        ]),
        '命' => Some([
            0x0180, 0x0180, 0x03C0, 0x0E70, 0x3C3C, 0x7FEE, 0x4000, 0x3EFC, 0x22C4, 0x22C4, 0x22C4,
            0x22C4, 0x3EDC, 0x20C0, 0x20C0, 0x0040,
        ]),
        '令' => Some([
            0x0080, 0x0180, 0x03C0, 0x0670, 0x1D38, 0x718E, 0x60C6, 0x0000, 0x3FFC, 0x0018, 0x0030,
            0x0660, 0x03C0, 0x01C0, 0x00E0, 0x0060,
        ]),
        '编' => Some([
            0x1020, 0x1030, 0x33FE, 0x3306, 0x2F06, 0x6BFE, 0x7B00, 0x33FE, 0x23AA, 0x7FAA, 0x73AA,
            0x03FE, 0x1FAA, 0x77AA, 0x07AA, 0x0486,
        ]),
        '辑' => Some([
            0x1000, 0x10FC, 0x7E84, 0x30FC, 0x3800, 0x29FE, 0x6884, 0x6884, 0x7EFC, 0x0884, 0x08FC,
            0x3E84, 0x7884, 0x0BFF, 0x0904, 0x0804,
        ]),
        '设' => Some([
            0x0000, 0x30F8, 0x1888, 0x0888, 0x0188, 0x018E, 0xF300, 0x1200, 0x13FC, 0x110C, 0x118C,
            0x10D8, 0x1C70, 0x18F8, 0x33DE, 0x0306,
        ]),
        '置' => Some([
            0x0000, 0x3FFE, 0x2666, 0x3FFE, 0x0180, 0x7FFE, 0x0100, 0x1FF8, 0x1008, 0x1FF8, 0x1008,
            0x1FF8, 0x1FF8, 0x1008, 0x7FFE, 0x0000,
        ]),
        '视' => Some([
            0x19FE, 0x1986, 0x7DA6, 0x0DB6, 0x0DB6, 0x19B6, 0x19A6, 0x3DA6, 0x7DA6, 0x5470, 0x1070,
            0x10F2, 0x10D3, 0x1192, 0x131E, 0x0000,
        ]),
        '频' => Some([
            0x0C00, 0x2CFE, 0x2F18, 0x2C10, 0x2C7E, 0x2CC6, 0xFFD6, 0x0856, 0x2B56, 0x6B56, 0x4A56,
            0x4E56, 0x0C38, 0x186C, 0x70C6, 0x6182,
        ]),
        '音' => Some([
            0x0180, 0x0180, 0x3FFE, 0x0810, 0x0C30, 0x0C30, 0x7FFE, 0x0000, 0x1FF8, 0x1008, 0x1008,
            0x1FF8, 0x1008, 0x1008, 0x1FF8, 0x1008,
        ]),
        '开' => Some([
            0x7FFE, 0x0C30, 0x0C30, 0x0C30, 0x0C30, 0x0C30, 0x7FFE, 0x0C30, 0x0C30, 0x0C30, 0x0830,
            0x1830, 0x3030, 0x7030, 0x4030, 0x0000,
        ]),
        '始' => Some([
            0x1020, 0x3060, 0x3060, 0x30C8, 0xFCCC, 0x2586, 0x27FE, 0x6D82, 0x6C00, 0x6DFE, 0x3986,
            0x1986, 0x1D86, 0x3586, 0x61FE, 0x4186,
        ]),
        '提' => Some([
            0x33FC, 0x3304, 0xFFFC, 0x3304, 0x33FC, 0x3000, 0x3000, 0x3FFE, 0x7820, 0x7120, 0x333C,
            0x3320, 0x36E0, 0x3C7F, 0x7000, 0x0000,
        ]),
        '示' => Some([
            0x0000, 0x3FFC, 0x0000, 0x0000, 0x0000, 0x7FFE, 0x0180, 0x0180, 0x1998, 0x1188, 0x318C,
            0x6184, 0x6186, 0x4180, 0x0700, 0x0000,
        ]),
        '符' => Some([
            0x30C0, 0x3FFE, 0x6D90, 0x4D18, 0x0808, 0x1808, 0x37FE, 0x3018, 0x7308, 0x5108, 0x1188,
            0x1088, 0x1008, 0x1018, 0x1078, 0x0000,
        ]),
        '亮' => Some([
            0x0100, 0x0180, 0x7FFE, 0x0000, 0x1FF8, 0x1818, 0x1FF8, 0x0000, 0x7FFE, 0x6006, 0x67E6,
            0x0C20, 0x0C22, 0x1822, 0x703E, 0x0000,
        ]),
        '度' => Some([
            0x0080, 0x00C0, 0x3FFE, 0x2210, 0x2318, 0x3FFE, 0x2318, 0x2318, 0x23F8, 0x2000, 0x2FFC,
            0x6618, 0x63B0, 0x61E0, 0x5FFE, 0x5C0E,
        ]),
        '量' => Some([
            0x0000, 0x1FF8, 0x1FF8, 0x1008, 0x1FF8, 0x7FFE, 0x7FFE, 0x3FFC, 0x318C, 0x3FFC, 0x3FFC,
            0x0180, 0x3FFC, 0x0180, 0x7FFE, 0x0000,
        ]),
        '主' => Some([
            0x0200, 0x0380, 0x0180, 0x00C0, 0x7FFE, 0x0180, 0x0180, 0x0180, 0x0180, 0x3FFC, 0x0180,
            0x0180, 0x0180, 0x0180, 0x7FFE, 0x0000,
        ]),
        '题' => Some([
            0x3EFE, 0x2210, 0x3E30, 0x22FE, 0x3ED6, 0x00D6, 0x7FD6, 0x08D6, 0x28F6, 0x2FFE, 0x286C,
            0x78C6, 0x7D82, 0x47FE, 0x0000, 0x0000,
        ]),
        '播' => Some([
            0x33F8, 0x312C, 0x312C, 0xFDE8, 0x37FE, 0x30F8, 0x31AC, 0x7F26, 0xF3FE, 0x3326, 0x3326,
            0x33FE, 0x3326, 0x33FE, 0x7304, 0x0000,
        ]),
        '放' => Some([
            0x1860, 0x0860, 0x7F7F, 0x30C4, 0x30C4, 0x3FEC, 0x33EC, 0x3228, 0x3238, 0x3238, 0x3230,
            0x2238, 0x666C, 0x66CE, 0xDF82, 0x0000,
        ]),
        '器' => Some([
            0x0000, 0x3E7C, 0x2244, 0x2244, 0x2244, 0x3E7C, 0x0338, 0x7FFE, 0x0C70, 0x1838, 0x781E,
            0x7E7E, 0x2244, 0x2244, 0x3E7C, 0x2244,
        ]),
        '类' => Some([
            0x0180, 0x1998, 0x0990, 0x0DB0, 0x7FFE, 0x03E0, 0x0DB8, 0x798E, 0x6182, 0x0180, 0x7FFE,
            0x03C0, 0x0660, 0x0E70, 0x781E, 0x6006,
        ]),
        '型' => Some([
            0x0004, 0x7FA4, 0x1324, 0x1324, 0x7FA4, 0x1324, 0x3324, 0x3304, 0x631C, 0x0180, 0x0180,
            0x3FFC, 0x0180, 0x0180, 0x7FFE, 0x0000,
        ]),
        '目' => Some([
            0x3FFC, 0x300C, 0x300C, 0x300C, 0x3FFC, 0x300C, 0x300C, 0x300C, 0x3FFC, 0x300C, 0x300C,
            0x300C, 0x3FFC, 0x300C, 0x1008, 0x0000,
        ]),
        '录' => Some([
            0x0000, 0x3FF8, 0x0008, 0x0008, 0x3FF8, 0x0018, 0x0018, 0x7FFE, 0x0180, 0x398C, 0x0DF8,
            0x03E0, 0x0FB0, 0x799C, 0x618E, 0x0700,
        ]),
        '保' => Some([
            0x0800, 0x0BFE, 0x1A06, 0x1206, 0x3206, 0x73FE, 0xF060, 0xD060, 0x1060, 0x17FE, 0x10F0,
            0x11F8, 0x116C, 0x1766, 0x1463, 0x1060,
        ]),
        '存' => Some([
            0x0200, 0x0200, 0x0600, 0x7FFE, 0x0C00, 0x0C00, 0x19FC, 0x1818, 0x3030, 0x7020, 0x57FF,
            0x1060, 0x1020, 0x1020, 0x1060, 0x11E0,
        ]),
        '上' => Some([
            0x0100, 0x0100, 0x0100, 0x0100, 0x0100, 0x0100, 0x01FC, 0x0100, 0x0100, 0x0100, 0x0100,
            0x0100, 0x0100, 0x0300, 0x7FFE, 0x0000,
        ]),
        '下' => Some([
            0x0000, 0x7FFE, 0x0180, 0x0180, 0x0180, 0x01C0, 0x01F0, 0x01BC, 0x018C, 0x0180, 0x0180,
            0x0180, 0x0180, 0x0180, 0x0100, 0x0000,
        ]),
        _ => None,
    }
}

fn cjk_glyph(ch: char) -> Option<[u8; 8]> {
    match ch {
        '新' => Some([0x10, 0x7C, 0x54, 0x7C, 0x28, 0x7E, 0x24, 0x44]),
        '建' => Some([0x3C, 0x04, 0x3C, 0x04, 0x7E, 0x48, 0x34, 0x7E]),
        '夹' => Some([0x10, 0x7C, 0x38, 0xFE, 0x10, 0x28, 0x44, 0x82]),
        '上' => Some([0x10, 0x10, 0x10, 0x10, 0x10, 0x7C, 0x00, 0x00]),
        '下' => Some([0x10, 0x10, 0x7C, 0x10, 0x10, 0x12, 0x0C, 0x00]),
        '主' => Some([0x10, 0x10, 0x7C, 0x10, 0x10, 0x7C, 0x00, 0x00]),
        '亮' => Some([0x38, 0x44, 0x7C, 0x10, 0x7C, 0x44, 0x44, 0x38]),
        '文' => Some([0x10, 0x10, 0x7C, 0x10, 0x28, 0x44, 0x82, 0x00]),
        '件' => Some([0x10, 0x7C, 0x10, 0xFE, 0x10, 0x10, 0x12, 0x0C]),
        '命' => Some([0x10, 0x28, 0x44, 0xFE, 0x10, 0x7C, 0x44, 0x7C]),
        '令' => Some([0x10, 0x28, 0x44, 0xFE, 0x10, 0x08, 0x10, 0x20]),
        '保' => Some([0x10, 0x28, 0x44, 0x7E, 0x48, 0x7C, 0x48, 0x44]),
        '编' => Some([0x24, 0x24, 0x7E, 0x10, 0x7C, 0x44, 0x7C, 0x44]),
        '辑' => Some([0x42, 0x7E, 0x42, 0x7C, 0x44, 0x7C, 0x44, 0x7C]),
        '设' => Some([0x42, 0x22, 0x14, 0x08, 0x7E, 0x22, 0x14, 0x08]),
        '置' => Some([0x7E, 0x12, 0x7E, 0x12, 0x7E, 0x10, 0x10, 0x7E]),
        '视' => Some([0x42, 0x22, 0x14, 0x08, 0x7C, 0x44, 0x44, 0x38]),
        '频' => Some([0x42, 0x7E, 0x42, 0x3C, 0x20, 0x3E, 0x22, 0x3C]),
        '音' => Some([0x10, 0x7C, 0x10, 0xFE, 0x10, 0x7C, 0x44, 0x7C]),
        '量' => Some([0x7C, 0x44, 0x7C, 0x10, 0x7C, 0x54, 0x54, 0x7C]),
        '开' => Some([0x00, 0xFE, 0x20, 0x20, 0x3C, 0x22, 0x22, 0x00]),
        '始' => Some([0x44, 0x24, 0x1E, 0x44, 0x7C, 0x44, 0x44, 0x38]),
        '提' => Some([0x10, 0x7C, 0x10, 0x7C, 0x54, 0x7C, 0x10, 0x0C]),
        '示' => Some([0x10, 0x7C, 0x10, 0xFE, 0x10, 0x28, 0x44, 0x82]),
        '符' => Some([0x10, 0x28, 0x44, 0xFE, 0x28, 0x44, 0x7C, 0x10]),
        '帮' => Some([0x44, 0xFE, 0x44, 0x7C, 0x10, 0xFE, 0x10, 0x10]),
        '助' => Some([0x42, 0x7E, 0x42, 0x02, 0x02, 0x12, 0x22, 0x1C]),
        '应' => Some([0x08, 0x7E, 0x40, 0x5C, 0x52, 0x52, 0x22, 0x0C]),
        '用' => Some([0x7C, 0x44, 0x44, 0x7C, 0x44, 0x44, 0x44, 0x44]),
        '目' => Some([0x7C, 0x44, 0x44, 0x44, 0x44, 0x44, 0x7C, 0x00]),
        '录' => Some([0x10, 0x7C, 0x54, 0x7C, 0x10, 0x28, 0x44, 0x82]),
        '类' => Some([0x10, 0x54, 0x38, 0x10, 0x10, 0x28, 0x44, 0x82]),
        '型' => Some([0x44, 0x28, 0x10, 0x7C, 0x10, 0x10, 0x12, 0x0C]),
        '存' => Some([0x10, 0x28, 0x44, 0xFE, 0x10, 0x7C, 0x44, 0x44]),
        '运' => Some([0x22, 0x12, 0x04, 0x7C, 0x10, 0x28, 0x44, 0x82]),
        '行' => Some([0x10, 0x7C, 0x10, 0xFE, 0x10, 0x10, 0x12, 0x0C]),
        '播' => Some([0x10, 0x7C, 0x54, 0x7C, 0x10, 0x7C, 0x54, 0x7C]),
        '放' => Some([0x42, 0x22, 0x14, 0x08, 0x24, 0x52, 0x52, 0x8C]),
        '器' => Some([0x7C, 0x44, 0x7C, 0x10, 0x7C, 0x44, 0x7C, 0x00]),
        '关' => Some([0x10, 0x28, 0x44, 0xFE, 0x10, 0x10, 0x28, 0x44]),
        '机' => Some([0x44, 0x28, 0x10, 0x7C, 0x44, 0x54, 0x64, 0x44]),
        '题' => Some([0x20, 0x3E, 0x2A, 0x3E, 0x20, 0x3E, 0x22, 0x3C]),
        _ => None,
    }
}

pub fn shutdown() -> ! {
    unsafe {
        asm!("cli", options(nomem, nostack, preserves_flags));
    }

    io_out32(QEMU_DEBUG_EXIT_PORT, 0x10);
    io_out16(0x604, 0x2000);
    io_out16(0xB004, 0x2000);
    io_out16(0x4004, 0x3400);

    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack));
        }
    }
}

pub fn halt() -> ! {
    unsafe {
        asm!("cli", options(nomem, nostack, preserves_flags));
    }
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack));
        }
    }
}

fn init_serial() {
    io_out8(COM1 + 1, 0x00);
    io_out8(COM1 + 3, 0x80);
    io_out8(COM1 + 0, 0x03);
    io_out8(COM1 + 1, 0x00);
    io_out8(COM1 + 3, 0x03);
    io_out8(COM1 + 2, 0xC7);
    io_out8(COM1 + 4, 0x0B);
}

fn init_ps2_mouse() {
    ps2_wait_write();
    io_out8(0x64, 0xAD);
    ps2_wait_write();
    io_out8(0x64, 0xA7);
    flush_controller_output();

    ps2_wait_write();
    io_out8(0x64, 0xAE);

    ps2_wait_write();
    io_out8(0x64, 0xA8);

    ps2_wait_write();
    io_out8(0x64, 0x20);
    if !ps2_wait_read() {
        return;
    }
    let mut command = unsafe { io_in8(0x60) };
    command |= 0x01;
    command |= 0x02;
    command &= !0x10;
    command |= 0x40;
    command &= !0x20;

    ps2_wait_write();
    io_out8(0x64, 0x60);
    ps2_wait_write();
    io_out8(0x60, command);

    keyboard_write(0xF4);
    let _ = keyboard_read_ack();

    mouse_write(0xF6);
    let _ = mouse_read_ack();
    mouse_write(0xF3);
    let _ = mouse_read_ack();
    mouse_write(200);
    let _ = mouse_read_ack();
    mouse_write(0xE8);
    let _ = mouse_read_ack();
    mouse_write(0x03);
    let _ = mouse_read_ack();
    mouse_write(0xF4);
    let _ = mouse_read_ack();
}

fn flush_controller_output() {
    for _ in 0..256 {
        let status = unsafe { io_in8(0x64) };
        if status & 0x01 == 0 {
            break;
        }
        let _ = unsafe { io_in8(0x60) };
    }
}

fn serial_write_byte(byte: u8) {
    unsafe { while (io_in8(COM1 + 5) & 0x20) == 0 {} }
    io_out8(COM1, byte);
}

fn serial_read_byte() -> Option<u8> {
    if unsafe { io_in8(COM1 + 5) } & 0x01 == 0 {
        return None;
    }
    Some(unsafe { io_in8(COM1) })
}

struct InputByte {
    is_mouse: bool,
    value: u8,
}

fn read_controller_byte() -> Option<InputByte> {
    let mut status: u8;
    unsafe {
        asm!(
            "in al, dx",
            in("dx") 0x64u16,
            out("al") status,
            options(nomem, nostack, preserves_flags)
        );
    }

    if status & 0x01 == 0 {
        return None;
    }

    let mut value: u8;
    unsafe {
        asm!(
            "in al, dx",
            in("dx") 0x60u16,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }

    Some(InputByte {
        is_mouse: status & 0x20 != 0,
        value,
    })
}

fn scancode_to_ascii(code: u8, shift: bool) -> Option<u8> {
    match code {
        0x02 => Some(if shift { b'!' } else { b'1' }),
        0x03 => Some(if shift { b'@' } else { b'2' }),
        0x04 => Some(if shift { b'#' } else { b'3' }),
        0x05 => Some(if shift { b'$' } else { b'4' }),
        0x06 => Some(if shift { b'%' } else { b'5' }),
        0x07 => Some(if shift { b'^' } else { b'6' }),
        0x08 => Some(if shift { b'&' } else { b'7' }),
        0x09 => Some(if shift { b'*' } else { b'8' }),
        0x0A => Some(if shift { b'(' } else { b'9' }),
        0x0B => Some(if shift { b')' } else { b'0' }),
        0x0C => Some(if shift { b'_' } else { b'-' }),
        0x0D => Some(if shift { b'+' } else { b'=' }),
        0x0E => Some(0x08),
        0x10 => Some(letter(b'q', shift)),
        0x11 => Some(letter(b'w', shift)),
        0x12 => Some(letter(b'e', shift)),
        0x13 => Some(letter(b'r', shift)),
        0x14 => Some(letter(b't', shift)),
        0x15 => Some(letter(b'y', shift)),
        0x16 => Some(letter(b'u', shift)),
        0x17 => Some(letter(b'i', shift)),
        0x18 => Some(letter(b'o', shift)),
        0x19 => Some(letter(b'p', shift)),
        0x1A => Some(if shift { b'{' } else { b'[' }),
        0x1B => Some(if shift { b'}' } else { b']' }),
        0x1C => Some(b'\n'),
        0x1E => Some(letter(b'a', shift)),
        0x1F => Some(letter(b's', shift)),
        0x20 => Some(letter(b'd', shift)),
        0x21 => Some(letter(b'f', shift)),
        0x22 => Some(letter(b'g', shift)),
        0x23 => Some(letter(b'h', shift)),
        0x24 => Some(letter(b'j', shift)),
        0x25 => Some(letter(b'k', shift)),
        0x26 => Some(letter(b'l', shift)),
        0x27 => Some(if shift { b':' } else { b';' }),
        0x28 => Some(if shift { b'"' } else { b'\'' }),
        0x29 => Some(if shift { b'~' } else { b'`' }),
        0x2B => Some(if shift { b'|' } else { b'\\' }),
        0x2C => Some(letter(b'z', shift)),
        0x2D => Some(letter(b'x', shift)),
        0x2E => Some(letter(b'c', shift)),
        0x2F => Some(letter(b'v', shift)),
        0x30 => Some(letter(b'b', shift)),
        0x31 => Some(letter(b'n', shift)),
        0x32 => Some(letter(b'm', shift)),
        0x33 => Some(if shift { b'<' } else { b',' }),
        0x34 => Some(if shift { b'>' } else { b'.' }),
        0x35 => Some(if shift { b'?' } else { b'/' }),
        0x39 => Some(b' '),
        _ => None,
    }
}

const fn letter(byte: u8, shift: bool) -> u8 {
    if shift {
        byte - 32
    } else {
        byte
    }
}

fn ps2_wait_write() {
    for _ in 0..100_000 {
        let status = unsafe { io_in8(0x64) };
        if status & 0x02 == 0 {
            return;
        }
    }
}

fn ps2_wait_read() -> bool {
    for _ in 0..100_000 {
        let status = unsafe { io_in8(0x64) };
        if status & 0x01 != 0 {
            return true;
        }
    }
    false
}

fn mouse_write(value: u8) {
    ps2_wait_write();
    io_out8(0x64, 0xD4);
    ps2_wait_write();
    io_out8(0x60, value);
}

fn keyboard_write(value: u8) {
    ps2_wait_write();
    io_out8(0x60, value);
}

fn keyboard_read_ack() -> Option<u8> {
    for _ in 0..100_000 {
        let status = unsafe { io_in8(0x64) };
        if status & 0x01 != 0 {
            let value = unsafe { io_in8(0x60) };
            if status & 0x20 == 0 {
                return Some(value);
            }
        }
        core::hint::spin_loop();
    }
    None
}

fn mouse_read_ack() -> Option<u8> {
    for _ in 0..100_000 {
        let status = unsafe { io_in8(0x64) };
        if status & 0x01 != 0 {
            let value = unsafe { io_in8(0x60) };
            if status & 0x20 != 0 {
                return Some(value);
            }
        }
        core::hint::spin_loop();
    }
    None
}

fn scroll_if_needed() {
    unsafe {
        if (*CURSOR_ROW.get()) < VGA_HEIGHT {
            return;
        }

        for row in 1..VGA_HEIGHT {
            for col in 0..VGA_WIDTH {
                let src = (row * VGA_WIDTH + col) * 2;
                let dst = ((row - 1) * VGA_WIDTH + col) * 2;
                *VGA_BUFFER.add(dst) = *VGA_BUFFER.add(src);
                *VGA_BUFFER.add(dst + 1) = *VGA_BUFFER.add(src + 1);
            }
        }

        let last_row = VGA_HEIGHT - 1;
        for col in 0..VGA_WIDTH {
            let pos = (last_row * VGA_WIDTH + col) * 2;
            *VGA_BUFFER.add(pos) = b' ';
            *VGA_BUFFER.add(pos + 1) = 0x07;
        }

        (*CURSOR_ROW.get()) = VGA_HEIGHT - 1;
    }
}

pub fn port_out8(port: u16, value: u8) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

pub fn port_out16(port: u16, value: u16) {
    unsafe {
        asm!(
            "out dx, ax",
            in("dx") port,
            in("ax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

pub fn port_out32(port: u16, value: u32) {
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

pub fn port_in8(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

pub fn port_in16(port: u16) -> u16 {
    let value: u16;
    unsafe {
        asm!(
            "in ax, dx",
            in("dx") port,
            out("ax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

pub fn port_in32(port: u16) -> u32 {
    let value: u32;
    unsafe {
        asm!(
            "in eax, dx",
            in("dx") port,
            out("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

fn io_out8(port: u16, value: u8) {
    port_out8(port, value);
}

fn io_out16(port: u16, value: u16) {
    port_out16(port, value);
}

fn io_out32(port: u16, value: u32) {
    port_out32(port, value);
}

unsafe fn io_in8(port: u16) -> u8 {
    port_in8(port)
}

fn rtc_snapshot() -> Option<(u8, u8, u8)> {
    for _ in 0..4 {
        wait_for_rtc_update();

        let second = rtc_read_register(0x00);
        let minute = rtc_read_register(0x02);
        let hour = rtc_read_register(0x04);
        let status_b = rtc_read_register(0x0B);

        let binary_mode = status_b & 0x04 != 0;
        let hour_24 = status_b & 0x02 != 0;

        let second = rtc_decode_value(second, binary_mode);
        let minute = rtc_decode_value(minute, binary_mode);
        let mut hour_value = rtc_decode_hour(hour, binary_mode);

        if !hour_24 {
            let pm = hour & 0x80 != 0;
            hour_value %= 12;
            if pm {
                hour_value = hour_value.saturating_add(12);
            }
        }

        if hour_value < 24 && minute < 60 && second < 60 {
            return Some((hour_value, minute, second));
        }
    }

    None
}

fn wait_for_rtc_update() {
    for _ in 0..100_000 {
        if rtc_read_register(0x0A) & 0x80 == 0 {
            return;
        }
        delay(64);
    }
}

fn rtc_read_register(register: u8) -> u8 {
    port_out8(0x70, 0x80 | (register & 0x7F));
    port_in8(0x71)
}

fn rtc_decode_value(value: u8, binary_mode: bool) -> u8 {
    if binary_mode {
        value
    } else {
        ((value >> 4) * 10).saturating_add(value & 0x0F)
    }
}

fn rtc_decode_hour(value: u8, binary_mode: bool) -> u8 {
    if binary_mode {
        value & 0x7F
    } else {
        let raw = value & 0x7F;
        ((raw >> 4) * 10).saturating_add(raw & 0x0F)
    }
}
