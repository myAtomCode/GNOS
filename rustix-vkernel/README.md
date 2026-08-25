# Rustix Microkernel

这是 Rustix 当前唯一的内核实现，采用微内核结构组织核心、服务和应用。

- 微内核核心：任务表、端点、IPC、服务注册
- 能力型 IPC：PID 绑定、代际句柄、单接收者权限、有界 O(1) 队列和页 loan 零拷贝传输
- 用户态服务：`console`、`vfs`、`proc`
- 用户态程序：`/bin/hello`、`/bin/game`
- 一个可以交互的 shell
- 一个 BIOS VGA 图形桌面，带任务栏、桌面图标、圆角窗口和鼠标交互
- Linux 风格的路径规范化、完整 errno 编号与原始 syscall `-errno` 返回约定
- 进程级文件描述符表及 `read/write/openat/close/statx/getdents64/lseek`
- Linux 内存与 I/O 调用：`mmap/munmap/mprotect/brk`、`pipe2/dup3/fcntl/ioctl`、`poll/select/epoll`
- Linux socket、时间、随机数、系统信息 syscall 与统一受检用户缓冲区
- 最多 8 项的批量 syscall、固定深度异步文件 I/O 与用户地址生命周期隔离
- 4 KiB 数据页缓存、顺序预读、脏页写回，以及 ACPI SRAT 驱动的 NUMA 本地优先分配
- 固定容量任务表、稳定 IPC opcode 和无堆分配同步原语
- 高半区内核、独立用户页表、NX/W^X、SMEP/SMAP 和带守卫页的内核栈
- 受检的 `copy_to_user` / `copy_from_user`、带守卫页的用户栈和异常 IST 栈
- x86_64 IDT、CPU 异常、LAPIC timer IRQ、ACPI MADT/IOAPIC 初始化
- ACPI RSDT/XSDT/MADT 处理器拓扑、xAPIC INIT/SIPI 多核启动和 GS per-CPU 数据
- IRQ-safe ranked spinlock、精确中断状态恢复、递归/逆序锁检查
- 单调 PID、PPID 父子关系、PID 1 孤儿收养、僵尸状态和显式 `wait` 回收
- Linux 风格 futex wait/wake、信号中断等待与值变化 `EAGAIN` 检查
- 普通/FIFO/RR 调度类别、nice、实时优先级、CPU affinity 与 per-CPU O(1) 位图运行队列
- 固定容量睡眠队列、通用等待队列，以及一次性/周期内核定时器
- TSC、HPET 与 APIC timer 校准，以及稳定单调时钟回退链
- 4 MiB NX 内核堆、合并空闲块、可恢复 OOM 与致命 OOM 停机处理
- LAPIC 时钟驱动的抢占式轮转调度、每线程独立守卫内核栈和完整通用寄存器上下文
- 独立 CR3 用户地址空间、x86_64 `syscall/sysretq`（兼容 `int 0x80`）与 AArch64 `svc/eret` 入口
- 跨 CR3 Ring 3 IPC：阻塞接收、受检用户复制、能力预校验、服务唤醒与恶意句柄拒绝
- ELF64 静态/PIE 装载、`arch_prctl` FS-base TLS、随机化基础 VDSO 与 SysV auxv
- 固定版本 musl 1.2.5 工具链、静态 BusyBox 1.36.1 `ash` 和基础 shell 脚本启动回归

代码分层和安全约束见 `docs/ARCHITECTURE.md`，公开接口见 `docs/INTERFACES.md`。

底层 C fastpath 仅用于页帧位图、内存图排序、网络/ACPI 校验和、IPv4 解析和安全清零；
所有权、长度校验、锁与错误处理仍由 Rust 控制，内核不链接 libc。ABI 与安全约束见
`docs/C_FASTPATH.md`。启动日志中的 `[c-fastpath] ABI/self-test=ok` 表示 Rust/C 边界自检通过。

## 运行

```bash
./build_bios.sh
./run_qemu.sh headless
```

`build_bios.sh` 默认调用 `userland/build_busybox.sh`。首次构建会下载并校验 musl 1.2.5 与
BusyBox 1.36.1 源码，生成静态 `build/userland/busybox`；后续构建直接复用该产物。

GUI/shell 兼容监视器体积超过 BIOS 低端暂存区，使用不受该限制的 Multiboot2 ISO：

```bash
./build_iso.sh gui
./run_qemu.sh gui
```

当前已验证 BIOS 软盘和 GRUB/Multiboot2 两条启动链路：

- 构建脚本：`./build_bios.sh`（启动隔离微内核；GUI 使用 `./build_iso.sh gui`）
- 产物：`build/rustix-vkernel-bios.img`
- QEMU 微内核：`qemu-system-x86_64 -smp 4 -cpu max -drive format=raw,file=build/rustix-vkernel-bios.img,if=floppy -boot a -serial stdio -display none -monitor none -m 512M`
- 兼容监视器图形交互：运行 `./build_iso.sh gui && ./run_qemu.sh gui`；
  图形窗口支持键盘、鼠标、窗口聚焦和拖拽，shell 同时回显到串口
- 串口自检：`ucopy`、`uguard`、`IDT`、`IST-guard`、`LAPIC`、`IOAPIC` 和
  `timer-irq` 应显示为 `ok`；时钟会按 `invariant-tsc -> hpet -> apic-timer` 回退
- 调度自检：`[sched] preempt=ok ... ring3=ok ... fork-wait=ok exec=ok signal=ok
  futex=ok policy=ok ipc-user=ok ipc-deny=ok ptable=ok reclaim=ok`；所有结果都来自真实
  frame/CR3 切换和 Ring 3 syscall，不再预置
- 文件 ABI 自检：串口先输出 `[fd] abi=ok`，覆盖文件写读、文件偏移、statx 和目录枚举
- syscall ABI 自检：串口输出 `[syscall] abi=ok`，覆盖时间、随机数、系统信息、原始
  `-errno` 返回以及用户指针的长度、权限、溢出和跨页原子校验
- 批量/异步 I/O 自检：串口输出 `[batch/aio] abi=ok`，覆盖两项批量查询和异步文件读的
  submit/reap 生命周期
- ELF ABI 自检：Ring 3 通过 `execve(59)` 加载 PIE，串口输出 `[elf] abi=ok`；覆盖 ELF64
  program headers、随机主程序/解释器基址、随机用户栈、`argc/argv/envp/auxv` 和 `PT_INTERP`
- 静态程序自检：PIE 再通过 `execve(59)` 加载无解释器的 Rust/C freestanding ABI 镜像，输出
  `[static] abi=ok`；覆盖 `AT_BASE=0`、`AT_SYSINFO_EHDR`、VDSO 时钟入口和线程 FS-base TLS 继承
- musl/BusyBox 自检：静态程序 fork 后由子进程执行 `/bin/busybox sh /etc/init.d/rcS`；脚本覆盖
  变量赋值/展开、`if`/`test`、`for`、`echo` 和 `printf`，成功输出 `[sh] musl`、
  `[sh] busybox`、`[sh] shell` 与 `[busybox] abi=ok`
- IPC ABI：v1 提供动态 `endpoint_create/service_lookup/call/receive/reply`；回复权限为绑定
  服务 PID 的一次性 token，成功使用或任一参与进程退出后失效
- 多核自检：`[acpi/smp]` 中在线数应与 QEMU `-smp` 数量一致，`per-cpu` 和
  `irq-lock` 应显示为 `ok`

- Multiboot2 headless 构建：`./build_iso.sh headless`
- Multiboot2 产物：`build/rustix-vkernel.iso`
- Multiboot2 运行：`qemu-system-x86_64 -smp 4 -cdrom build/rustix-vkernel.iso -boot d -serial stdio -display gtk -m 512M`
- Multiboot2 GUI 构建：`./build_iso.sh gui`
- Multiboot2 GUI 产物：`build/rustix-vkernel-gui.iso`
- Multiboot2 GUI 运行：`./run_qemu.sh gui`
- UEFI：已实现标准 `EFI_MEMORY_DESCRIPTOR` 交接映射解析；PE/COFF EFI 加载器不在当前仓库内

从仓库任意目录直接运行：

```bash
qemu-system-x86_64 -smp 4 -cpu max -cdrom /home/my/ai-venv/rustix/build/rustix-vkernel-gui.iso -boot d -serial stdio -display gtk -vga std -m 512M
```

## 兼容监视器 Shell 命令

以下命令仅在 `RUSTIX_COMPAT_MONITOR=1` 构建中提供；使用 `RUSTIX_COMPAT_MONITOR=0` 时不启动 Ring 0 shell。

- `help`
- `services`
- `ps`
- `apps`
- `ls /path`
- `cat /path`
- `mkfile /path/to/file.txt`（创建文件；也支持 `touch /path/to/file.txt`）
- `mkdir /path/to/directory`（创建文件夹）
- `exec /bin/hello arg1 arg2`
- `exec-bg /bin/hello`（执行后保留为僵尸，供生命周期调试）
- `wait [pid|all]`
- `futexdemo`
- `waitqdemo`
- `timerdemo`
- `sleep <milliseconds>`
- `nice <pid> <-20..19>`
- `chrt <pid> <other|fifo|rr> [priority]`
- `taskset <pid> <hex-mask>`
- `rebalance`
- `exec /bin/game`
- `ipc`
- `shutdown`

## GUI 右键菜单

- 在桌面空白处右键，可用中文菜单打开“文件”“命令提示符”或“设置”。
- 在文件管理器的文件列表内右键，可选择“新建文件夹”或“新建文件”。
- GUI 会创建 `NewFolder` / `NewFile.txt`，名称冲突时自动追加数字。

## 说明

这个版本现在已经是“可启动、可交互的微内核结构原型”。
它重点演示：

- 服务分离
- 进程通过 IPC 访问 `console` / `vfs`
- 程序通过 `proc` 服务执行
- BIOS -> protected mode -> long mode -> Rust 内核启动链
- VGA mode 13h 图形输出
- 圆角 GUI、任务栏、桌面图标与 shell 回显同屏显示
- PS/2 鼠标交互

当前能力型 IPC 已用于服务授权和控制消息传输，但 console/VFS 数据处理仍在内核中，
原图形 shell 因此明确作为 Ring 0 兼容调试监视器。独立 CR3 的微内核执行路径已经完成
双向用户态 VFS 策略请求，但完整桌面仍不是全部用户态隔离的成品。后续退役监视器的
顺序见 `docs/ARCHITECTURE.md` 的 Capability IPC foundation。

## 构建产物

所有 Cargo 中间文件统一写入 `target/`，可启动镜像和 ISO 统一写入 `build/`。
这两个目录均被 Git 忽略；运行 `./clean.sh` 可一次清除全部生成物。

## ARM64

ARM64 公共启动层通过板级模块选择 `QEMU virt` 或 Raspberry Pi 5。当前目标是先保证串口启动和交互，而不是直接复刻 x86 BIOS GUI。

### QEMU virt

```bash
./build_aarch64.sh
./run_qemu_aarch64.sh
```

- 构建产物：`build/rustix-vkernel-aarch64.bin`
- 已适配：`aarch64` 启动入口、PL011 串口输出/输入、shell 交互
- 当前降级：VGA 图形、PS/2 鼠标、RTL8139 网卡、PC Speaker
- 直接运行命令：
  `qemu-system-aarch64 -machine virt,gic-version=2 -cpu cortex-a72 -smp 4 -m 512M -nographic -kernel build/rustix-vkernel-aarch64.bin`

### Raspberry Pi 5

Pi 5 由官方 GPU 固件加载原始 AArch64 镜像，入口兼容固件的 EL2/EL1 状态，并接收 `x0` 中的 Flattened Device Tree。内核从 DTB 的 `/memory`、内存保留表和 `/reserved-memory` 建立物理内存图。

```bash
./build_rpi5.sh /path/to/bcm2712-rpi-5-b.dtb
```

- 输出目录：`build/rpi5/`
- 内核文件：`build/rpi5/kernel_2712.img`，链接和加载地址为 `0x80000`
- 固件配置：`build/rpi5/config.txt`
- 设备树：`build/rpi5/bcm2712-rpi-5-b.dtb`；省略脚本参数时会尝试从本机 `/boot/firmware` 或 `/boot` 复制
- 串口：BCM2712 调试 PL011，地址 `0x107d001000`，115200 8N1
- 系统计数器：BCM2712 1 MHz system timer，地址 `0x107d003000`
- 中断控制器：GIC-400 distributor `0x107fff9000`、CPU interface `0x107fffa000`
- 时钟中断：ARM non-secure physical timer PPI 30，默认 100 Hz
- 多核：最多 4 核；兼容固件预启动核，并通过 PSCI `CPU_ON` 启动休眠核
- 内存映射：前 16 GiB RAM 使用 Normal Write-Back/Inner-Shareable 属性，BCM2712/RP1 高地址窗口使用 Device-nGnRE 属性

把 `build/rpi5/` 中的文件复制到已有 Raspberry Pi 5 FAT 启动分区。该分区仍需包含官方固件文件（至少 `start4.elf`、`fixup4.dat` 及相关启动文件）。连接 Pi 5 三针调试 UART 后上电即可查看启动日志。
