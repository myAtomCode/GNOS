use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::instructions::segmentation::{Segment, CS, DS, ES, SS};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::PrivilegeLevel;
use x86_64::VirtAddr;

use super::acpi::AcpiInfo;
use crate::mm::address::PHYSICAL_MEMORY_OFFSET;
use crate::sync::StaticCell;

pub const TIMER_VECTOR: u8 = 0x40;
pub const SYSCALL_VECTOR: u8 = 0x80;
const FIRST_EXTERNAL_VECTOR: u8 = 0x41;
const LAST_EXTERNAL_VECTOR: u8 = 0x5f;
const SPURIOUS_VECTOR: u8 = 0xff;
const DOUBLE_FAULT_IST: u16 = 0;
const EXCEPTION_STACK_BOTTOM: u64 = 0xffff_a100_0000_1000;
const EXCEPTION_STACK_PAGES: usize = 8;
const IA32_APIC_BASE: u32 = 0x1b;
const IA32_EFER: u32 = 0xc000_0080;
const IA32_STAR: u32 = 0xc000_0081;
const IA32_LSTAR: u32 = 0xc000_0082;
const IA32_FMASK: u32 = 0xc000_0084;
const EFER_SYSTEM_CALL_EXTENSIONS: u64 = 1;
const SYSCALL_FLAGS_MASK: u64 = (1 << 8) | (1 << 9) | (1 << 10) | (1 << 14) | (1 << 16) | (1 << 18);
const KERNEL_CODE_SELECTOR: u64 = 0x08;
const USER_SELECTOR_BASE: u64 = 0x13;
const APIC_ENABLE: u64 = 1 << 11;
const APIC_BASE_MASK: u64 = 0xffff_f000;
const APIC_ID: u32 = 0x20;
const APIC_EOI: u32 = 0xb0;
const APIC_SPURIOUS: u32 = 0xf0;
const APIC_LVT_TIMER: u32 = 0x320;
const APIC_TIMER_INITIAL: u32 = 0x380;
const APIC_TIMER_CURRENT: u32 = 0x390;
const APIC_TIMER_DIVIDE: u32 = 0x3e0;
const APIC_ICR_LOW: u32 = 0x300;
const APIC_ICR_HIGH: u32 = 0x310;

static LOCAL_APIC_BASE: AtomicU64 = AtomicU64::new(0);
static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
static TSS: StaticCell<TaskStateSegment> = StaticCell::new(TaskStateSegment::new());
static GDT: StaticCell<GlobalDescriptorTable> = StaticCell::new(GlobalDescriptorTable::new());
static IDT: StaticCell<InterruptDescriptorTable> = StaticCell::new(InterruptDescriptorTable::new());

global_asm!(
    r#"
    .section .text

    .macro VKERNEL_SAVE_CONTEXT
        cld
        push rax
        push rbx
        push rcx
        push rdx
        push rbp
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15
        mov ax, 0x10
        mov ds, ax
        mov es, ax
    .endm

    .macro VKERNEL_RESTORE_CONTEXT
        # CS is 16 qwords above the saved R15. Restore a user data selector
        # only when iretq is about to return to CPL3.
        mov ax, word ptr [rsp + 128]
        test al, 3
        jz 1f
        mov ax, 0x1b
        jmp 2f
    1:
        mov ax, 0x10
    2:
        mov ds, ax
        mov es, ax
        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rbp
        pop rdx
        pop rcx
        pop rbx
        pop rax
        iretq
    .endm

    .global vkernel_timer_entry
    .type vkernel_timer_entry,@function
vkernel_timer_entry:
    VKERNEL_SAVE_CONTEXT
    mov rdi, rsp
    and rsp, -16
    call vkernel_timer_dispatch
    mov rsp, rax
    VKERNEL_RESTORE_CONTEXT
    .size vkernel_timer_entry, .-vkernel_timer_entry

    .global vkernel_syscall_entry
    .type vkernel_syscall_entry,@function
vkernel_syscall_entry:
    VKERNEL_SAVE_CONTEXT
    mov rdi, rsp
    and rsp, -16
    call vkernel_syscall_dispatch
    mov rsp, rax
    VKERNEL_RESTORE_CONTEXT
    .size vkernel_syscall_entry, .-vkernel_syscall_entry

    .global vkernel_fast_syscall_entry
    .type vkernel_fast_syscall_entry,@function
vkernel_fast_syscall_entry:
    swapgs
    mov qword ptr gs:[{user_rsp_offset}], rsp
    mov rsp, qword ptr gs:[{kernel_stack_offset}]
    push 0x1b
    push qword ptr gs:[{user_rsp_offset}]
    push r11
    push 0x23
    push rcx
    # Linux x86_64 passes argument four in r10. The existing common dispatcher
    # consumes it from the saved rcx slot used by the int 0x80 compatibility ABI.
    mov rcx, r10
    VKERNEL_SAVE_CONTEXT
    mov r12, rsp
    mov rdi, rsp
    and rsp, -16
    call vkernel_syscall_dispatch
    cmp rax, r12
    jne 3f
    mov rsp, rax
    cmp qword ptr [rsp + 128], 0x23
    jne 4f
    cmp qword ptr [rsp + 152], 0x1b
    jne 4f
    mov rcx, qword ptr [rsp + 120]
    mov rdx, rcx
    shl rdx, 16
    sar rdx, 16
    cmp rdx, rcx
    jne 4f
    test rcx, rcx
    js 4f
    mov rdx, qword ptr [rsp + 144]
    mov rax, rdx
    shl rax, 16
    sar rax, 16
    cmp rax, rdx
    jne 4f
    test rdx, rdx
    js 4f
    mov qword ptr gs:[{user_rsp_offset}], rdx
    mov r11, qword ptr [rsp + 136]
    btr r11, 12
    btr r11, 13
    btr r11, 14
    btr r11, 16
    btr r11, 17
    or r11, 2
    pop r15
    pop r14
    pop r13
    pop r12
    add rsp, 8
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rbp
    pop rdx
    add rsp, 8
    pop rbx
    pop rax
    mov rsp, qword ptr gs:[{user_rsp_offset}]
    swapgs
    sysretq
3:
    mov rsp, rax
4:
    test byte ptr [rsp + 128], 3
    jz 5f
    swapgs
5:
    VKERNEL_RESTORE_CONTEXT
    .size vkernel_fast_syscall_entry, .-vkernel_fast_syscall_entry
    "#
    ,
    kernel_stack_offset = const super::smp::PER_CPU_KERNEL_STACK_TOP_OFFSET,
    user_rsp_offset = const super::smp::PER_CPU_SYSCALL_USER_RSP_OFFSET,
);

unsafe extern "C" {
    fn vkernel_timer_entry();
    fn vkernel_syscall_entry();
    fn vkernel_fast_syscall_entry();
}

#[derive(Clone, Copy)]
pub struct InterruptInfo {
    pub idt: bool,
    pub syscall: bool,
    pub exception_guard: bool,
    pub local_apic: bool,
    pub ioapic: bool,
    pub apic_id: u8,
    pub ioapic_redirections: u8,
}

pub fn init(acpi: AcpiInfo) -> InterruptInfo {
    unsafe { asm!("cli", options(nomem, nostack, preserves_flags)) };
    let exception_stack =
        crate::mm::paging::map_guarded_kernel_stack(EXCEPTION_STACK_BOTTOM, EXCEPTION_STACK_PAGES);
    let exception_guard = exception_stack.is_some();
    init_gdt(exception_stack.unwrap_or(0xffff_ffff_8080_0000));
    init_idt(exception_guard);
    let syscall = init_syscall_msrs();

    if acpi.legacy_pic {
        // Once the APIC path is active, leaving the 8259 unmasked can deliver the
        // same physical interrupt through two controllers.
        super::port_out8(0x21, 0xff);
        super::port_out8(0xa1, 0xff);
    }

    let local_apic = init_local_apic(acpi.lapic_address);
    let (ioapic, redirections) = acpi.ioapic_address.map(init_ioapic).unwrap_or((false, 0));
    InterruptInfo {
        idt: true,
        syscall,
        exception_guard,
        local_apic,
        ioapic,
        apic_id: if local_apic {
            (apic_read(APIC_ID) >> 24) as u8
        } else {
            0
        },
        ioapic_redirections: redirections,
    }
}

pub fn enable() {
    unsafe { asm!("sti", options(nomem, nostack, preserves_flags)) };
}

pub fn current_apic_id() -> u8 {
    (apic_read(APIC_ID) >> 24) as u8
}

pub fn init_ap(cpu_index: usize, kernel_stack_top: u64, exception_stack_top: u64) {
    init_ap_gdt(cpu_index, kernel_stack_top, exception_stack_top);
    unsafe { (&*IDT.get()).load() };
    let _ = init_syscall_msrs();
    let physical = LOCAL_APIC_BASE
        .load(Ordering::Acquire)
        .saturating_sub(PHYSICAL_MEMORY_OFFSET);
    if physical != 0 {
        let old = rdmsr(IA32_APIC_BASE);
        wrmsr(
            IA32_APIC_BASE,
            (old & !APIC_BASE_MASK & !(1 << 10)) | physical | APIC_ENABLE,
        );
        let spurious = apic_read(APIC_SPURIOUS);
        apic_write(APIC_SPURIOUS, spurious | 0x100 | SPURIOUS_VECTOR as u32);
    }
}

pub fn send_init_ipi(apic_id: u8, assert: bool) -> bool {
    let level = if assert { 1 << 14 } else { 0 };
    send_ipi(apic_id, (5 << 8) | (1 << 15) | level)
}

pub fn send_startup_ipi(apic_id: u8, vector: u8) -> bool {
    send_ipi(apic_id, (6 << 8) | vector as u32)
}

pub fn timer_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Relaxed)
}

pub fn apic_timer_begin_calibration() -> bool {
    if LOCAL_APIC_BASE.load(Ordering::Acquire) == 0 {
        return false;
    }
    apic_write(APIC_TIMER_DIVIDE, 0x3);
    apic_write(APIC_LVT_TIMER, (1 << 16) | TIMER_VECTOR as u32);
    apic_write(APIC_TIMER_INITIAL, u32::MAX);
    true
}

pub fn apic_timer_elapsed() -> u32 {
    u32::MAX.wrapping_sub(apic_read(APIC_TIMER_CURRENT))
}

pub fn apic_timer_stop() {
    if LOCAL_APIC_BASE.load(Ordering::Acquire) != 0 {
        apic_write(APIC_LVT_TIMER, 1 << 16);
        apic_write(APIC_TIMER_INITIAL, 0);
    }
}

pub fn start_periodic_timer(ticks: u32) -> bool {
    if ticks == 0 || LOCAL_APIC_BASE.load(Ordering::Acquire) == 0 {
        return false;
    }
    TIMER_TICKS.store(0, Ordering::Relaxed);
    apic_write(APIC_TIMER_DIVIDE, 0x3);
    apic_write(APIC_LVT_TIMER, (1 << 17) | TIMER_VECTOR as u32);
    apic_write(APIC_TIMER_INITIAL, ticks);
    true
}

fn init_gdt(exception_stack_top: u64) {
    unsafe {
        let tss = &mut *TSS.get();
        tss.privilege_stack_table[0] = VirtAddr::new(0xffff_ffff_8080_0000);
        tss.interrupt_stack_table[DOUBLE_FAULT_IST as usize] = VirtAddr::new(exception_stack_top);
        let gdt = &mut *GDT.get();
        let code = gdt.append(Descriptor::kernel_code_segment());
        let data = gdt.append(Descriptor::kernel_data_segment());
        let user_data = gdt.append(Descriptor::user_data_segment());
        let user_code = gdt.append(Descriptor::user_code_segment());
        let tss_selector = gdt.append(Descriptor::tss_segment(tss));
        debug_assert_eq!(user_data.0 | 3, 0x1b);
        debug_assert_eq!(user_code.0 | 3, 0x23);
        (&*GDT.get()).load();
        CS::set_reg(code);
        DS::set_reg(data);
        ES::set_reg(data);
        SS::set_reg(data);
        load_tss(tss_selector);
    }
}

#[repr(C, align(16))]
struct ApGdt {
    entries: [u64; 7],
}

impl ApGdt {
    const fn new() -> Self {
        Self { entries: [0; 7] }
    }
}

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

static AP_TSS: StaticCell<[TaskStateSegment; super::acpi::MAX_CPUS]> =
    StaticCell::new([TaskStateSegment::new(); super::acpi::MAX_CPUS]);
static AP_GDTS: StaticCell<[ApGdt; super::acpi::MAX_CPUS]> =
    StaticCell::new([const { ApGdt::new() }; super::acpi::MAX_CPUS]);

fn init_ap_gdt(cpu_index: usize, kernel_stack_top: u64, exception_stack_top: u64) {
    if cpu_index >= super::acpi::MAX_CPUS {
        fatal_exception(b"AP-ID", cpu_index as u64);
    }
    unsafe {
        let tss = &mut (*AP_TSS.get())[cpu_index];
        tss.privilege_stack_table[0] = VirtAddr::new(kernel_stack_top);
        tss.interrupt_stack_table[DOUBLE_FAULT_IST as usize] = VirtAddr::new(exception_stack_top);
        let tss_base = tss as *const TaskStateSegment as u64;
        let tss_limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;
        let tss_low = (tss_limit & 0xffff)
            | ((tss_base & 0xffff) << 16)
            | (((tss_base >> 16) & 0xff) << 32)
            | (0x89u64 << 40)
            | (((tss_limit >> 16) & 0xf) << 48)
            | (((tss_base >> 24) & 0xff) << 56);
        let gdt = &mut (*AP_GDTS.get())[cpu_index];
        gdt.entries = [
            0,
            0x00af_9a00_0000_ffff,
            0x00af_9200_0000_ffff,
            0x00af_f200_0000_ffff,
            0x00af_fa00_0000_ffff,
            tss_low,
            tss_base >> 32,
        ];
        let pointer = DescriptorTablePointer {
            limit: (core::mem::size_of::<ApGdt>() - 1) as u16,
            base: gdt as *const ApGdt as u64,
        };
        asm!(
            "lgdt [{pointer}]",
            "push 0x08",
            "lea rax, [rip + 2f]",
            "push rax",
            "retfq",
            "2:",
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "mov ax, 0x28",
            "ltr ax",
            pointer = in(reg) &pointer,
            out("rax") _,
            options(preserves_flags)
        );
    }
}

fn init_syscall_msrs() -> bool {
    let maximum_extended = core::arch::x86_64::__cpuid(0x8000_0000).eax;
    if maximum_extended < 0x8000_0001
        || core::arch::x86_64::__cpuid(0x8000_0001).edx & (1 << 11) == 0
    {
        return false;
    }
    let star = (USER_SELECTOR_BASE << 48) | (KERNEL_CODE_SELECTOR << 32);
    wrmsr(IA32_STAR, star);
    wrmsr(
        IA32_LSTAR,
        vkernel_fast_syscall_entry as *const () as usize as u64,
    );
    wrmsr(IA32_FMASK, SYSCALL_FLAGS_MASK);
    wrmsr(IA32_EFER, rdmsr(IA32_EFER) | EFER_SYSTEM_CALL_EXTENSIONS);
    true
}

fn init_idt(use_ist: bool) {
    unsafe {
        let idt = &mut *IDT.get();
        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.debug.set_handler_fn(debug_handler);
        idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.overflow.set_handler_fn(overflow_handler);
        idt.bound_range_exceeded.set_handler_fn(bound_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.device_not_available
            .set_handler_fn(device_unavailable_handler);
        idt.invalid_tss.set_handler_fn(invalid_tss_handler);
        idt.segment_not_present
            .set_handler_fn(segment_not_present_handler);
        idt.stack_segment_fault.set_handler_fn(stack_fault_handler);
        idt.general_protection_fault
            .set_handler_fn(general_protection_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.x87_floating_point.set_handler_fn(x87_handler);
        idt.alignment_check.set_handler_fn(alignment_handler);
        idt.machine_check.set_handler_fn(machine_check_handler);
        idt.simd_floating_point.set_handler_fn(simd_handler);
        let double_fault = idt.double_fault.set_handler_fn(double_fault_handler);
        if use_ist {
            double_fault.set_stack_index(DOUBLE_FAULT_IST);
        }
        idt[TIMER_VECTOR].set_handler_addr(VirtAddr::new(
            vkernel_timer_entry as *const () as usize as u64,
        ));
        idt[SYSCALL_VECTOR]
            .set_handler_addr(VirtAddr::new(
                vkernel_syscall_entry as *const () as usize as u64,
            ))
            .set_privilege_level(PrivilegeLevel::Ring3);
        for vector in FIRST_EXTERNAL_VECTOR..=LAST_EXTERNAL_VECTOR {
            idt[vector].set_handler_fn(unexpected_irq_handler);
        }
        idt[SPURIOUS_VECTOR].set_handler_fn(spurious_handler);
        (&*IDT.get()).load();
    }
}

fn init_local_apic(acpi_address: u64) -> bool {
    let mut base = rdmsr(IA32_APIC_BASE);
    let physical = if acpi_address != 0 {
        acpi_address
    } else {
        base & APIC_BASE_MASK
    };
    if physical == 0
        || physical >= 0x1_0000_0000
        || !crate::mm::paging::map_mmio_uncached(physical, 4096)
    {
        return false;
    }
    // Force xAPIC MMIO mode. The bootstrap CPU has not enabled x2APIC yet, and
    // xAPIC keeps the same implementation working on older Linux-compatible PCs.
    base = (base & !APIC_BASE_MASK & !(1 << 10)) | physical | APIC_ENABLE;
    wrmsr(IA32_APIC_BASE, base);
    LOCAL_APIC_BASE.store(PHYSICAL_MEMORY_OFFSET + physical, Ordering::Release);
    let spurious = apic_read(APIC_SPURIOUS);
    apic_write(APIC_SPURIOUS, spurious | 0x100 | SPURIOUS_VECTOR as u32);
    true
}

fn init_ioapic(physical: u64) -> (bool, u8) {
    if physical >= 0x1_0000_0000 || !crate::mm::paging::map_mmio_uncached(physical, 4096) {
        return (false, 0);
    }
    let base = PHYSICAL_MEMORY_OFFSET + physical;
    let version = ioapic_read(base, 1);
    let count = (((version >> 16) & 0xff) + 1).min(120) as u8;
    // Devices remain polling-driven for now. Masking all redirections before
    // enabling IF prevents firmware routing state from causing an IRQ storm.
    for index in 0..count {
        let low_register = 0x10 + index as u8 * 2;
        let low = ioapic_read(base, low_register);
        ioapic_write(base, low_register, low | (1 << 16));
    }
    (true, count)
}

fn ioapic_read(base: u64, register: u8) -> u32 {
    unsafe {
        core::ptr::write_volatile(base as *mut u32, register as u32);
        core::ptr::read_volatile((base + 0x10) as *const u32)
    }
}

fn ioapic_write(base: u64, register: u8, value: u32) {
    unsafe {
        core::ptr::write_volatile(base as *mut u32, register as u32);
        core::ptr::write_volatile((base + 0x10) as *mut u32, value);
    }
}

fn apic_read(register: u32) -> u32 {
    let base = LOCAL_APIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        return 0;
    }
    unsafe { core::ptr::read_volatile((base + register as u64) as *const u32) }
}

fn apic_write(register: u32, value: u32) {
    let base = LOCAL_APIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    unsafe { core::ptr::write_volatile((base + register as u64) as *mut u32, value) };
    let _ = apic_read(APIC_ID);
}

fn send_ipi(apic_id: u8, command: u32) -> bool {
    if LOCAL_APIC_BASE.load(Ordering::Acquire) == 0 {
        return false;
    }
    for _ in 0..100_000usize {
        if apic_read(APIC_ICR_LOW) & (1 << 12) == 0 {
            apic_write(APIC_ICR_HIGH, (apic_id as u32) << 24);
            apic_write(APIC_ICR_LOW, command);
            for _ in 0..100_000usize {
                if apic_read(APIC_ICR_LOW) & (1 << 12) == 0 {
                    return true;
                }
                core::hint::spin_loop();
            }
            return false;
        }
        core::hint::spin_loop();
    }
    false
}

fn local_apic_eoi() {
    apic_write(APIC_EOI, 0);
}

fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high, options(nostack));
    }
    (high as u64) << 32 | low as u64
}

fn wrmsr(msr: u32, value: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nostack)
        );
    }
}

extern "x86-interrupt" fn divide_error_handler(_frame: InterruptStackFrame) {
    fatal_exception(b"#DE", 0)
}

extern "x86-interrupt" fn debug_handler(_frame: InterruptStackFrame) {
    fatal_exception(b"#DB", 0)
}

extern "x86-interrupt" fn nmi_handler(_frame: InterruptStackFrame) {
    fatal_exception(b"NMI", 0)
}

extern "x86-interrupt" fn breakpoint_handler(_frame: InterruptStackFrame) {
    serial_text(b"[exception] breakpoint\r\n");
}

extern "x86-interrupt" fn overflow_handler(_frame: InterruptStackFrame) {
    fatal_exception(b"#OF", 0)
}

extern "x86-interrupt" fn bound_handler(_frame: InterruptStackFrame) {
    fatal_exception(b"#BR", 0)
}

extern "x86-interrupt" fn invalid_opcode_handler(frame: InterruptStackFrame) {
    fatal_exception(b"#UD", frame.instruction_pointer.as_u64())
}

extern "x86-interrupt" fn device_unavailable_handler(_frame: InterruptStackFrame) {
    fatal_exception(b"#NM", 0)
}

extern "x86-interrupt" fn invalid_tss_handler(_frame: InterruptStackFrame, error: u64) {
    fatal_exception(b"#TS", error)
}

extern "x86-interrupt" fn segment_not_present_handler(_frame: InterruptStackFrame, error: u64) {
    fatal_exception(b"#NP", error)
}

extern "x86-interrupt" fn stack_fault_handler(_frame: InterruptStackFrame, error: u64) {
    fatal_exception(b"#SS", error)
}

extern "x86-interrupt" fn general_protection_handler(_frame: InterruptStackFrame, error: u64) {
    fatal_exception(b"#GP", error)
}

extern "x86-interrupt" fn double_fault_handler(_frame: InterruptStackFrame, error: u64) -> ! {
    fatal_exception(b"#DF", error)
}

extern "x86-interrupt" fn page_fault_handler(
    _frame: InterruptStackFrame,
    error: PageFaultErrorCode,
) {
    let address: u64;
    unsafe { asm!("mov {}, cr2", out(reg) address, options(nomem, nostack, preserves_flags)) };
    let _ = error;
    fatal_exception(b"#PF", address)
}

extern "x86-interrupt" fn x87_handler(_frame: InterruptStackFrame) {
    fatal_exception(b"#MF", 0)
}

extern "x86-interrupt" fn alignment_handler(_frame: InterruptStackFrame, error: u64) {
    fatal_exception(b"#AC", error)
}

extern "x86-interrupt" fn machine_check_handler(_frame: InterruptStackFrame) -> ! {
    fatal_exception(b"#MC", 0)
}

extern "x86-interrupt" fn simd_handler(_frame: InterruptStackFrame) {
    fatal_exception(b"#XM", 0)
}

#[no_mangle]
extern "C" fn vkernel_timer_dispatch(
    frame: *mut crate::kernel::scheduler::RegisterFrame,
) -> *mut crate::kernel::scheduler::RegisterFrame {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    crate::gui::poll_locked_input();
    local_apic_eoi();
    crate::kernel::scheduler::on_timer(frame)
}

#[no_mangle]
extern "C" fn vkernel_syscall_dispatch(
    frame: *mut crate::kernel::scheduler::RegisterFrame,
) -> *mut crate::kernel::scheduler::RegisterFrame {
    crate::kernel::scheduler::on_syscall(frame)
}

extern "x86-interrupt" fn unexpected_irq_handler(_frame: InterruptStackFrame) {
    local_apic_eoi();
}

extern "x86-interrupt" fn spurious_handler(_frame: InterruptStackFrame) {}

fn fatal_exception(name: &[u8], value: u64) -> ! {
    unsafe { asm!("cli", options(nomem, nostack, preserves_flags)) };
    crate::kernel::scheduler::dump_recent_syscalls();
    serial_text(b"\r\n[fatal exception] ");
    serial_text(name);
    serial_text(b" value=0x");
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xf) as u8;
        super::port_out8(
            0x3f8,
            if nibble < 10 {
                b'0' + nibble
            } else {
                b'a' + nibble - 10
            },
        );
    }
    serial_text(b"\r\n");
    loop {
        unsafe { asm!("hlt", options(nomem, nostack)) };
    }
}

fn serial_text(text: &[u8]) {
    for &byte in text {
        super::port_out8(0x3f8, byte);
    }
}

/// Updates the ring-transition stack used by the BSP TSS. The scheduler calls
/// this with interrupts disabled before returning to a different thread.
pub fn set_kernel_stack_top(stack_top: u64) {
    unsafe {
        (*TSS.get()).privilege_stack_table[0] = VirtAddr::new(stack_top);
    }
}
