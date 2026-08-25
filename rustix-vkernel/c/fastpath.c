#include "fastpath.h"

#define RUSTIX_NOT_FOUND ((size_t)-1)

static size_t popcount64(uint64_t value) {
    value -= (value >> 1) & UINT64_C(0x5555555555555555);
    value = (value & UINT64_C(0x3333333333333333))
          + ((value >> 2) & UINT64_C(0x3333333333333333));
    value = (value + (value >> 4)) & UINT64_C(0x0f0f0f0f0f0f0f0f);
    return (size_t)((value * UINT64_C(0x0101010101010101)) >> 56);
}

void rustix_bitmap_fill(uint64_t *words, size_t word_count, uint64_t value) {
    if (words == NULL && word_count != 0) {
        return;
    }
    for (size_t index = 0; index < word_count; ++index) {
        words[index] = value;
    }
}

size_t rustix_bitmap_count_clear(const uint64_t *words, size_t word_count) {
    if (words == NULL && word_count != 0) {
        return 0;
    }
    size_t total = 0;
    for (size_t index = 0; index < word_count; ++index) {
        total += popcount64(~words[index]);
    }
    return total;
}

static size_t find_clear_in_range(const uint64_t *words, size_t begin,
                                  size_t end, unsigned int *bit_index) {
    for (size_t index = begin; index < end; ++index) {
        uint64_t available = ~words[index];
        if (available != 0) {
            *bit_index = (unsigned int)__builtin_ctzll(available);
            return index;
        }
    }
    return RUSTIX_NOT_FOUND;
}

size_t rustix_bitmap_find_clear(const uint64_t *words, size_t word_count,
                                size_t start_word, unsigned int *bit_index) {
    if (word_count == 0 || words == NULL || bit_index == NULL) {
        return RUSTIX_NOT_FOUND;
    }
    if (start_word >= word_count) {
        start_word = 0;
    }
    size_t found = find_clear_in_range(words, start_word, word_count, bit_index);
    if (found != RUSTIX_NOT_FOUND) {
        return found;
    }
    return find_clear_in_range(words, 0, start_word, bit_index);
}

static void swap_u64(uint64_t *left, uint64_t *right) {
    uint64_t temporary = *left;
    *left = *right;
    *right = temporary;
}

static void sift_down(uint64_t *values, size_t root, size_t end) {
    if (end == 0) {
        return;
    }
    while (root <= (end - 1) / 2) {
        size_t child = root * 2 + 1;
        if (child < end && values[child] < values[child + 1]) {
            ++child;
        }
        if (values[root] >= values[child]) {
            return;
        }
        swap_u64(&values[root], &values[child]);
        root = child;
    }
}

size_t rustix_sort_unique_u64(uint64_t *values, size_t count) {
    if (count == 0) {
        return 0;
    }
    if (values == NULL) {
        return 0;
    }

    for (size_t start = count / 2; start != 0; --start) {
        sift_down(values, start - 1, count - 1);
    }
    for (size_t end = count - 1; end != 0; --end) {
        swap_u64(&values[0], &values[end]);
        sift_down(values, 0, end - 1);
    }

    size_t unique = 1;
    for (size_t index = 1; index < count; ++index) {
        if (values[index] != values[unique - 1]) {
            values[unique++] = values[index];
        }
    }
    return unique;
}

uint16_t rustix_internet_checksum(const uint8_t *data, size_t length) {
    if (data == NULL && length != 0) {
        return 0;
    }
    uint64_t sum = 0;
    while (length >= 2) {
        sum += ((uint16_t)data[0] << 8) | data[1];
        data += 2;
        length -= 2;
    }
    if (length != 0) {
        sum += (uint16_t)data[0] << 8;
    }
    while ((sum >> 16) != 0) {
        sum = (sum & UINT64_C(0xffff)) + (sum >> 16);
    }
    return (uint16_t)~sum;
}

int rustix_parse_ipv4(const uint8_t *text, size_t length, uint8_t output[4]) {
    if (text == NULL || output == NULL || length < 7 || length > 15) {
        return 0;
    }
    size_t component = 0;
    unsigned int value = 0;
    unsigned int digits = 0;

    for (size_t index = 0; index < length; ++index) {
        uint8_t byte = text[index];
        if (byte >= (uint8_t)'0' && byte <= (uint8_t)'9') {
            if (++digits > 3) {
                return 0;
            }
            value = value * 10U + (unsigned int)(byte - (uint8_t)'0');
            if (value > 255U) {
                return 0;
            }
        } else if (byte == (uint8_t)'.') {
            if (digits == 0 || component >= 3) {
                return 0;
            }
            output[component++] = (uint8_t)value;
            value = 0;
            digits = 0;
        } else {
            return 0;
        }
    }
    if (digits == 0 || component != 3) {
        return 0;
    }
    output[3] = (uint8_t)value;
    return 1;
}

int rustix_checksum8_is_zero(const volatile uint8_t *data, size_t length) {
    if (data == NULL && length != 0) {
        return 0;
    }
    uint8_t sum = 0;
    for (size_t index = 0; index < length; ++index) {
        sum = (uint8_t)(sum + data[index]);
    }
    return sum == 0;
}

void rustix_zero_explicit(void *memory, size_t length) {
    if (memory == NULL && length != 0) {
        return;
    }
    volatile uint8_t *bytes = (volatile uint8_t *)memory;
    for (size_t index = 0; index < length; ++index) {
        bytes[index] = 0;
    }
    __asm__ __volatile__("" : : "r"(memory) : "memory");
}
