use core::arch::asm;

pub const NAME: &str = "aarch64-qemu-virt";
pub const SHELL_BANNER: &str = "Rustix v-kernel (AArch64 QEMU virt)";
pub const UNAME: &str = "Rustix v-kernel aarch64 qemu-virt";
pub const PROC_VERSION: &str = "Rustix v-kernel 0.1.0 aarch64 qemu-virt\n";

pub const UART_BASE: usize = 0x0900_0000;
pub const UART_REQUIRES_INIT: bool = true;
pub const UART_IBRD: u32 = 13;
pub const UART_FBRD: u32 = 1;
pub const GIC_DISTRIBUTOR_BASE: usize = 0x0800_0000;
pub const GIC_CPU_INTERFACE_BASE: usize = 0x0801_0000;
pub const GIC_USE_GROUP1: bool = false;
pub const SYSTEM_TIMER_BASE: Option<usize> = None;
pub const SECONDARIES_ENTER_IMAGE: bool = false;
pub const DEFAULT_PSCI_HVC: bool = true;

pub fn power_off() -> ! {
    unsafe {
        asm!("hvc #0", in("x0") 0x8400_0008u64, options(nostack));
    }
    loop {
        unsafe { asm!("wfi", options(nomem, nostack)) };
    }
}
