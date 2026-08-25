use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use super::acpi::{AcpiInfo, MAX_CPUS};
use super::{interrupts, time};
use crate::mm::address::PHYSICAL_MEMORY_OFFSET;

const IA32_GS_BASE: u32 = 0xc000_0101;
const IA32_KERNEL_GS_BASE: u32 = 0xc000_0102;
const TRAMPOLINE_PHYSICAL: u64 = 0x9000;
const TRAMPOLINE_VECTOR: u8 = (TRAMPOLINE_PHYSICAL >> 12) as u8;
const KERNEL_STACK_BASE: u64 = 0xffff_a200_0000_1000;
const EXCEPTION_STACK_BASE: u64 = 0xffff_a300_0000_1000;
const STACK_STRIDE: u64 = 0x20_000;
const STACK_PAGES: usize = 8;
pub const MAX_LOCK_DEPTH: usize = 8;

#[repr(C)]
pub struct PerCpu {
    self_pointer: AtomicU64,
    logical_id: AtomicU32,
    apic_id: AtomicU32,
    acpi_id: AtomicU32,
    online: AtomicBool,
    kernel_stack_top: AtomicU64,
    syscall_user_rsp: AtomicU64,
    exception_stack_top: AtomicU64,
    irq_disable_depth: AtomicU32,
    lock_depth: AtomicU8,
    lock_ranks: [AtomicU8; MAX_LOCK_DEPTH],
}

impl PerCpu {
    const fn new() -> Self {
        Self {
            self_pointer: AtomicU64::new(0),
            logical_id: AtomicU32::new(0),
            apic_id: AtomicU32::new(0),
            acpi_id: AtomicU32::new(0),
            online: AtomicBool::new(false),
            kernel_stack_top: AtomicU64::new(0),
            syscall_user_rsp: AtomicU64::new(0),
            exception_stack_top: AtomicU64::new(0),
            irq_disable_depth: AtomicU32::new(0),
            lock_depth: AtomicU8::new(0),
            lock_ranks: [const { AtomicU8::new(0) }; MAX_LOCK_DEPTH],
        }
    }

    pub fn logical_id(&self) -> u32 {
        self.logical_id.load(Ordering::Relaxed)
    }

    pub fn apic_id(&self) -> u32 {
        self.apic_id.load(Ordering::Relaxed)
    }

    pub fn irq_disable_depth(&self) -> u32 {
        self.irq_disable_depth.load(Ordering::Relaxed)
    }
}

pub const PER_CPU_KERNEL_STACK_TOP_OFFSET: usize = core::mem::offset_of!(PerCpu, kernel_stack_top);
pub const PER_CPU_SYSCALL_USER_RSP_OFFSET: usize = core::mem::offset_of!(PerCpu, syscall_user_rsp);

static PER_CPUS: [PerCpu; MAX_CPUS] = [const { PerCpu::new() }; MAX_CPUS];
static PER_CPU_READY: AtomicBool = AtomicBool::new(false);
static ONLINE_CPUS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
pub struct SmpInfo {
    pub discovered: usize,
    pub attempted: usize,
    pub online: usize,
    pub per_cpu: bool,
}

pub fn init(acpi: AcpiInfo) -> SmpInfo {
    let bsp_apic_id = interrupts::current_apic_id() as u32;
    let bsp_acpi_id = acpi.processors[..acpi.processor_count]
        .iter()
        .find(|processor| processor.apic_id == bsp_apic_id)
        .map_or(0, |processor| processor.acpi_id);
    let bsp_numa_node = acpi.processors[..acpi.processor_count]
        .iter()
        .find(|processor| processor.apic_id == bsp_apic_id)
        .map_or(0, |processor| processor.numa_node);
    let _ = crate::mm::frame::set_cpu_node(0, bsp_numa_node);
    prepare_cpu(0, bsp_apic_id, bsp_acpi_id, 0xffff_ffff_8080_0000, 0);
    set_current_cpu(0);
    PER_CPUS[0].online.store(true, Ordering::Release);
    ONLINE_CPUS.store(1, Ordering::Release);
    PER_CPU_READY.store(true, Ordering::Release);

    if !install_trampoline() {
        return SmpInfo {
            discovered: acpi.processor_count.max(1),
            attempted: 0,
            online: 1,
            per_cpu: current().logical_id() == 0,
        };
    }
    // The normal kernel policy makes the complete low identity map NX. APs need
    // a few instructions there after enabling paging, so open this window only
    // while interrupts are disabled and APs are started serially.
    if !crate::mm::paging::set_low_identity_executable(true) {
        return SmpInfo {
            discovered: acpi.processor_count.max(1),
            attempted: 0,
            online: 1,
            per_cpu: true,
        };
    }

    let mut logical_id = 1usize;
    let mut attempted = 0usize;
    for processor in &acpi.processors[..acpi.processor_count] {
        if processor.apic_id == bsp_apic_id || !processor.enabled || logical_id >= MAX_CPUS {
            continue;
        }
        let kernel_bottom = KERNEL_STACK_BASE + logical_id as u64 * STACK_STRIDE;
        let exception_bottom = EXCEPTION_STACK_BASE + logical_id as u64 * STACK_STRIDE;
        let Some(kernel_top) =
            crate::mm::paging::map_guarded_kernel_stack(kernel_bottom, STACK_PAGES)
        else {
            break;
        };
        let Some(exception_top) =
            crate::mm::paging::map_guarded_kernel_stack(exception_bottom, STACK_PAGES)
        else {
            break;
        };
        prepare_cpu(
            logical_id,
            processor.apic_id,
            processor.acpi_id,
            kernel_top,
            exception_top,
        );
        let _ = crate::mm::frame::set_cpu_node(logical_id, processor.numa_node);
        patch_trampoline(logical_id, kernel_top);
        attempted += 1;
        if start_ap(processor.apic_id as u8) && wait_online(logical_id, 200_000_000) {
            logical_id += 1;
        } else {
            break;
        }
    }
    let _ = crate::mm::paging::set_low_identity_executable(false);

    SmpInfo {
        discovered: acpi.processor_count.max(1),
        attempted,
        online: ONLINE_CPUS.load(Ordering::Acquire),
        per_cpu: current().logical_id() == 0 && current().apic_id() == bsp_apic_id,
    }
}

pub fn current() -> &'static PerCpu {
    if !PER_CPU_READY.load(Ordering::Acquire) {
        return &PER_CPUS[0];
    }
    let pointer: u64;
    unsafe { asm!("mov {}, gs:[0]", out(reg) pointer, options(nostack, preserves_flags)) };
    if pointer == 0 {
        &PER_CPUS[0]
    } else {
        unsafe { &*(pointer as *const PerCpu) }
    }
}

pub fn online_cpu_count() -> usize {
    ONLINE_CPUS.load(Ordering::Acquire).clamp(1, MAX_CPUS)
}

pub fn set_kernel_stack_top(stack_top: u64) {
    current()
        .kernel_stack_top
        .store(stack_top, Ordering::Release);
}

pub fn lock_order_allows(rank: u8) -> bool {
    let cpu = current();
    let depth = cpu.lock_depth.load(Ordering::Relaxed) as usize;
    rank != 0
        && depth < MAX_LOCK_DEPTH
        && (depth == 0 || cpu.lock_ranks[depth - 1].load(Ordering::Relaxed) < rank)
}

pub fn push_lock_rank(rank: u8) -> bool {
    if !lock_order_allows(rank) {
        return false;
    }
    let cpu = current();
    let depth = cpu.lock_depth.load(Ordering::Relaxed) as usize;
    cpu.lock_ranks[depth].store(rank, Ordering::Relaxed);
    cpu.lock_depth.store((depth + 1) as u8, Ordering::Relaxed);
    true
}

pub fn pop_lock_rank(rank: u8) -> bool {
    let cpu = current();
    let depth = cpu.lock_depth.load(Ordering::Relaxed) as usize;
    if depth == 0 || cpu.lock_ranks[depth - 1].load(Ordering::Relaxed) != rank {
        return false;
    }
    cpu.lock_ranks[depth - 1].store(0, Ordering::Relaxed);
    cpu.lock_depth.store((depth - 1) as u8, Ordering::Relaxed);
    true
}

pub fn irq_disabled_enter() {
    current().irq_disable_depth.fetch_add(1, Ordering::Relaxed);
}

pub fn irq_disabled_exit() -> bool {
    current()
        .irq_disable_depth
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |depth| {
            depth.checked_sub(1)
        })
        .is_ok()
}

fn prepare_cpu(
    logical_id: usize,
    apic_id: u32,
    acpi_id: u32,
    kernel_stack_top: u64,
    exception_stack_top: u64,
) {
    let cpu = &PER_CPUS[logical_id];
    cpu.self_pointer
        .store(cpu as *const PerCpu as u64, Ordering::Relaxed);
    cpu.logical_id.store(logical_id as u32, Ordering::Relaxed);
    cpu.apic_id.store(apic_id, Ordering::Relaxed);
    cpu.acpi_id.store(acpi_id, Ordering::Relaxed);
    cpu.kernel_stack_top
        .store(kernel_stack_top, Ordering::Relaxed);
    cpu.exception_stack_top
        .store(exception_stack_top, Ordering::Relaxed);
    cpu.online.store(false, Ordering::Relaxed);
}

fn set_current_cpu(logical_id: usize) {
    let base = &PER_CPUS[logical_id] as *const PerCpu as u64;
    wrmsr(IA32_GS_BASE, base);
    wrmsr(IA32_KERNEL_GS_BASE, base);
}

fn start_ap(apic_id: u8) -> bool {
    if !interrupts::send_init_ipi(apic_id, true) {
        return false;
    }
    let _ = time::busy_wait_nanoseconds(10_000_000);
    if !interrupts::send_init_ipi(apic_id, false) {
        return false;
    }
    let _ = time::busy_wait_nanoseconds(200_000);
    if !interrupts::send_startup_ipi(apic_id, TRAMPOLINE_VECTOR) {
        return false;
    }
    let _ = time::busy_wait_nanoseconds(200_000);
    if !interrupts::send_startup_ipi(apic_id, TRAMPOLINE_VECTOR) {
        return false;
    }
    true
}

fn wait_online(logical_id: usize, timeout_ns: u64) -> bool {
    let start = time::monotonic_nanoseconds();
    for _ in 0..5_000_000usize {
        if PER_CPUS[logical_id].online.load(Ordering::Acquire) {
            return true;
        }
        if time::monotonic_nanoseconds().wrapping_sub(start) >= timeout_ns {
            return false;
        }
        core::hint::spin_loop();
    }
    false
}

fn install_trampoline() -> bool {
    let start = symbol_address(core::ptr::addr_of!(ap_trampoline_start));
    let end = symbol_address(core::ptr::addr_of!(ap_trampoline_end));
    let size = end.saturating_sub(start);
    if size == 0 || size > 4096 {
        return false;
    }
    let destination = (PHYSICAL_MEMORY_OFFSET + TRAMPOLINE_PHYSICAL) as *mut u8;
    unsafe { core::ptr::copy_nonoverlapping(start as *const u8, destination, size as usize) };
    let gdt_offset = symbol_offset(core::ptr::addr_of!(ap_trampoline_gdt), start);
    write_u32(
        symbol_offset(core::ptr::addr_of!(ap_trampoline_gdt_base), start),
        (TRAMPOLINE_PHYSICAL + gdt_offset) as u32,
    );
    let cr3: u64;
    unsafe { asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };
    write_u32(
        symbol_offset(core::ptr::addr_of!(ap_trampoline_cr3), start),
        cr3 as u32,
    );
    write_u64(
        symbol_offset(core::ptr::addr_of!(ap_trampoline_entry), start),
        ap_entry as *const () as u64,
    );
    true
}

fn patch_trampoline(logical_id: usize, stack_top: u64) {
    let start = symbol_address(core::ptr::addr_of!(ap_trampoline_start));
    write_u64(
        symbol_offset(core::ptr::addr_of!(ap_trampoline_stack), start),
        stack_top,
    );
    write_u64(
        symbol_offset(core::ptr::addr_of!(ap_trampoline_cpu), start),
        logical_id as u64,
    );
}

fn write_u32(offset: u64, value: u32) {
    unsafe {
        core::ptr::write_volatile(
            (PHYSICAL_MEMORY_OFFSET + TRAMPOLINE_PHYSICAL + offset) as *mut u32,
            value,
        )
    };
}

fn write_u64(offset: u64, value: u64) {
    unsafe {
        core::ptr::write_volatile(
            (PHYSICAL_MEMORY_OFFSET + TRAMPOLINE_PHYSICAL + offset) as *mut u64,
            value,
        )
    };
}

fn symbol_address(symbol: *const u8) -> u64 {
    symbol as u64
}

fn symbol_offset(symbol: *const u8, start: u64) -> u64 {
    symbol_address(symbol).saturating_sub(start)
}

fn wrmsr(msr: u32, value: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nostack)
        )
    };
}

#[no_mangle]
extern "C" fn ap_entry(logical_id: u64) -> ! {
    let index = logical_id as usize;
    set_current_cpu(index);
    super::initialize_fpu();
    let _ = super::enable_supervisor_protections();
    let cpu = &PER_CPUS[index];
    interrupts::init_ap(
        index,
        cpu.kernel_stack_top.load(Ordering::Relaxed),
        cpu.exception_stack_top.load(Ordering::Relaxed),
    );
    cpu.online.store(true, Ordering::Release);
    ONLINE_CPUS.fetch_add(1, Ordering::AcqRel);
    unsafe { asm!("sti", "2:", "hlt", "jmp 2b", options(noreturn)) }
}

unsafe extern "C" {
    static ap_trampoline_start: u8;
    static ap_trampoline_end: u8;
    static ap_trampoline_gdt: u8;
    static ap_trampoline_gdt_base: u8;
    static ap_trampoline_cr3: u8;
    static ap_trampoline_stack: u8;
    static ap_trampoline_cpu: u8;
    static ap_trampoline_entry: u8;
}

global_asm!(
    r#"
    .section .rodata.ap_trampoline,"a"
    .set TR_GDT_DESC_OFF, ap_trampoline_gdt_descriptor - ap_trampoline_start
    .set TR_PROTECTED_OFF, ap_trampoline_protected - ap_trampoline_start
    .set TR_CR3_OFF, ap_trampoline_cr3 - ap_trampoline_start
    .set TR_LONG_OFF, ap_trampoline_long - ap_trampoline_start
    .code16
    .global ap_trampoline_start
ap_trampoline_start:
    cli
    cld
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x8ff0
    mov si, 0x9000
    lgdt [si + TR_GDT_DESC_OFF]
    mov eax, cr0
    or eax, 1
    mov cr0, eax
    .byte 0xea
    .word 0x9000 + TR_PROTECTED_OFF
    .word 0x08

    .code32
ap_trampoline_protected:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov esi, 0x9000
    mov eax, dword ptr [esi + TR_CR3_OFF]
    mov cr3, eax
    mov eax, cr4
    or eax, (1 << 5) | (1 << 7) | (1 << 9) | (1 << 10)
    mov cr4, eax
    mov ecx, 0xc0000080
    rdmsr
    or eax, (1 << 8) | (1 << 11)
    wrmsr
    mov eax, cr0
    and eax, 0xfffffff3
    or eax, 0x80010022
    mov cr0, eax
    .byte 0xea
    .long 0x9000 + TR_LONG_OFF
    .word 0x18

    .code64
ap_trampoline_long:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov rsp, qword ptr [rip + ap_trampoline_stack]
    mov rdi, qword ptr [rip + ap_trampoline_cpu]
    mov rax, qword ptr [rip + ap_trampoline_entry]
    jmp rax

    .align 8
    .global ap_trampoline_cr3
ap_trampoline_cr3:
    .long 0
    .align 8
    .global ap_trampoline_stack
ap_trampoline_stack:
    .quad 0
    .global ap_trampoline_cpu
ap_trampoline_cpu:
    .quad 0
    .global ap_trampoline_entry
ap_trampoline_entry:
    .quad 0
    .align 8
    .global ap_trampoline_gdt
ap_trampoline_gdt:
    .quad 0
    .quad 0x00cf9a000000ffff
    .quad 0x00cf92000000ffff
    .quad 0x00af9a000000ffff
ap_trampoline_gdt_descriptor:
    .word (4 * 8) - 1
    .global ap_trampoline_gdt_base
ap_trampoline_gdt_base:
    .long 0
    .global ap_trampoline_end
ap_trampoline_end:
    .code64
    "#
);
