# Rustix Microkernel interfaces

本文档描述当前微内核内部接口和 Linux 风格兼容边界。接口以固定容量、无隐式堆分配
和受检参数为原则；当前实现不是完整 Linux syscall ABI，也不承诺直接运行通用 Linux ELF。

## IPC opcodes

`src/ipc.rs` 中的 opcode 是服务间稳定编号。已有编号不得复用；新操作只能追加。

| Opcode | 名称 | 服务 |
| ---: | --- | --- |
| 1 | `write` | console |
| 2 | `read` | console |
| 3 | `pread64` | VFS |
| 4 | `pwrite64` | VFS |
| 5 | `getdents64` | VFS |
| 6 | `execve` | proc |
| 7 | `service_list` | kernel |
| 8 | `task_list` | kernel |
| 9 | `rtnetlink_getlink` | net |
| 10 | `icmp_echo` | net |
| 11 | `fork` | proc |
| 12 | `clone` | proc |
| 13 | `exit` | proc |
| 14 | `wait4` | proc |
| 15 | `rt_sigqueueinfo` | proc |
| 16 | `futex` | proc |
| 17 | `sched_setscheduler` | proc |
| 18 | `sched_setaffinity` | proc |
| 19 | `wait_queue` | proc |
| 20 | `timer_settime` | proc |

## IPC capability model

`IpcTable` provides eight endpoints, 64 capabilities, eight queued messages per endpoint and a
192-byte maximum inline payload. These are compile-time bounds; IPC performs no heap allocation.
Endpoint and capability handles encode a slot plus generation. A capability is also bound to its
subject PID, so copying or guessing another task's raw handle does not grant access.

Endpoint owners receive `send + receive + manage`; delegated tasks can receive only `send`.
Receive rights are never duplicated, preventing multiple consumers from racing a server queue.
Queues are FIFO and O(1). Full queues, oversized messages, stale generations, wrong caller PIDs and
rights mismatches return explicit errors without truncation or fallback dispatch.

IPC ABI v2 provides `ipc_loan_send` (`0x1106`) and `ipc_loan_map` (`0x1107`). Loan send accepts a
send capability, page-aligned source, length up to eight pages and opcode. Success transfers page
ownership away from the sender and returns the message ID plus loan handle. Receive returns the
loan handle in `r10`; the endpoint owner maps it at an unmapped page-aligned address. Mapping is
read-only unless bit 0 of the map flags is set. The queued object contains only the descriptor, so
payload transfer does not copy through the kernel.

Current `SysApi` requests belong to the Ring 0 compatibility monitor. Separately, the x86 execution
scheduler uses the same capability rules for an isolated two-way Ring 3 VFS policy exchange. The
monitor interface must not be described as a user-space server ABI.

### x86_64 Ring 3 IPC ABI v2

稳定 ABI 使用 `syscall`/`sysretq`，并保留 `int 0x80`/`iretq` 兼容入口。`syscall` 按
Linux x86_64 约定从 `rax` 读取编号、从 `rdi/rsi/rdx/r10/r8/r9` 读取参数；版本由
`IPC_ABI_INFO` 查询。兼容 `int 0x80` 路径继续用 `rcx` 传递和接收第四寄存器值。ABI 编号
只追加、不复用：

v1 的系统服务编号同样稳定：`1=VFS`；后续服务只能追加新编号。

| Syscall | Operation | Inputs | Success result |
| ---: | --- | --- | --- |
| `0x1100` | `IPC_ABI_INFO` | none | `rax=version`, `rdx=max inline payload`, `rcx=queue depth`, `r10=max loan pages`, `r8=max endpoints`, `r9=max services` |
| `0x1101` | `ENDPOINT_CREATE` | none | `rax=owner capability`, `rdx=endpoint id` |
| `0x1102` | `SERVICE_LOOKUP` | `rdi=service id` | `rax=caller-bound send capability` |
| `0x1103` | `IPC_CALL_SEND` | `rdi=send capability`, `rsi=buffer`, `rdx=length`, `r10=opcode`, `r8=reply owner capability` | `rax=request message id` |
| `0x1104` | `IPC_RECEIVE` | `rdi=owner capability`, `rsi=buffer`, `rdx=capacity` | `rax=length`, `rdx=sender PID`, `rcx=opcode`, `r10=loan handle`, `r8=message id`, `r9=reply token` |
| `0x1105` | `IPC_REPLY` | `rdi=reply token`, `rsi=buffer`, `rdx=length`, `r10=opcode` | `rax=reply message id` |
| `0x1106` | `IPC_LOAN_SEND` | `rdi=send capability`, `rsi=page-aligned source`, `rdx=length`, `r10=opcode` | `rax=message id`, `rdx=loan handle` |
| `0x1107` | `IPC_LOAN_MAP` | `rdi=loan handle`, `rsi=page-aligned destination`, `rdx=flags` | `rax=mapped length` |

`IPC_CALL_SEND` 要求调用者拥有回复端点，并为该请求生成 generation-tagged token。token
只允许请求目标服务的 PID 使用，成功 `IPC_REPLY` 后立即失效，不能转交或重放；回复直接
进入请求者端点，不向服务暴露新的 send capability。服务或客户端退出、端点关闭时，相关
未完成 token 一并吊销。

`IPC_RECEIVE` 在有效空端点上阻塞。所有发送和回复都先验证 capability/token，再读取用户
指针；随后至多通过 `copy_from_user` 复制 192 字节，并在接收者 CR3 中通过
`copy_to_user` 交付。失败遵循 Linux 原始 syscall 约定，在返回寄存器中编码 `-errno`
（范围 `-1..=-4095`，例如 `-EBADF`、`-EFAULT`、`-EMSGSIZE`、`-ENOSYS`）；不截断、
不降级到监视器路径。旧 syscall 15--17 仅为启动回归保留，不属于稳定 ABI。

### x86_64 file descriptor ABI

每个任务有 16 个描述符槽，用户任务初始获得 `0=stdin`、`1=stdout`、`2=stderr`。普通文件
描述符引用全局固定容量的 open-file-description；fork/clone 复制描述符表并增加引用计数，
因此父子任务共享文件偏移。`O_CLOEXEC` 描述符在 exec 时关闭，所有描述符在退出时回收。

| Linux syscall | Number | Behavior |
| --- | ---: | --- |
| `read` | `0` | 从 stdin 或可读文件读取并推进共享偏移 |
| `write` | `1` | 写 stdout/stderr 或可写文件，支持 `O_APPEND` |
| `open` | `2` | 兼容入口，按 `AT_FDCWD` 转交给 `openat` |
| `close` | `3` | 关闭描述符并释放 open-file-description 引用 |
| `fstat` | `5` | 输出 Linux x86_64 `struct stat` 基础字段 |
| `lseek` | `8` | 支持 `SEEK_SET`、`SEEK_CUR`、`SEEK_END` |
| `readv` / `writev` | `19` / `20` | 最多处理 8 个受检 `iovec` |
| `chmod` / `fchmod` | `90` / `91` | 修改内存文件或目录的权限位 |
| `chown` / `fchown` | `92` / `93` | root 修改内存文件或目录的 UID/GID |
| `umask` | `95` | 设置任务创建掩码并返回旧值 |
| `getdents64` | `217` | 输出 8 字节对齐的 `linux_dirent64` 和 `DT_DIR/DT_REG` |
| `openat` | `257` | 支持绝对路径、`AT_FDCWD` 和目录描述符相对路径 |
| `newfstatat` | `262` | 支持相对路径和 `AT_EMPTY_PATH` |
| `statx` | `332` | 输出 256 字节 Linux `statx`，支持 `AT_EMPTY_PATH` |

`openat` 当前支持 `O_RDONLY/O_WRONLY/O_RDWR`、`O_CREAT`、`O_EXCL`、`O_TRUNC`、`O_APPEND`、
`O_DIRECTORY` 和 `O_CLOEXEC`，并接受 x86_64 上无操作语义的 `O_LARGEFILE`。VFS 文件最大
2048 字节，目录一次最多枚举 24 个不同名称；
容量限制分别返回 `EFBIG`、`EMFILE`、`ENFILE` 或 `ENOSPC`，无效用户地址返回 `EFAULT`。
内核启动自检从真实 Ring 3 顺序覆盖文件创建、写入、seek、读回比较、statx、目录 openat、
getdents64 和 close，成功时串口输出 `[fd] abi=ok`。

### Batched syscalls and asynchronous file I/O

x86_64 实验 ABI 使用 `0x1200` (`batch`)、`0x1201` (`async_submit`) 和 `0x1202`
(`async_reap`)。`batch` 的 `rdi` 指向最多 8 个 64 字节条目，`rsi` 为条目数；每个条目依次是
`number`、六个参数和 `result`。内核先完整 copy-in 并验证全部编号，再执行无阻塞白名单中的
`getpid/getuid/getgid/getcpu/self_heartbeat`，最后一次性 copy-out；不支持的编号使整个批次返回
`ENOSYS`。

`async_submit` 的 `rdi` 指向五个 `u64` 字段：`operation`（0=read，1=write）、`fd`、`buffer`、
`length` 和显式文件 `offset`，成功返回所有者绑定的 token。队列深度为 8，每项最多 256 字节；
write 在提交时 copy-in，read 在完成槽中保留数据，内核不会跨 syscall 保存用户指针。
`async_reap(token, output, capacity)` 在未完成时返回 `EAGAIN`，完成 read 时才 copy-out，并在成功或
I/O 错误后释放槽位。进程退出会清理其全部请求。Ring 3 回归成功时输出 `[batch/aio] abi=ok`。

### Memory and I/O multiplexing

进程虚拟地址空间提供 Linux `mmap(9)`、`munmap(11)`、`mprotect(10)` 和 `brk(12)`；匿名映射使用
固定的 `0x6000_0000_0000` 区间、按页分配，并遵守 W^X。当前每个任务最多跟踪 16 个映射区域。
`MAP_ANONYMOUS` 是必需 flag，文件映射返回 `ENODEV`。

`pipe2(293)` 创建 256 字节固定容量的内核管道，`dup3(292)`、`fcntl(72)` 和 `ioctl(16)` 提供
描述符复制、`FD_CLOEXEC`/基本状态查询以及终端窗口大小查询。管道读写采用非阻塞语义，满/空时返回
`EAGAIN`。

`poll(7)`、`select(23)`、`epoll_create1(291)`、`epoll_ctl(233)` 和 `epoll_wait(232)` 支持控制台、
普通文件和管道的读写就绪查询。epoll 每个实例最多 8 个 watch；所有表均为固定容量，耗尽时返回
`EMFILE`、`ENFILE` 或 `ENOSPC`。

### Socket, time and system information

socket ABI 覆盖 `socket(41)`、`connect(42)`、`accept(43)`、`sendto(44)`、`recvfrom(45)`、
`shutdown(48)`、`bind(49)`、`listen(50)`、`getsockname(51)`、`getpeername(52)`、
`socketpair(53)`、`setsockopt(54)` 和 `getsockopt(55)`。固定容量 socket 表支持 `AF_UNIX` 和
`AF_INET` 的 stream/datagram 对象、本地监听连接以及最多 1024 字节接收缓冲；当前不提供外部 TCP/IP。

时间和系统信息 ABI 覆盖 `nanosleep(35)`、`uname(63)`、`gettimeofday(96)`、`sysinfo(99)`、
`time(201)`、`clock_gettime(228)`、`clock_nanosleep(230)`、`getcpu(309)` 和 `getrandom(318)`。
时钟值来自平台单调时钟，`clock_nanosleep` 支持相对时间和 `TIMER_ABSTIME`。`getrandom` 接受
`GRND_NONBLOCK`、`GRND_RANDOM` 和 `GRND_INSECURE`（后两者不能组合）；随机数由单调时钟与 CPU
编号扰动的内核 xorshift 状态产生，不作为密码学随机源。

所有 syscall 用户缓冲区统一经过 `validate_user_range`：先对长度执行 `usize/isize` 上限检查，再用
checked-add 拒绝地址溢出，在页表锁内逐页验证用户位和读写权限，完整范围通过后才执行拷贝，因此
跨入无效页不会留下部分写入。非法范围统一返回 `EFAULT`，超出具体 ABI 容量的长度返回 `EINVAL`、
`EMSGSIZE` 或对应资源错误。Ring 3 启动回归覆盖上述成功路径、未知 syscall 的 `ENOSYS`、原始
`-errno` 编码以及空指针、溢出、只读页、超长缓冲区和跨页失败；成功时输出 `[syscall] abi=ok`。

服务由内核启动清单创建：启动器统一分配 PID、独立地址空间、内核栈和接收端点，将 owner
capability、service ID、ABI version 分别放入初始 `rdi`、`rsi`、`rdx`，再原子加入固定容量
注册表。用户态没有任意注册 syscall，因此普通进程不能抢占系统 service ID；服务退出时
注册项与 IPC 资源由同一生命周期路径删除。客户端只通过 `SERVICE_LOOKUP` 动态取得权限。

## Application API

内置程序通过 `SysApi` 访问服务，不直接调用驱动。当前公开方法如下：

- 身份：`pid`、`ppid`、`tgid`、`pgid`、`sid`
- 控制台：`write`、`write_line`、`write_line_num`、`prompt`、`clear_screen`
- VFS：`read_file`、`read_path`、`write_file`
- 时间：`sleep_ms`，任务进入睡眠队列并通过架构定时中断唤醒

兼容监视器中的程序仍通过 `src/apps.rs` 的静态 Rust 入口运行。隔离 x86 执行路径提供 Linux
`execve(59)`：路径、argv 和 envp 使用受检用户复制，当前 ELF 镜像注册表发布
`/bin/elf-selftest`、`/bin/static-selftest`、`/bin/busybox` 及 `/lib/ld-rustix.so` 解释器。

ELF64 装载器接受 x86_64 `ET_EXEC`/`ET_DYN`，限制 program header 和映射页数量，验证所有文件与
内存范围、对齐、入口和 `PT_INTERP`，拒绝 W+X。`PT_LOAD` 按最终权限映射并对 `p_memsz-p_filesz`
零填充。PIE 主程序、PIE 解释器和初始栈分别随机化；初始栈遵守 SysV 16 字节对齐并包含
`argc/argv/envp`、`AT_PHDR/PHENT/PHNUM/PAGESZ/BASE/ENTRY/RANDOM/EXECFN/SYSINFO_EHDR` 等 auxv。
每次 exec 还会在独立区间随机映射只读可执行的基础 VDSO；其 ELF 入口是
`clock_gettime(228)` thunk，当前尚未提供共享时间数据页或动态符号表。内核只按
`PT_INTERP` 装载并进入用户态动态链接器，符号解析和重定位仍由解释器负责。启动回归从真实 Ring 3
调用 `execve`，解释器读取 `AT_ENTRY` 转交 PIE 主程序，成功输出 `[elf] abi=ok`；随后再次 exec
无解释器的 `ET_EXEC`，验证静态 Rust/C freestanding ABI 并输出 `[static] abi=ok`。

x86_64 TLS 基础 ABI 提供 `arch_prctl(158)` 的 `ARCH_SET_FS`/`ARCH_GET_FS`。FS base 只接受低半区
规范用户地址，GET 目标使用统一受检用户复制；fork/clone 继承该值，上下文切换写回
`IA32_FS_BASE`，成功 exec 将其清零。该接口可运行自行提供运行时和 syscall 封装的静态 Rust/C
程序。仓库还用 musl 1.2.5 静态链接精简 BusyBox 1.36.1；其 `ash` 从 VFS 打开
`/etc/init.d/rcS`，验证变量展开、条件、循环和内置命令后输出 `[busybox] abi=ok`。当前移植目标是
这组受控静态 applet；完整 glibc、动态 TLS 模型、pthread ABI 和任意 BusyBox 配置不在兼容范围内。

## Task and process API

`src/kernel/task.rs` 提供固定容量任务表。主要生命周期接口为：

- `spawn`、`fork`、`clone_task`、`execve`、`exit`、`wait4`
- `set_process_group`、`setsid`
- `set_signal_action`、`send_signal`、`deliver_signal`
- `futex_wait`、`futex_wake`
- `wait_queue_wait`、`wait_queue_wake`
- `sleep_for`、`arm_timer`、`cancel_timer`、`poll_timers`

PID/TID 单调分配。PID 1 收养孤儿且不可退出；僵尸任务只能由合法父任务回收。
普通退出和默认致命信号退出共用收养、通知和清理路径；无退出信号的 clone 线程退出时
自动释放，不会留下无法 `wait4` 的僵尸。信号会中断 futex、等待队列和睡眠。
等待队列按入队顺序 FIFO 唤醒，重复阻塞会返回错误；周期定时器累计所有已跨过的间隔。

## Scheduling API

调度类别为 `Normal`、`Fifo` 和 `RoundRobin`。普通任务 nice 范围为 `-20..19`，
实时优先级范围为 `1..99`。`set_affinity` 接受 64 位 CPU mask，并与在线 CPU 集合
求交；`rebalance` 只把可运行任务迁移到其 affinity 允许的最轻负载 CPU。

x86 BSP 另有实际执行调度器：timer IRQ 可返回另一线程的完整寄存器 frame，并同步切换
TSS `rsp0` 和 CR3。其受控 `syscall` 自检 ABI 覆盖 fork、共享 VM clone、exit/wait、
exec、sleep、signal frame/sigreturn、futex、nice、RR/FIFO 基础和 CPU0 affinity。每个 CPU
使用独立的 64 级位图运行队列，最高优先级选择和同级轮转为 O(1)。该 ABI
目前用于内核启动验证，不是稳定的 Linux syscall ABI。它与兼容监视器共享唯一任务表、
PID 分配器和生命周期状态；AP 调度、跨 CPU 迁移和 AArch64 上下文切换仍未实现。因此
AArch64 已具备 `svc` ABI 入口，但在 EL0 任务调度接入前文件系统调用返回 `ENOSYS`。

等待和定时器表均为固定容量。一次性睡眠定时器唤醒任务，周期定时器按截止时间重装；
x86_64 使用 `HLT`、AArch64 使用 `WFI` 等待中断，避免忙等。

## VFS and Linux errors

`VfsService` 提供规范化绝对路径上的读取、写入、创建和目录枚举。inode 元数据包含
POSIX mode、UID 和 GID；`open/openat` 创建文件时应用每任务 umask，`stat/fstat/statx`
返回实际权限和所有者。路径先经过 `src/linux/path.rs` 规范化，错误使用
`src/linux/errno.rs` 中的 Linux 风格 errno。

挂载命名空间包含根内存文件系统、`/tmp` tmpfs、`/dev` devfs、`/proc` procfs 和
`/sys` sysfs。devfs 提供 `null`、`zero`、`console`、`random`；procfs/sysfs
暴露挂载、文件系统、umask 和 cache 统计。常规文件读取进入保存真实数据的 4 KiB page cache，
并保留 512 字节 block 热度统计；缺页时从后备存储装页，跨页顺序读取会预取下一页。常规写入先
更新缓存并标脏，`fsync/fdatasync/sync` 将脏页写回，驱逐只选择空闲或干净槽；truncate 和直接
替换文件内容会失效对应 inode。`/proc/cache` 暴露 dirty、readahead 和 writeback 计数。
VFS 仍不宣称 ext4、xfs 或其他
磁盘文件系统兼容性。

## Block storage and ext filesystems

`src/storage.rs` 提供 512 字节扇区的 `BlockDevice`、固定容量内存块设备和轮询式异步请求队列。
请求使用 `IoToken` 提交、`poll` 完成、`reap` 回收；队列深度固定为 8，越界、未对齐和设备错误
均返回显式 `BlockError`。启动时 PCI class-code 探测 IDE、AHCI SATA 和 NVMe；ATA PIO 已支持
IDENTIFY、LBA28 单扇区读写，AHCI/NVMe 控制器完成 BAR/能力探测并在 DMA 队列尚未初始化时
安全返回 `Unsupported`。探测与自检结果同时暴露在 `/proc/storage` 和 `/sys/block/devices`。

`src/extfs.rs` 提供无分配的 `ExtFilesystem` 适配器：解析 ext2/ext4 superblock、组描述符、inode、
目录项和 extents，支持路径查找、stat、read/read_at，以及对已有 direct-block/extent 文件的
write_at 和 inode size 更新。启动自检使用内存块设备分别验证 ext2 与 ext4 extent 镜像；真实磁盘
挂载可通过同一 `BlockDevice` 接口接入，尚未声明 journal、间接块分配和在线 fsck 支持。

## Architecture contract

`src/arch.rs` 是架构无关契约，x86_64 与 AArch64 实现在各自平台文件中。新增平台能力
必须先加入该契约，并为两个架构提供实现或明确的安全降级。

x86_64 初始化 `EFER.SCE`、`STAR`、`LSTAR` 和 `FMASK`，快速入口切换到每 CPU 内核栈，
安全返回满足条件时使用 `sysretq`，调度切换或非规范返回地址则回退到 `iretq`。AArch64
安装 2 KiB 对齐的 `VBAR_EL1` 向量表，从 EL0 的 `svc` 入口保存 `x0..x30`、`SP_EL0`、
`ELR_EL1` 与 `SPSR_EL1`；系统调用号位于 `x8`，参数位于 `x0..x5`，结果位于 `x0`。

## Build outputs

- Cargo 中间文件：`target/`
- BIOS、ARM64 和 ISO 产物：`build/`
- 清理命令：`./clean.sh`

源码目录不得写入生成文件，构建脚本也不得在仓库根目录散落镜像、对象文件或日志。
