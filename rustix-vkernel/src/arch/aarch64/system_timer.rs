use super::platform;

const COUNTER_LOW: usize = 0x04;
const COUNTER_HIGH: usize = 0x08;

pub fn counter_ns() -> Option<u64> {
    let base = platform::system_timer_base()?;
    loop {
        let high = read(base + COUNTER_HIGH);
        let low = read(base + COUNTER_LOW);
        if high == read(base + COUNTER_HIGH) {
            return Some(((high as u64) << 32 | low as u64).saturating_mul(1_000));
        }
    }
}

fn read(address: usize) -> u32 {
    unsafe { (address as *const u32).read_volatile() }
}
