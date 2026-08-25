use core::ffi::{c_int, c_uint, c_void};

unsafe extern "C" {
    fn rustix_bitmap_fill(words: *mut u64, word_count: usize, value: u64);
    fn rustix_bitmap_count_clear(words: *const u64, word_count: usize) -> usize;
    fn rustix_bitmap_find_clear(
        words: *const u64,
        word_count: usize,
        start_word: usize,
        bit_index: *mut c_uint,
    ) -> usize;
    fn rustix_sort_unique_u64(values: *mut u64, count: usize) -> usize;
    fn rustix_internet_checksum(data: *const u8, length: usize) -> u16;
    fn rustix_parse_ipv4(text: *const u8, length: usize, output: *mut u8) -> c_int;
    fn rustix_checksum8_is_zero(data: *const u8, length: usize) -> c_int;
    fn rustix_zero_explicit(memory: *mut c_void, length: usize);
}

pub fn bitmap_fill(words: &mut [u64], value: u64) {
    unsafe { rustix_bitmap_fill(words.as_mut_ptr(), words.len(), value) }
}

pub fn bitmap_count_clear(words: &[u64]) -> usize {
    unsafe { rustix_bitmap_count_clear(words.as_ptr(), words.len()) }
}

pub fn bitmap_find_clear(words: &[u64], start_word: usize) -> Option<(usize, usize)> {
    let mut bit = 0;
    let word =
        unsafe { rustix_bitmap_find_clear(words.as_ptr(), words.len(), start_word, &mut bit) };
    if word >= words.len() || bit >= 64 {
        None
    } else {
        Some((word, bit as usize))
    }
}

pub fn sort_unique_u64(values: &mut [u64]) -> usize {
    let unique = unsafe { rustix_sort_unique_u64(values.as_mut_ptr(), values.len()) };
    unique.min(values.len())
}

pub fn internet_checksum(data: &[u8]) -> u16 {
    unsafe { rustix_internet_checksum(data.as_ptr(), data.len()) }
}

pub fn parse_ipv4(text: &str) -> Option<[u8; 4]> {
    let mut output = [0; 4];
    let valid = unsafe { rustix_parse_ipv4(text.as_ptr(), text.len(), output.as_mut_ptr()) };
    (valid == 1).then_some(output)
}

/// The caller must provide a readable range for the complete length.
pub unsafe fn checksum8_is_zero(data: *const u8, length: usize) -> bool {
    unsafe { rustix_checksum8_is_zero(data, length) == 1 }
}

/// The caller must provide a writable, exclusively borrowed range.
pub unsafe fn zero_explicit(memory: *mut u8, length: usize) {
    unsafe { rustix_zero_explicit(memory.cast(), length) }
}

pub fn self_test() -> bool {
    let mut bitmap = [0, 0];
    bitmap_fill(&mut bitmap, u64::MAX);
    bitmap[1] &= !(1 << 17);
    let bitmap_ok =
        bitmap_count_clear(&bitmap) == 1 && bitmap_find_clear(&bitmap, 0) == Some((1, 17));

    let mut values = [9, 1, 9, 4, 1, 7];
    let unique = sort_unique_u64(&mut values);
    let sort_ok = unique == 4 && values[..unique] == [1, 4, 7, 9];
    let parse_ok =
        parse_ipv4("10.0.2.15") == Some([10, 0, 2, 15]) && parse_ipv4("256.1.1.1").is_none();
    let checksum_ok =
        internet_checksum(&[0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7]) == 0x220d;
    let firmware_checksum = [1u8, 2, 253];
    let firmware_ok =
        unsafe { checksum8_is_zero(firmware_checksum.as_ptr(), firmware_checksum.len()) };
    let mut secret = [0xa5u8; 17];
    unsafe { zero_explicit(secret.as_mut_ptr(), secret.len()) };
    let zero_ok = secret.iter().all(|byte| *byte == 0);

    bitmap_ok && sort_ok && parse_ok && checksum_ok && firmware_ok && zero_ok
}
