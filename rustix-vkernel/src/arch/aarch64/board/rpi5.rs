use core::arch::asm;

pub const NAME: &str = "aarch64-rpi5";
pub const SHELL_BANNER: &str = "Rustix v-kernel (Raspberry Pi 5)";
pub const UNAME: &str = "Rustix v-kernel aarch64 raspberrypi-5";
pub const PROC_VERSION: &str = "Rustix v-kernel 0.1.0 aarch64 raspberrypi-5\n";

pub const UART_BASE: usize = 0x10_7d00_1000;
pub const UART_REQUIRES_INIT: bool = false;
pub const UART_IBRD: u32 = 0;
pub const UART_FBRD: u32 = 0;
pub const GIC_DISTRIBUTOR_BASE: usize = 0x10_7fff_9000;
pub const GIC_CPU_INTERFACE_BASE: usize = 0x10_7fff_a000;
pub const GIC_USE_GROUP1: bool = true;
pub const SYSTEM_TIMER_BASE: Option<usize> = Some(0x10_7d00_3000);
pub const SECONDARIES_ENTER_IMAGE: bool = true;
pub const DEFAULT_PSCI_HVC: bool = false;

pub fn power_off() -> ! {
    unsafe {
        asm!("smc #0", in("x0") 0x8400_0008u64, options(nostack));
    }
    loop {
        unsafe { asm!("wfi", options(nomem, nostack)) };
    }
}
