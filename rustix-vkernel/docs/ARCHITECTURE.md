# Rustix v-kernel architecture

The microkernel keeps policy in services and limits architecture-specific code to the platform
layer. User programs do not call drivers directly.

```text
apps -> SysApi -> IPC request -> console / VFS / proc / net service
                         |
                         +-> Linux errno and canonical path rules

kernel -> task table / service registry -> arch -> x86_64 or AArch64 hardware
```

## Source layout

- `src/kernel.rs`: boot orchestration, shell and service routing.
- `src/kernel/task.rs`: bounded PID allocation and task lifecycle.
- `src/linux/`: Linux-facing errno and path semantics.
- `src/boot/`: BIOS E820, Multiboot2 and UEFI memory-map normalization.
- `src/mm/`: bitmap physical allocation and architecture paging.
- `src/services.rs`: console and in-memory VFS services.
- `src/storage.rs`: bounded block devices and asynchronous request queues.
- `src/ipc.rs`: stable IPC opcodes and Linux-like operation names.
- `src/arch.rs`: architecture-neutral platform contract.
- `src/arch_x86_64.rs`, `src/arch_aarch64.rs`: architecture implementations.
- `src/arch/aarch64/board/`: QEMU virt and Raspberry Pi 5 UART, power and identity data.
- `src/x86_acpi.rs`: bounded RSDP/RSDT/XSDT, MADT and HPET discovery.
- `src/x86_interrupts.rs`: GDT/TSS, IDT, exceptions, `syscall/sysretq`, LAPIC, IOAPIC and timer IRQ dispatch.
- `src/kernel/scheduler.rs`: preemptive contexts, guarded per-thread kernel stacks and Ring 3 setup.
- `src/x86_time.rs`: HPET/TSC/APIC calibration and monotonic-clock selection.
- `src/x86_smp.rs`: INIT/SIPI trampoline, AP lifecycle and GS-based per-CPU state.
- `src/sync.rs`: allocation-free synchronization primitives.

Memory-map and paging details are documented in `docs/MEMORY.md`.

## AArch64 board boundary

The shared AArch64 entry parks secondary CPUs, normalizes an EL2 firmware entry to EL1, clears BSS and forwards the firmware DTB pointer to Rust. Board modules own fixed MMIO addresses, UART initialization policy, platform identity and the PSCI conduit. QEMU virt links at `0x40080000`; the Raspberry Pi 5 firmware image links at `0x80000` and is emitted as `kernel_2712.img`.

The flattened-device-tree boot protocol validates the FDT header and all structure/string ranges before use. RAM comes from root `memory` nodes. The FDT reservation map, `/reserved-memory` children and the DTB blob itself remain unavailable to the frame allocator.

The early translation tables use 1 GiB identity blocks. QEMU virt maps its sub-1-GiB MMIO window as Device-nGnRE and RAM from `0x40000000` as cacheable inner-shareable memory. BCM2712 maps up to 16 GiB of RAM and the `0x10_00000000` through `0x1f_ffffffff` peripheral aperture, which includes the debug PL011, system timer, GIC-400 and RP1 windows. All CPUs install the same TTBR0/MAIR/TCR configuration before entering Rust.

The GICv2 layer owns distributor and per-CPU interface setup, interrupt grouping, acknowledgement and EOI. QEMU's non-security GIC uses Group 0; the Raspberry Pi firmware path uses non-secure Group 1. The architectural physical timer raises PPI 30 at 100 Hz while the BCM2712 1 MHz system counter supplies the Pi monotonic clock. IRQ vectors preserve every general register before dispatch.

Secondary CPUs either arrive at the image entry and wait on the release word or are requested with PSCI `CPU_ON`. Each receives a private 64 KiB boot stack, activates the shared page tables, installs VBAR and its GIC CPU interface, then publishes itself online with release ordering. Lock rank and interrupt-disable tracking are per-CPU.

## Capability IPC foundation

The kernel IPC table uses fixed-capacity endpoint queues and caller-bound capabilities. Endpoint
and capability handles include non-wrapping generations; an exhausted slot is retired instead of
risking stale-handle reuse. Only an endpoint owner has receive/manage rights; delegated
capabilities are send-only.
Every send and receive validates the caller PID, generation and rights before accessing an O(1)
ring queue. Payloads and queue depth are bounded, allocation-free and fail closed on overflow.

IPC ABI v2 adds page loans for payloads up to eight pages. A loan send detaches page-aligned user
pages from the sender and queues only a generation-checked descriptor. The endpoint owner maps the
same physical frames with `ipc_loan_map`; no payload bytes cross a kernel bounce buffer. Failed
sends restore the original mapping, failed destination maps retain the loan for retry, and receiver
exit releases unclaimed frames.

Service registration now creates real endpoints. Existing `SysApi` service access must cross this
capability gate, and each server has an opcode allowlist. A direct-mapped PID capability cache keeps
the established-call path O(1) without bypassing validation. Boot tests cover forged and stale
handles, confused caller identities, receive-right escalation, oversized messages and full queues.

The x86 execution scheduler exposes versioned `endpoint_create`, `service_lookup`, `call`, `receive`
and `reply` syscalls. Its bounded launcher allocates the service PID, CR3, kernel stack and endpoint,
registers the service ID, then passes only the owner capability in the initial Ring 3 frame. Clients
contain no pre-injected handles: they create a reply endpoint and obtain a caller-bound send
capability through lookup. Each call mints a generation-tagged, one-shot reply token bound to the
target server PID; a successful reply consumes it, and process exit revokes it.

The isolated VFS client tests the ABI version, submits a forged capability that is rejected before
its user buffer is read, requests `/version`, and verifies `vfs:ok!\n` from the service in another
CR3. The server then attempts to replay its consumed reply token, which is also rejected. Boot
exposes the completed exchange as `ipc-user=ok` and both hostile paths as `ipc-deny=ok`.

The interactive GUI/shell and its original console/VFS objects remain a Ring 0 compatibility
monitor. They are not part of the isolated workload and must not be presented as user services.
The microkernel execution path now has isolated scheduling, address spaces, capability IPC,
blocking receive and a user-space VFS policy round trip. Retiring the compatibility monitor still
requires:

1. add timeout/cancellation to the checked send/receive syscalls;
2. move VFS, console and network policy into separate Ring 3 address spaces;
3. extend the bounded ELF loader's image registry with signed persistent binaries;
4. restart services on task failure;
5. add measured IPC fast paths and opt-in shared pages for bulk data.

The complete desktop should only be called fully isolated after step 3 removes direct service
objects from the compatibility monitor.

The default BIOS build does not start that monitor. It boots the microkernel scheduler, capability
IPC and isolated Ring 3 service workload, then leaves the bootstrap task in an interrupt wait loop.
`RUSTIX_COMPAT_MONITOR=1 ./build_bios.sh` explicitly opts into the legacy Ring 0 GUI/shell for
debugging. This keeps the compatibility surface out of the default runtime security boundary while
preserving it for migration work.

## Safety rules

- Kernel tables are fixed-capacity; no allocator is required during boot or IPC.
- Shared VFS state is accessed through `SpinMutex`, never through public `static mut` references.
- GUI state is serialized by its process-context lock. Display, keyboard, DMA, GDT/IDT and TSS
  storage uses `StaticCell` only behind the owning subsystem; RTL8139 control state uses a mutex
  and DMA ownership transitions use compiler memory fences.
- `InlineString` preserves valid UTF-8 and checks every capacity change.
- User paths are absolute, bounded, and canonicalized before VFS lookup.
- PID and endpoint allocation uses checked arithmetic.

The current spin mutex is intended for short, non-sleeping process-context sections. Interrupt
handlers must not acquire the VFS lock.

## x86 scheduler execution

The BSP LAPIC timer drives a real fixed-capacity preemptive scheduler. Interrupt and syscall entry
save all general registers plus the complete `iretq` tail; selecting another thread returns its
saved frame instead of the interrupted frame. Every non-boot thread has a guarded kernel stack, and
Ring 3 threads additionally own a lower-half address space, NX user stack and read-only executable
code mapping. A switch updates TSS `rsp0`, CR3 and `IA32_FS_BASE` before restoring the target
context, so `arch_prctl`-configured TLS follows each fork/clone thread and is reset by exec. BSP and
AP bootstrap paths enable x87/SSE2 before entering Rust. Each scheduler context also owns a
16-byte-aligned FXSAVE image; switches save/restore x87, MXCSR and XMM state, while fork/clone take a
fresh snapshot so libc state cannot leak between runnable tasks.

The boot workload validates the execution path rather than preset counters: a Ring 3 process forks
an independently cloned address space, the child exits with a status and the waiting parent reaps
it, then the parent replaces its image through Linux `execve(59)`. The bounded ELF64 loader maps a
randomized PIE and its `PT_INTERP` image, builds the SysV argument/environment/auxiliary-vector stack,
and enters the user-space interpreter. The new image performs a user-stack signal-frame
and `rt_sigreturn` round trip, creates a shared-VM clone, blocks it on a futex, wakes it, applies RR,
nice and CPU0 affinity, then execs an interpreter-free static ELF. Each image receives a separately
randomized VDSO through `AT_SYSINFO_EHDR`; the static image calls its clock thunk and verifies FS-base
TLS plus clone inheritance before entering the timer sleep loop. The scheduler reports success only
after each transition has occurred. With the static ABI proven, a child then execs the musl-linked
BusyBox image as `sh /etc/init.d/rcS`. The in-memory script exercises assignment and expansion,
`if`/`test`, `for`, `echo` and `printf`; `[busybox] abi=ok` is the end-to-end libc/shell marker.

The x86 execution scheduler uses independent per-CPU 64-level bitmap run queues. Enqueue, removal,
highest-priority selection and same-level rotation are O(1), while blocking and wake transitions
update queue membership directly instead of rescanning the thread table.

This execution scheduler currently runs on the x86 BSP. APs have per-CPU GDT/TSS/guarded boot stacks
but remain in their idle loop, so cross-CPU task migration is not yet active. AArch64 also retains
the task-table model without an equivalent exception-frame scheduler. The exception-frame scheduler
and compatibility monitor now share one IRQ-safe task table and monotonic PID allocator. Scheduler
contexts retain only execution state; PPID/TGID/PGID/SID and lifecycle state have a single owner.
Reaped and exec-replaced address spaces are deferred until their CR3 is inactive, then destroyed;
task exit also revokes owned IPC endpoints and delegated capabilities. Boot reports these invariants
as `ptable=ok` and `reclaim=ok`. Registered compatibility-monitor applications remain direct Rust
entries; isolated scheduler images use ELF.

## PID and child lifecycle

PID 0 is reserved and the first kernel task is PID 1. PIDs increase monotonically and are never
reused merely because a task exits. Every non-init task has a validated live parent; `ps` exposes
the resulting PPID. A task exit records its status and transitions to `Zombie`, but keeps its PID
and fixed task-table slot. Only its parent can release that slot through `wait(pid)` or `wait(any)`.
If a parent exits first, all live and zombie children are reparented to PID 1 so their statuses are
not lost. PID 1 itself cannot exit.

Normal exit and default-fatal signal exit share the same lifecycle transition, including timer
cleanup, orphan adoption and `SIGCHLD` notification. Clone threads without an exit signal are
released immediately instead of creating an unreapable zombie; process children retain their slot
and status until their parent waits.

Foreground `exec` waits and reaps immediately, matching a normal shell. `exec-bg` is a deterministic
lifecycle diagnostic: the program runs synchronously, then remains visible as a zombie until
`wait [pid|all]`. Boot self-tests parent validation, zombie retention, orphan adoption, wait
ownership, PID 1 protection, slot reuse after reap and monotonic PID allocation.

Generic wait queues and futex queues assign a monotonic enqueue order and wake matching tasks FIFO.
A task must be runnable before it can block, preventing duplicate queue entries and stale sleep
timers. One-shot sleep timers wake their owner, while periodic timers account for every elapsed
interval and rearm from the previous deadline to avoid drift. Timer polling runs during console idle
processing as well as explicit sleep waits.

## x86 interrupt and clock policy

The bootstrap CPU installs a full exception IDT before enabling interrupts. Fatal exceptions use
an allocation-free serial path, double faults switch to a guarded IST stack, and LAPIC handlers
always issue EOI. Firmware 8259 delivery is masked when ACPI reports a legacy PIC. IOAPIC entries
are discovered through MADT and initially masked because the existing keyboard, mouse and network
drivers remain polling-based; the LAPIC timer is the first enabled IRQ source.

HPET is discovered from ACPI and used to calibrate TSC and the divided LAPIC timer. PIT channel 2
is the calibration fallback when HPET is absent. Monotonic time prefers an invariant calibrated
TSC, then HPET, then the 1 kHz APIC tick counter. Boot waits for two timer interrupts before
reporting `timer-irq=ok`.

## SMP and lock order

MADT Local APIC and x2APIC entries are normalized into a bounded eight-CPU topology. The BSP
starts xAPIC-compatible APs one at a time through INIT/SIPI and a page-9 real-mode trampoline.
Each AP receives separate guarded kernel and double-fault stacks, GDT/TSS/IST state and a GS-base
pointer to its `PerCpu` record before publishing `online=true` with release ordering. The low
identity map is executable only during this serialized bootstrap window and is restored to NX
before normal interrupts are enabled.

`SpinMutex` remains for short process-only sections that are never touched by interrupt handlers.
`IrqSpinMutex<T, RANK>` is required for interrupt-shared state. It disables local interrupts before
contending, restores the previous IF/DAIF state after unlock, and records ranks per CPU. Locks must
be acquired in strictly increasing rank order; recursive, reverse-order, release-order and depth
violations stop the kernel instead of silently deadlocking. The physical frame allocator uses rank
10, and boot tests nested ranks 10 -> 20 plus interrupt-disable depth and exact IF restoration.
