# Memory subsystem

## Boot memory maps

All x86 boot paths call the Rust entry point as `_start(protocol, information)` and normalize
firmware records into bounded `MemoryRegion` values.

- BIOS uses `INT 15h, EAX=E820h` and a 24-byte extended E820 record.
- Multiboot2 uses magic `0x36d76289` and parses aligned type-6 memory-map tags.
- UEFI uses protocol `0x55454649` and the handoff structure below after `ExitBootServices`.

All inputs are size/count bounded before dereference. Normalization sorts every region boundary,
splits overlaps and lets bad/reserved firmware ranges override reclaimable/usable ranges. If the
fixed-capacity result cannot represent the complete map, initialization fails closed instead of
silently making an omitted reserved page allocatable.

```text
offset  size  field
0       8     magic = 0x525553544d454d31
8       4     version = 1
12      4     descriptor_size
16      4     descriptor_count
20      4     reserved
24      8     physical address of descriptors
```

UEFI descriptors use the standard 40-byte `EFI_MEMORY_DESCRIPTOR` prefix. The parser is ready,
but this repository does not yet contain a PE/COFF `.efi` loader, so UEFI boot itself remains a
separate milestone.

## Physical frames

The x86 bitmap covers its 4 GiB bootstrap/direct-map window; AArch64 covers the 16 GiB identity-map window used by BCM2712. A set bit is unavailable. Boot
initialization frees only firmware-usable ranges and then reserves low memory, the complete kernel
image, the early stack, boot information and all firmware-reserved overlaps. A second ownership
bitmap prevents reserved frames and double-freed frames from being released.

The allocator maintains up to eight NUMA node ranges, a per-node scan cursor and per-node free
counts. `allocate()` selects the calling CPU's node, scans that range first, then falls back across
other online nodes; explicit callers can use `allocate_on_node`. Local and fallback allocations are
reported in `FrameStats`. On x86, ACPI SRAT processor and memory affinity records are validated and
normalized into allocator nodes before AP startup, and each logical CPU is bound to its SRAT node.
Missing, incomplete or overlapping firmware topology fails closed to one node covering all usable
memory. AArch64 currently uses that UMA fallback until DT NUMA distance data is available.

## AArch64 paging

The firmware enters with translation disabled. The primary CPU clears two aligned translation tables, builds a 48-bit TTBR0 identity map, installs MAIR and TCR, invalidates the local TLB and enables instruction/data caches with `SCTLR_EL1.M/C/I`. Secondary CPUs reuse the completed tables after an acquire/release handoff.

QEMU virt maps the first 1 GiB as Device-nGnRE and its RAM block beginning at `0x40000000` as Normal Write-Back, inner-shareable memory. The Raspberry Pi 5 maps RAM below 16 GiB with the same cacheable attributes and maps the BCM2712/RP1 peripheral aperture from `0x10_00000000` through `0x1f_ffffffff` as Device-nGnRE. The DTB memory and reservation records still decide which Normal-memory frames may enter the allocator.

## x86_64 paging

`ActivePageTable` reads CR3 and supports 4 KiB map, translate, permission update and unmap
operations. It also provides checked contiguous `map_range`, allocator-backed `allocate_range`
and rollback-safe `release_range` operations. Empty P1/P2/P3 tables are reclaimed after unmap,
and `AddressSpace::destroy` releases allocator-owned user frames plus the complete lower-half page
table tree. Page-table mutation uses lock rank 5 and physical allocation rank 10. Writable leaf
mappings must be NX, enforcing W^X at the mapping API boundary.

The mapper can split bootstrap 2 MiB huge pages into 4 KiB tables. Boot enables EFER.NXE and
CR0.WP, then applies executable/read-only permissions to `.text` and NX permissions to
`.rodata`, `.data` and `.bss`. The kernel executes above `0xffffffff80000000`; physical memory
is available through the supervisor-only direct map at `0xffff800000000000`. Writable low and
direct-map aliases of kernel text are removed so W^X also holds across aliases.

Each user address space starts with an empty lower half and shares only supervisor high-half
mappings. User mappings must stay below `0x0000800000000000` and explicitly carry the USER bit.
SMEP and SMAP are enabled after CPUID feature checks; temporary user-memory access must use the
scoped `begin_user_access` guard.

`AddressSpace::copy_from_user` and `copy_to_user` never dereference a caller-supplied virtual
address in kernel mode. They reject the null page, arithmetic overflow and non-user ranges, walk
every spanned page, verify USER on every page-table level and verify WRITABLE for kernel-to-user
copies. Data is then copied through the supervisor-only physical direct map. This makes an invalid
pointer a normal `UserCopyError` instead of a recoverable kernel page fault and keeps SMAP enabled.

User stacks are allocated with an unmapped lower guard page. The double-fault IST uses a separate
32 KiB NX stack with its own unmapped guard page, while the original 64 KiB early kernel stack
retains its existing guard page. Boot self-tests a user copy that crosses a page boundary and
confirms that the user-stack guard remains unmapped.

SMP application processors receive independent 32 KiB NX kernel stacks and 32 KiB double-fault
IST stacks. Every stack has an unmapped lower guard page and is recorded in its per-CPU structure;
no AP publishes itself online until its GDT, TSS, IST, IDT and GS base are active.

The 64 KiB early kernel stack is high-mapped, NX, compiled without a red zone and preceded by an
unmapped guard page. LLVM strong stack protectors use a boot-randomized canary and halt through
`__stack_chk_fail` on corruption.

## Kernel heap and OOM

The global 4 MiB heap is eagerly mapped at `0xffff900000000000` with writable/NX supervisor pages.
Its address-ordered free list splits and coalesces regions; allocation headers are checked before
freeing, and released memory is cleared. Fallible kernel paths can use `try_allocate`, while Rust's
global allocation error handler prints request and heap statistics and halts safely. Boot tests
alignment, coalescing and a recoverable oversized OOM request.

The boot self-tests also map/release a multi-page range, verify intermediate page-table reclamation,
copy across a user-page boundary and destroy a populated disposable user address space.
The serial line reports each protection as `ok` only when its runtime test succeeds.

## x86 MMIO mappings

LAPIC, IOAPIC and HPET pages are split out of the bootstrap huge-page mappings and marked
write-through/cache-disabled on both identity and direct-map aliases before volatile MMIO access.
This avoids conflicting cache types for device registers while preserving the fast huge-page
direct map for normal RAM.
