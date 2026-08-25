#ifndef RUSTIX_FASTPATH_H
#define RUSTIX_FASTPATH_H

#include <stddef.h>
#include <stdint.h>

/* All pointer arguments may be NULL only when their associated length is zero. */
void rustix_bitmap_fill(uint64_t *words, size_t word_count, uint64_t value);
size_t rustix_bitmap_count_clear(const uint64_t *words, size_t word_count);
size_t rustix_bitmap_find_clear(const uint64_t *words, size_t word_count,
                                size_t start_word, unsigned int *bit_index);

/* Sorts in place and returns the number of unique values in the prefix. */
size_t rustix_sort_unique_u64(uint64_t *values, size_t count);

uint16_t rustix_internet_checksum(const uint8_t *data, size_t length);
int rustix_parse_ipv4(const uint8_t *text, size_t length, uint8_t output[4]);

/* Volatile access is required for firmware-backed physical mappings. */
int rustix_checksum8_is_zero(const volatile uint8_t *data, size_t length);

/* This store loop is deliberately resistant to dead-store elimination. */
void rustix_zero_explicit(void *memory, size_t length);

#endif
