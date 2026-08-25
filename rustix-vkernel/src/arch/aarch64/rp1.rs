use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};

use super::platform;

const PCIE_EXT_CFG_DATA: usize = 0x8000;
const PCIE_EXT_CFG_INDEX: usize = 0x9000;
const PCIE_STATUS: usize = 0x4068;
const RP1_VENDOR_ID: u16 = 0x1de4;
const RP1_DEVICE_ID: u16 = 0x0001;

static LINK_UP: AtomicBool = AtomicBool::new(false);
static FOUND: AtomicBool = AtomicBool::new(false);
static VENDOR: AtomicU16 = AtomicU16::new(u16::MAX);
static DEVICE: AtomicU16 = AtomicU16::new(u16::MAX);
static BUS_DEVICE_FUNCTION: AtomicU32 = AtomicU32::new(0);
static BAR0: AtomicU64 = AtomicU64::new(0);

pub struct Rp1Info {
    pub controller: usize,
    pub link_up: bool,
    pub found: bool,
    pub vendor: u16,
    pub device: u16,
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub bar0: u64,
}

pub fn probe() {
    let Some(controller) = platform::pcie_controller_base() else {
        return;
    };
    let status = read32(controller + PCIE_STATUS);
    LINK_UP.store(status & ((1 << 4) | (1 << 5)) != 0, Ordering::Relaxed);
    for bus in 1..=3u8 {
        for slot in 0..32u8 {
            let identity = config_read32(controller, bus, slot, 0, 0);
            let vendor = identity as u16;
            let device = (identity >> 16) as u16;
            if vendor != RP1_VENDOR_ID || device != RP1_DEVICE_ID {
                continue;
            }
            let bar_low = config_read32(controller, bus, slot, 0, 0x10);
            let bar0 = if bar_low & 0x6 == 0x4 {
                let high = config_read32(controller, bus, slot, 0, 0x14);
                ((high as u64) << 32) | (bar_low as u64 & !0xf)
            } else {
                (bar_low & !0xf) as u64
            };
            VENDOR.store(vendor, Ordering::Relaxed);
            DEVICE.store(device, Ordering::Relaxed);
            BUS_DEVICE_FUNCTION.store(
                ((bus as u32) << 16) | ((slot as u32) << 8),
                Ordering::Relaxed,
            );
            BAR0.store(bar0, Ordering::Relaxed);
            FOUND.store(true, Ordering::Release);
            return;
        }
    }
}

pub fn info() -> Rp1Info {
    let location = BUS_DEVICE_FUNCTION.load(Ordering::Relaxed);
    Rp1Info {
        controller: platform::pcie_controller_base().unwrap_or(0),
        link_up: LINK_UP.load(Ordering::Relaxed),
        found: FOUND.load(Ordering::Acquire),
        vendor: VENDOR.load(Ordering::Relaxed),
        device: DEVICE.load(Ordering::Relaxed),
        bus: (location >> 16) as u8,
        slot: (location >> 8) as u8,
        function: location as u8,
        bar0: BAR0.load(Ordering::Relaxed),
    }
}

fn config_read32(controller: usize, bus: u8, slot: u8, function: u8, offset: u16) -> u32 {
    let index = ((bus as u32) << 20) | ((slot as u32) << 15) | ((function as u32) << 12);
    write32(controller + PCIE_EXT_CFG_INDEX, index);
    read32(controller + PCIE_EXT_CFG_DATA + (offset as usize & 0xfff))
}

fn read32(address: usize) -> u32 {
    unsafe { (address as *const u32).read_volatile() }
}

fn write32(address: usize, value: u32) {
    unsafe { (address as *mut u32).write_volatile(value) }
}
