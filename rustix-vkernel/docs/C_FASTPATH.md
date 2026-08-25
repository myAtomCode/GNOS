# C fastpath boundary

Rustix keeps ownership, synchronization, address validation, error handling,
syscalls, drivers, and scheduling in Rust. C is restricted to small
freestanding primitives whose complete input and output ranges are supplied by
Rust. The kernel does not link libc.

## Included primitives

| Primitive | Rust owner | Reason for C boundary |
| --- | --- | --- |
| Bitmap fill/count/scan | frame allocator lock | Tight word-at-a-time physical-frame hot path |
| `u64` sort and deduplicate | boot memory-map parser | Bounded in-place algorithm with no allocation or recursion |
| Internet checksum | network packet builder | Compact byte-order-aware checksum loop |
| IPv4 parser | network API | One bounded input slice and four-byte output |
| ACPI checksum | ACPI parser | Explicit volatile access to firmware-backed mappings |
| Explicit zero | kernel heap | Volatile stores prevent dead-store elimination |

These primitives cover the low-level fastpath responsibilities across memory,
boot, firmware, and networking. They intentionally do not make up 30% of total
source lines: doing that would move safe kernel policy into an unsafe language
without a performance benefit.

## ABI rules

- Every function uses the platform C ABI and fixed-width integer types.
- A non-zero length requires a non-null pointer to the entire stated range.
- C never stores a pointer after returning, allocates memory, calls back into
  Rust, or acquires a kernel lock.
- Rust wrappers construct all slices and clamp returned indices before the
  values reach safe Rust.
- The C compiler uses `-ffreestanding`, `-fno-builtin`, no red zone on x86_64,
  no unwind tables, no stack protector, and no libc.
- Both x86_64 and AArch64 builds use the same source and boot-time ABI self-test.

The boundary lives in `src/c_fastpath.rs`; its matching declarations and
implementation live in `c/fastpath.h` and `c/fastpath.c`.
