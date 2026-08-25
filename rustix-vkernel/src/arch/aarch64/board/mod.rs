#[cfg(all(feature = "board-qemu-virt", feature = "board-rpi5"))]
compile_error!("select exactly one AArch64 board feature");

#[cfg(not(any(feature = "board-qemu-virt", feature = "board-rpi5")))]
compile_error!("an AArch64 board feature is required");

#[cfg(feature = "board-qemu-virt")]
mod qemu_virt;
#[cfg(feature = "board-rpi5")]
mod rpi5;

#[cfg(feature = "board-qemu-virt")]
pub use qemu_virt::*;
#[cfg(feature = "board-rpi5")]
pub use rpi5::*;
