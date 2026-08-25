pub type Pid = u32;
pub type Tid = u32;
pub type Signal = u8;

pub const MAX_TASKS: usize = 32;
const MAX_SIGNAL: usize = 32;
const MAX_TIMERS: usize = 32;
const MAX_CPUS: usize = super::sched::MAX_CPUS;
const ALL_CPUS: u64 = (1u64 << MAX_CPUS) - 1;

pub const SIGCHLD: Signal = 17;
pub const SIGTERM: Signal = 15;
pub const SIGKILL: Signal = 9;
pub const SIGUSR1: Signal = 10;

pub const CLONE_VM: u64 = 0x0000_0100;
pub const CLONE_FS: u64 = 0x0000_0200;
pub const CLONE_FILES: u64 = 0x0000_0400;
pub const CLONE_SIGHAND: u64 = 0x0000_0800;
pub const CLONE_THREAD: u64 = 0x0001_0000;
pub const WNOHANG: u32 = 0x0000_0001;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskKind {
    Kernel,
    Service,
    User,
    Shell,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskState {
    Ready,
    Running,
    Waiting,
    FutexWait(u64),
    WaitQueue(u64),
    ChildWait,
    IpcWait,
    Sleeping(u32, u64),
    Zombie(i32),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TimerPollResult {
    pub expirations: usize,
    pub woken: usize,
    pub rearmed: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TimerKind {
    WakeTask,
    NotifyTask,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KernelTimer {
    id: u32,
    owner: Pid,
    deadline_ns: u64,
    interval_ns: u64,
    kind: TimerKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchedulingPolicy {
    Normal,
    Fifo,
    RoundRobin,
}

impl SchedulingPolicy {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Normal => "other",
            Self::Fifo => "fifo",
            Self::RoundRobin => "rr",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskRelation {
    Fork,
    Clone(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalAction {
    pub handler: u64,
    pub flags: u64,
    pub mask: u64,
}

impl SignalAction {
    pub const fn ignored() -> Self {
        Self {
            handler: 0,
            flags: 0,
            mask: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalFrame {
    pub signum: Signal,
    pub handler: u64,
    pub restorer: u64,
    pub saved_ip: u64,
    pub saved_sp: u64,
    pub saved_flags: u64,
    pub blocked_mask: u64,
}

#[derive(Clone, Copy)]
pub struct Task {
    pub pid: Pid,
    pub tid: Tid,
    pub tgid: Pid,
    pub parent: Option<Pid>,
    pub pgid: Pid,
    pub sid: Pid,
    pub name: &'static str,
    pub kind: TaskKind,
    pub state: TaskState,
    pub clone_flags: u64,
    pub exit_signal: Option<Signal>,
    pub signal_mask: u64,
    pub pending_signals: u64,
    pub signal_actions: [SignalAction; MAX_SIGNAL + 1],
    pub signal_frame: Option<SignalFrame>,
    pub scheduling_policy: SchedulingPolicy,
    pub nice: i8,
    pub rt_priority: u8,
    pub cpu_affinity: u64,
    pub cpu: usize,
    pub runtime_ticks: u64,
    pub timer_expirations: u32,
    wait_order: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskError {
    PidExhausted,
    TableFull,
    ParentNotFound,
    TaskNotFound,
    InitCannotExit,
    AlreadyExited,
    InvalidSignal,
    InvalidCloneFlags,
    NoSignalPending,
    InvalidFutexAddress,
    FutexValueChanged,
    InvalidNice,
    InvalidPriority,
    InvalidAffinity,
    NoEligibleCpu,
    InvalidWaitQueue,
    TaskNotRunnable,
    WaitOrderExhausted,
    InvalidTimeout,
    TimerTableFull,
    TimerNotFound,
    TimerIdExhausted,
}

impl TaskError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::PidExhausted => "error: pid space exhausted",
            Self::TableFull => "error: task table is full (reap zombie children)",
            Self::ParentNotFound => "error: parent task does not exist",
            Self::TaskNotFound => "error: task does not exist",
            Self::InitCannotExit => "error: PID 1 cannot exit",
            Self::AlreadyExited => "error: task is already a zombie",
            Self::InvalidSignal => "error: invalid signal",
            Self::InvalidCloneFlags => "error: invalid clone flags",
            Self::NoSignalPending => "error: no deliverable signal is pending",
            Self::InvalidFutexAddress => "error: futex address must be nonzero and 4-byte aligned",
            Self::FutexValueChanged => "error: futex value changed (EAGAIN)",
            Self::InvalidNice => "error: nice must be between -20 and 19",
            Self::InvalidPriority => "error: real-time priority must be between 1 and 99",
            Self::InvalidAffinity => "error: CPU affinity mask has no online CPU",
            Self::NoEligibleCpu => "error: task has no eligible CPU",
            Self::InvalidWaitQueue => "error: wait queue key must be nonzero",
            Self::TaskNotRunnable => "error: task is not runnable",
            Self::WaitOrderExhausted => "error: wait queue order space exhausted",
            Self::InvalidTimeout => "error: timeout must be nonzero and not overflow",
            Self::TimerTableFull => "error: timer table is full",
            Self::TimerNotFound => "error: timer does not exist",
            Self::TimerIdExhausted => "error: timer id space exhausted",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitError {
    NoChild,
    WouldBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaitResult {
    pub pid: Pid,
    pub status: i32,
}

pub struct TaskTable {
    next_pid: Pid,
    next_timer_id: u32,
    next_wait_order: u64,
    init_pid: Option<Pid>,
    entries: [Option<Task>; MAX_TASKS],
    timers: [Option<KernelTimer>; MAX_TIMERS],
}

impl TaskTable {
    pub const fn new() -> Self {
        Self {
            next_pid: 1,
            next_timer_id: 1,
            next_wait_order: 1,
            init_pid: None,
            entries: [None; MAX_TASKS],
            timers: [None; MAX_TIMERS],
        }
    }

    fn reset(&mut self) {
        self.next_pid = 1;
        self.next_timer_id = 1;
        self.next_wait_order = 1;
        self.init_pid = None;
        self.entries.fill(None);
        self.timers.fill(None);
    }

    pub fn spawn(
        &mut self,
        parent: Option<Pid>,
        name: &'static str,
        kind: TaskKind,
    ) -> Result<Pid, TaskError> {
        self.spawn_related(parent, name, kind, TaskRelation::Fork)
    }

    pub fn fork(&mut self, parent: Pid) -> Result<Pid, TaskError> {
        let template = self
            .get_by_pid(parent)
            .filter(|task| !matches!(task.state, TaskState::Zombie(_)))
            .ok_or(TaskError::ParentNotFound)?;
        self.spawn_related(
            Some(parent),
            template.name,
            template.kind,
            TaskRelation::Fork,
        )
    }

    pub fn clone_task(
        &mut self,
        parent: Pid,
        name: &'static str,
        clone_flags: u64,
    ) -> Result<Pid, TaskError> {
        if clone_flags & CLONE_THREAD != 0
            && clone_flags & (CLONE_VM | CLONE_SIGHAND) != (CLONE_VM | CLONE_SIGHAND)
        {
            return Err(TaskError::InvalidCloneFlags);
        }
        self.spawn_related(
            Some(parent),
            name,
            TaskKind::User,
            TaskRelation::Clone(clone_flags),
        )
    }

    pub fn execve(&mut self, pid: Pid, path: &'static str) -> Result<(), TaskError> {
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        if matches!(task.state, TaskState::Zombie(_)) {
            return Err(TaskError::AlreadyExited);
        }
        task.name = path;
        task.kind = TaskKind::User;
        task.pending_signals = 0;
        task.signal_frame = None;
        Ok(())
    }

    fn spawn_related(
        &mut self,
        parent: Option<Pid>,
        name: &'static str,
        kind: TaskKind,
        relation: TaskRelation,
    ) -> Result<Pid, TaskError> {
        match (self.init_pid, parent) {
            (None, None) if kind == TaskKind::Kernel => {}
            (Some(_), Some(parent_pid)) if self.is_live(parent_pid) => {}
            _ => return Err(TaskError::ParentNotFound),
        }

        let slot = self
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(TaskError::TableFull)?;
        let pid = self.next_pid;
        self.next_pid = self
            .next_pid
            .checked_add(1)
            .ok_or(TaskError::PidExhausted)?;
        let (
            tgid,
            pgid,
            sid,
            clone_flags,
            exit_signal,
            signal_actions,
            signal_mask,
            scheduling_policy,
            nice,
            rt_priority,
            cpu_affinity,
            cpu,
        ) = match parent {
            Some(parent_pid) => {
                let parent_task = self
                    .get_by_pid(parent_pid)
                    .ok_or(TaskError::ParentNotFound)?;
                let clone_flags = match relation {
                    TaskRelation::Fork => 0,
                    TaskRelation::Clone(flags) => flags,
                };
                let tgid = if clone_flags & CLONE_THREAD != 0 {
                    parent_task.tgid
                } else {
                    pid
                };
                let exit_signal = if clone_flags & CLONE_THREAD != 0 {
                    None
                } else {
                    Some(SIGCHLD)
                };
                (
                    tgid,
                    parent_task.pgid,
                    parent_task.sid,
                    clone_flags,
                    exit_signal,
                    parent_task.signal_actions,
                    parent_task.signal_mask,
                    parent_task.scheduling_policy,
                    parent_task.nice,
                    parent_task.rt_priority,
                    parent_task.cpu_affinity,
                    parent_task.cpu,
                )
            }
            None => (
                pid,
                pid,
                pid,
                0,
                None,
                [SignalAction::ignored(); MAX_SIGNAL + 1],
                0,
                SchedulingPolicy::Normal,
                0,
                0,
                ALL_CPUS,
                0,
            ),
        };
        self.entries[slot] = Some(Task {
            pid,
            tid: pid,
            tgid,
            parent,
            pgid,
            sid,
            name,
            kind,
            state: TaskState::Ready,
            clone_flags,
            exit_signal,
            signal_mask,
            pending_signals: 0,
            signal_actions,
            signal_frame: None,
            scheduling_policy,
            nice,
            rt_priority,
            cpu_affinity,
            cpu,
            runtime_ticks: 0,
            timer_expirations: 0,
            wait_order: 0,
        });
        if self.init_pid.is_none() {
            self.init_pid = Some(pid);
        }
        Ok(pid)
    }

    pub fn set_state(&mut self, pid: Pid, state: TaskState) -> bool {
        let Some(task) = self.task_mut(pid) else {
            return false;
        };
        if matches!(task.state, TaskState::Zombie(_)) || matches!(state, TaskState::Zombie(_)) {
            return false;
        }
        task.state = state;
        task.wait_order = 0;
        true
    }

    pub fn futex_wait(
        &mut self,
        pid: Pid,
        address: u64,
        observed: u32,
        expected: u32,
    ) -> Result<(), TaskError> {
        validate_futex_address(address)?;
        if observed != expected {
            return Err(TaskError::FutexValueChanged);
        }
        let wait_order = self.allocate_wait_order(pid)?;
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        task.state = TaskState::FutexWait(address);
        task.wait_order = wait_order;
        Ok(())
    }

    pub fn futex_wake(&mut self, address: u64, maximum: usize) -> Result<usize, TaskError> {
        validate_futex_address(address)?;
        let mut woken = 0usize;
        while woken < maximum {
            let Some(index) = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, task)| {
                    task.as_ref()
                        .filter(|task| task.state == TaskState::FutexWait(address))
                        .map(|task| (index, task.wait_order))
                })
                .min_by_key(|(_, order)| *order)
                .map(|(index, _)| index)
            else {
                break;
            };
            let task = self.entries[index]
                .as_mut()
                .expect("futex waiter disappeared");
            task.state = TaskState::Ready;
            task.wait_order = 0;
            woken += 1;
        }
        Ok(woken)
    }

    pub fn wait_queue_wait(&mut self, pid: Pid, key: u64) -> Result<(), TaskError> {
        if key == 0 {
            return Err(TaskError::InvalidWaitQueue);
        }
        let wait_order = self.allocate_wait_order(pid)?;
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        task.state = TaskState::WaitQueue(key);
        task.wait_order = wait_order;
        Ok(())
    }

    pub fn wait_queue_wake(&mut self, key: u64, maximum: usize) -> Result<usize, TaskError> {
        if key == 0 {
            return Err(TaskError::InvalidWaitQueue);
        }
        let mut woken = 0usize;
        while woken < maximum {
            let Some(index) = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, task)| {
                    task.as_ref()
                        .filter(|task| task.state == TaskState::WaitQueue(key))
                        .map(|task| (index, task.wait_order))
                })
                .min_by_key(|(_, order)| *order)
                .map(|(index, _)| index)
            else {
                break;
            };
            let task = self.entries[index]
                .as_mut()
                .expect("wait-queue waiter disappeared");
            task.state = TaskState::Ready;
            task.wait_order = 0;
            woken += 1;
        }
        Ok(woken)
    }

    pub fn sleep_for(&mut self, pid: Pid, now_ns: u64, duration_ns: u64) -> Result<u32, TaskError> {
        let deadline_ns = checked_deadline(now_ns, duration_ns)?;
        self.ensure_runnable(pid)?;
        let timer_id = self.insert_timer(pid, deadline_ns, 0, TimerKind::WakeTask)?;
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        task.state = TaskState::Sleeping(timer_id, deadline_ns);
        task.wait_order = 0;
        Ok(timer_id)
    }

    pub fn arm_timer(
        &mut self,
        pid: Pid,
        now_ns: u64,
        delay_ns: u64,
        interval_ns: u64,
    ) -> Result<u32, TaskError> {
        let deadline_ns = checked_deadline(now_ns, delay_ns)?;
        if !self.is_live(pid) {
            return Err(TaskError::TaskNotFound);
        }
        self.insert_timer(pid, deadline_ns, interval_ns, TimerKind::NotifyTask)
    }

    pub fn cancel_timer(&mut self, pid: Pid, timer_id: u32) -> Result<(), TaskError> {
        let Some(slot) = self
            .timers
            .iter_mut()
            .find(|entry| entry.is_some_and(|timer| timer.id == timer_id && timer.owner == pid))
        else {
            return Err(TaskError::TimerNotFound);
        };
        *slot = None;
        if let Some(task) = self.task_mut(pid) {
            if matches!(task.state, TaskState::Sleeping(id, _) if id == timer_id) {
                task.state = TaskState::Ready;
            }
        }
        Ok(())
    }

    pub fn poll_timers(&mut self, now_ns: u64) -> TimerPollResult {
        let mut result = TimerPollResult::default();
        for index in 0..self.timers.len() {
            let Some(timer) = self.timers[index] else {
                continue;
            };
            if timer.deadline_ns > now_ns {
                continue;
            }

            let periods = if timer.interval_ns == 0 {
                1
            } else {
                now_ns.saturating_sub(timer.deadline_ns) / timer.interval_ns + 1
            };
            result.expirations = result
                .expirations
                .saturating_add(usize::try_from(periods).unwrap_or(usize::MAX));
            if timer.interval_ns == 0 {
                self.timers[index] = None;
            } else {
                let next_deadline = timer
                    .interval_ns
                    .checked_mul(periods)
                    .and_then(|elapsed| timer.deadline_ns.checked_add(elapsed));
                if let Some(next_deadline) = next_deadline {
                    self.timers[index] = Some(KernelTimer {
                        deadline_ns: next_deadline,
                        ..timer
                    });
                    result.rearmed += 1;
                } else {
                    self.timers[index] = None;
                }
            }

            let Some(task) = self.task_mut(timer.owner) else {
                self.timers[index] = None;
                continue;
            };
            task.timer_expirations = task
                .timer_expirations
                .saturating_add(u32::try_from(periods).unwrap_or(u32::MAX));
            if timer.kind == TimerKind::WakeTask
                && matches!(task.state, TaskState::Sleeping(id, _) if id == timer.id)
            {
                task.state = TaskState::Ready;
                result.woken += 1;
            }
        }
        result
    }

    pub fn next_timer_deadline(&self) -> Option<u64> {
        self.timers
            .iter()
            .flatten()
            .map(|timer| timer.deadline_ns)
            .min()
    }

    pub fn timer_count(&self) -> usize {
        self.timers.iter().flatten().count()
    }

    fn insert_timer(
        &mut self,
        owner: Pid,
        deadline_ns: u64,
        interval_ns: u64,
        kind: TimerKind,
    ) -> Result<u32, TaskError> {
        let slot = self
            .timers
            .iter()
            .position(Option::is_none)
            .ok_or(TaskError::TimerTableFull)?;
        let timer_id = self.next_timer_id;
        self.next_timer_id = self
            .next_timer_id
            .checked_add(1)
            .ok_or(TaskError::TimerIdExhausted)?;
        self.timers[slot] = Some(KernelTimer {
            id: timer_id,
            owner,
            deadline_ns,
            interval_ns,
            kind,
        });
        Ok(timer_id)
    }

    fn ensure_runnable(&self, pid: Pid) -> Result<(), TaskError> {
        let task = self.get_by_pid(pid).ok_or(TaskError::TaskNotFound)?;
        if matches!(task.state, TaskState::Zombie(_)) {
            return Err(TaskError::AlreadyExited);
        }
        if !matches!(task.state, TaskState::Ready | TaskState::Running) {
            return Err(TaskError::TaskNotRunnable);
        }
        Ok(())
    }

    fn allocate_wait_order(&mut self, pid: Pid) -> Result<u64, TaskError> {
        self.ensure_runnable(pid)?;
        let order = self.next_wait_order;
        self.next_wait_order = self
            .next_wait_order
            .checked_add(1)
            .ok_or(TaskError::WaitOrderExhausted)?;
        Ok(order)
    }

    pub fn set_nice(&mut self, pid: Pid, nice: i8) -> Result<(), TaskError> {
        if !(-20..=19).contains(&nice) {
            return Err(TaskError::InvalidNice);
        }
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        task.nice = nice;
        Ok(())
    }

    pub fn set_scheduler(
        &mut self,
        pid: Pid,
        policy: SchedulingPolicy,
        priority: u8,
    ) -> Result<(), TaskError> {
        if (policy == SchedulingPolicy::Normal && priority != 0)
            || (policy != SchedulingPolicy::Normal && !(1..=99).contains(&priority))
        {
            return Err(TaskError::InvalidPriority);
        }
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        task.scheduling_policy = policy;
        task.rt_priority = priority;
        Ok(())
    }

    pub fn set_affinity(
        &mut self,
        pid: Pid,
        requested_mask: u64,
        online_cpus: usize,
    ) -> Result<u64, TaskError> {
        let online_mask = cpu_mask(online_cpus);
        let effective_mask = requested_mask & online_mask;
        if effective_mask == 0 {
            return Err(TaskError::InvalidAffinity);
        }
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        task.cpu_affinity = effective_mask;
        if effective_mask & (1u64 << task.cpu) == 0 {
            task.cpu = effective_mask.trailing_zeros() as usize;
        }
        Ok(effective_mask)
    }

    pub fn account_tick(&mut self, pid: Pid) -> Result<(), TaskError> {
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        task.runtime_ticks = task.runtime_ticks.saturating_add(1);
        Ok(())
    }

    pub fn pick_next(&self, cpu: usize) -> Option<Pid> {
        if cpu >= MAX_CPUS {
            return None;
        }
        self.entries
            .iter()
            .flatten()
            .filter(|task| {
                matches!(task.state, TaskState::Ready | TaskState::Running)
                    && task.cpu_affinity & (1u64 << cpu) != 0
                    && task.cpu == cpu
            })
            .max_by(|left, right| compare_runnable(left, right))
            .map(|task| task.pid)
    }

    pub fn rebalance(&mut self, online_cpus: usize) -> Result<usize, TaskError> {
        let online_cpus = online_cpus.clamp(1, MAX_CPUS);
        let online_mask = cpu_mask(online_cpus);
        let mut loads = [0u16; MAX_CPUS];
        for task in self.entries.iter().flatten() {
            if matches!(task.state, TaskState::Ready | TaskState::Running)
                && task.cpu < online_cpus
                && task.cpu_affinity & (1u64 << task.cpu) != 0
            {
                loads[task.cpu] = loads[task.cpu].saturating_add(1);
            }
        }

        let mut migrations = 0;
        for entry in &mut self.entries {
            let Some(task) = entry.as_mut() else {
                continue;
            };
            if !matches!(task.state, TaskState::Ready | TaskState::Running) {
                continue;
            }
            let allowed = task.cpu_affinity & online_mask;
            if allowed == 0 {
                return Err(TaskError::NoEligibleCpu);
            }
            let target = least_loaded_cpu(allowed, &loads).ok_or(TaskError::NoEligibleCpu)?;
            let current_allowed = task.cpu < online_cpus && allowed & (1u64 << task.cpu) != 0;
            if !current_allowed || loads[task.cpu] > loads[target].saturating_add(1) {
                if current_allowed {
                    loads[task.cpu] = loads[task.cpu].saturating_sub(1);
                }
                task.cpu = target;
                loads[target] = loads[target].saturating_add(1);
                migrations += 1;
            }
        }
        Ok(migrations)
    }

    pub fn exit(&mut self, pid: Pid, status: i32) -> Result<(), TaskError> {
        if self.init_pid == Some(pid) {
            return Err(TaskError::InitCannotExit);
        }
        let index = self
            .entries
            .iter()
            .position(|task| task.is_some_and(|task| task.pid == pid))
            .ok_or(TaskError::TaskNotFound)?;
        let exiting = self.entries[index].ok_or(TaskError::TaskNotFound)?;
        if matches!(exiting.state, TaskState::Zombie(_)) {
            return Err(TaskError::AlreadyExited);
        }
        for timer in &mut self.timers {
            if timer.is_some_and(|timer| timer.owner == pid) {
                *timer = None;
            }
        }

        // PID 1 adopts both live and zombie orphans. Zombie slots remain owned
        // until init explicitly waits, so no exit status is silently discarded.
        let init_pid = self.init_pid.ok_or(TaskError::TaskNotFound)?;
        let mut adopted_zombie = false;
        for child in self.entries.iter_mut().flatten() {
            if child.parent == Some(pid) {
                child.parent = Some(init_pid);
                adopted_zombie |= matches!(child.state, TaskState::Zombie(_));
            }
        }

        if let (Some(parent), Some(signal)) = (exiting.parent, exiting.exit_signal) {
            if let Some(parent) = self.task_mut(parent) {
                parent.pending_signals |= 1u64 << signal;
            }
        }
        if adopted_zombie {
            if let Some(init) = self.task_mut(init_pid) {
                init.pending_signals |= 1u64 << SIGCHLD;
            }
        }

        if exiting.exit_signal.is_none() {
            self.entries[index] = None;
        } else {
            let task = self.entries[index]
                .as_mut()
                .ok_or(TaskError::TaskNotFound)?;
            task.state = TaskState::Zombie(status);
            task.signal_frame = None;
            task.wait_order = 0;
        }
        Ok(())
    }

    pub fn wait(&mut self, parent: Pid, target: Option<Pid>) -> Result<WaitResult, WaitError> {
        match self.wait4(parent, target, 0)? {
            Some(result) => Ok(result),
            None => Err(WaitError::WouldBlock),
        }
    }

    pub fn wait4(
        &mut self,
        parent: Pid,
        target: Option<Pid>,
        options: u32,
    ) -> Result<Option<WaitResult>, WaitError> {
        let mut has_child = false;
        let mut zombie = None;
        let parent_pgid = self.get_by_pid(parent).map(|task| task.pgid).unwrap_or(0);
        for (index, task) in self.entries.iter().enumerate() {
            let Some(task) = task else {
                continue;
            };
            if task.parent != Some(parent) {
                continue;
            }
            if let Some(pid) = target {
                if pid == 0 {
                    if task.pgid != parent_pgid {
                        continue;
                    }
                } else if pid != task.pid {
                    continue;
                }
            }
            if task.exit_signal.is_none() {
                continue;
            }
            has_child = true;
            if let TaskState::Zombie(status) = task.state {
                zombie = Some((
                    index,
                    WaitResult {
                        pid: task.pid,
                        status,
                    },
                ));
                break;
            }
        }
        let Some((index, result)) = zombie else {
            if options & WNOHANG != 0 && has_child {
                return Ok(None);
            }
            return Err(if has_child {
                WaitError::WouldBlock
            } else {
                WaitError::NoChild
            });
        };
        // Wait releases a waitable child; detached clone threads release on exit.
        self.entries[index] = None;
        Ok(Some(result))
    }

    pub fn set_process_group(&mut self, pid: Pid, pgid: Pid) -> Result<(), TaskError> {
        let new_pgid = if pgid == 0 { pid } else { pgid };
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        task.pgid = new_pgid;
        Ok(())
    }

    pub fn setsid(&mut self, pid: Pid) -> Result<Pid, TaskError> {
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        task.sid = pid;
        task.pgid = pid;
        Ok(pid)
    }

    pub fn set_signal_action(
        &mut self,
        pid: Pid,
        signal: Signal,
        action: SignalAction,
    ) -> Result<(), TaskError> {
        let signal = signal_index(signal)?;
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        task.signal_actions[signal] = action;
        Ok(())
    }

    pub fn send_signal(&mut self, pid: Pid, signal: Signal) -> Result<(), TaskError> {
        let signal_index = signal_index(signal)?;
        if self.init_pid == Some(pid) && matches!(signal, SIGKILL | SIGTERM) {
            return Err(TaskError::InitCannotExit);
        }
        let (interrupted_timer, terminated) = {
            let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
            if matches!(task.state, TaskState::Zombie(_)) {
                return Err(TaskError::AlreadyExited);
            }
            task.pending_signals |= 1u64 << signal_index;
            let interrupted_timer = match task.state {
                TaskState::Sleeping(timer_id, _) => Some(timer_id),
                _ => None,
            };
            let terminated = matches!(signal, SIGKILL | SIGTERM)
                && task.signal_actions[signal_index].handler == 0;
            if !terminated
                && matches!(
                    task.state,
                    TaskState::FutexWait(_)
                        | TaskState::WaitQueue(_)
                        | TaskState::ChildWait
                        | TaskState::IpcWait
                        | TaskState::Sleeping(_, _)
                )
            {
                task.state = TaskState::Ready;
                task.wait_order = 0;
            }
            (interrupted_timer, terminated)
        };
        if terminated {
            self.exit(pid, 128 + signal as i32)?;
        } else if let Some(timer_id) = interrupted_timer {
            if let Some(slot) = self
                .timers
                .iter_mut()
                .find(|entry| entry.is_some_and(|timer| timer.id == timer_id && timer.owner == pid))
            {
                *slot = None;
            }
        }
        Ok(())
    }

    pub fn deliver_signal(&mut self, pid: Pid) -> Result<SignalFrame, TaskError> {
        let task = self.task_mut(pid).ok_or(TaskError::TaskNotFound)?;
        let deliverable = task.pending_signals & !task.signal_mask;
        if deliverable == 0 {
            return Err(TaskError::NoSignalPending);
        }

        let signal_index = deliverable.trailing_zeros() as usize;
        let signal_bit = 1u64 << signal_index;
        let signal = signal_index as Signal;
        let action = task.signal_actions[signal_index];
        task.pending_signals &= !signal_bit;

        // This is a serialized Linux-style rt_sigframe. Real user-mode entry
        // would push it to the user stack; the prototype stores it in the task
        // table so the ABI contract is visible without unsafe user pointers.
        let frame = SignalFrame {
            signum: signal,
            handler: action.handler,
            restorer: 0xffff_ffff_8000_1000,
            saved_ip: 0x0040_0000 + task.pid as u64,
            saved_sp: 0x007f_ffff_f000 - (task.pid as u64 * 0x1000),
            saved_flags: 0x202,
            blocked_mask: task.signal_mask,
        };
        task.signal_frame = Some(frame);
        Ok(frame)
    }

    pub fn slots(&self) -> usize {
        self.entries.len()
    }

    pub fn active_count(&self) -> usize {
        self.entries.iter().flatten().count()
    }

    pub fn zombie_count(&self) -> usize {
        self.entries
            .iter()
            .flatten()
            .filter(|task| matches!(task.state, TaskState::Zombie(_)))
            .count()
    }

    pub fn get(&self, index: usize) -> Option<Task> {
        self.entries.get(index).copied().flatten()
    }

    pub fn get_by_pid(&self, pid: Pid) -> Option<Task> {
        self.entries
            .iter()
            .flatten()
            .find(|task| task.pid == pid)
            .copied()
    }

    fn is_live(&self, pid: Pid) -> bool {
        self.get_by_pid(pid)
            .is_some_and(|task| !matches!(task.state, TaskState::Zombie(_)))
    }

    fn task_mut(&mut self, pid: Pid) -> Option<&mut Task> {
        self.entries
            .iter_mut()
            .flatten()
            .find(|task| task.pid == pid)
    }
}

static SHARED_TASKS: crate::sync::IrqSpinMutex<TaskTable, 30> =
    crate::sync::IrqSpinMutex::new(TaskTable::new());
static TASK_SELF_TEST: crate::sync::SpinMutex<TaskTable> =
    crate::sync::SpinMutex::new(TaskTable::new());

#[derive(Clone, Copy)]
pub struct SharedTaskTable;

impl SharedTaskTable {
    pub const fn new() -> Self {
        Self
    }

    pub fn spawn(
        &self,
        parent: Option<Pid>,
        name: &'static str,
        kind: TaskKind,
    ) -> Result<Pid, TaskError> {
        SHARED_TASKS.lock().spawn(parent, name, kind)
    }

    pub fn fork(&self, parent: Pid) -> Result<Pid, TaskError> {
        SHARED_TASKS.lock().fork(parent)
    }

    pub fn clone_task(
        &self,
        parent: Pid,
        name: &'static str,
        flags: u64,
    ) -> Result<Pid, TaskError> {
        SHARED_TASKS.lock().clone_task(parent, name, flags)
    }

    pub fn execve(&self, pid: Pid, path: &'static str) -> Result<(), TaskError> {
        SHARED_TASKS.lock().execve(pid, path)
    }

    pub fn set_state(&self, pid: Pid, state: TaskState) -> bool {
        SHARED_TASKS.lock().set_state(pid, state)
    }

    pub fn futex_wait(
        &self,
        pid: Pid,
        address: u64,
        observed: u32,
        expected: u32,
    ) -> Result<(), TaskError> {
        SHARED_TASKS
            .lock()
            .futex_wait(pid, address, observed, expected)
    }

    pub fn futex_wake(&self, address: u64, maximum: usize) -> Result<usize, TaskError> {
        SHARED_TASKS.lock().futex_wake(address, maximum)
    }

    pub fn wait_queue_wait(&self, pid: Pid, key: u64) -> Result<(), TaskError> {
        SHARED_TASKS.lock().wait_queue_wait(pid, key)
    }

    pub fn wait_queue_wake(&self, key: u64, maximum: usize) -> Result<usize, TaskError> {
        SHARED_TASKS.lock().wait_queue_wake(key, maximum)
    }

    pub fn sleep_for(&self, pid: Pid, now: u64, duration: u64) -> Result<u32, TaskError> {
        SHARED_TASKS.lock().sleep_for(pid, now, duration)
    }

    pub fn arm_timer(
        &self,
        pid: Pid,
        now: u64,
        delay: u64,
        interval: u64,
    ) -> Result<u32, TaskError> {
        SHARED_TASKS.lock().arm_timer(pid, now, delay, interval)
    }

    pub fn cancel_timer(&self, pid: Pid, timer_id: u32) -> Result<(), TaskError> {
        SHARED_TASKS.lock().cancel_timer(pid, timer_id)
    }

    pub fn poll_timers(&self, now: u64) -> TimerPollResult {
        SHARED_TASKS.lock().poll_timers(now)
    }

    pub fn next_timer_deadline(&self) -> Option<u64> {
        SHARED_TASKS.lock().next_timer_deadline()
    }

    pub fn timer_count(&self) -> usize {
        SHARED_TASKS.lock().timer_count()
    }

    pub fn set_nice(&self, pid: Pid, nice: i8) -> Result<(), TaskError> {
        SHARED_TASKS.lock().set_nice(pid, nice)
    }

    pub fn set_scheduler(
        &self,
        pid: Pid,
        policy: SchedulingPolicy,
        priority: u8,
    ) -> Result<(), TaskError> {
        SHARED_TASKS.lock().set_scheduler(pid, policy, priority)
    }

    pub fn set_affinity(&self, pid: Pid, mask: u64, online_cpus: usize) -> Result<u64, TaskError> {
        SHARED_TASKS.lock().set_affinity(pid, mask, online_cpus)
    }

    pub fn account_tick(&self, pid: Pid) -> Result<(), TaskError> {
        SHARED_TASKS.lock().account_tick(pid)
    }

    pub fn pick_next(&self, cpu: usize) -> Option<Pid> {
        SHARED_TASKS.lock().pick_next(cpu)
    }

    pub fn rebalance(&self, online_cpus: usize) -> Result<usize, TaskError> {
        SHARED_TASKS.lock().rebalance(online_cpus)
    }

    pub fn exit(&self, pid: Pid, status: i32) -> Result<(), TaskError> {
        SHARED_TASKS.lock().exit(pid, status)
    }

    pub fn wait(&self, parent: Pid, target: Option<Pid>) -> Result<WaitResult, WaitError> {
        SHARED_TASKS.lock().wait(parent, target)
    }

    pub fn wait4(
        &self,
        parent: Pid,
        target: Option<Pid>,
        options: u32,
    ) -> Result<Option<WaitResult>, WaitError> {
        SHARED_TASKS.lock().wait4(parent, target, options)
    }

    pub fn set_process_group(&self, pid: Pid, pgid: Pid) -> Result<(), TaskError> {
        SHARED_TASKS.lock().set_process_group(pid, pgid)
    }

    pub fn setsid(&self, pid: Pid) -> Result<Pid, TaskError> {
        SHARED_TASKS.lock().setsid(pid)
    }

    pub fn set_signal_action(
        &self,
        pid: Pid,
        signal: Signal,
        action: SignalAction,
    ) -> Result<(), TaskError> {
        SHARED_TASKS.lock().set_signal_action(pid, signal, action)
    }

    pub fn send_signal(&self, pid: Pid, signal: Signal) -> Result<(), TaskError> {
        SHARED_TASKS.lock().send_signal(pid, signal)
    }

    pub fn deliver_signal(&self, pid: Pid) -> Result<SignalFrame, TaskError> {
        SHARED_TASKS.lock().deliver_signal(pid)
    }

    pub fn slots(&self) -> usize {
        SHARED_TASKS.lock().slots()
    }

    pub fn active_count(&self) -> usize {
        SHARED_TASKS.lock().active_count()
    }

    pub fn zombie_count(&self) -> usize {
        SHARED_TASKS.lock().zombie_count()
    }

    pub fn get(&self, index: usize) -> Option<Task> {
        SHARED_TASKS.lock().get(index)
    }

    pub fn get_by_pid(&self, pid: Pid) -> Option<Task> {
        SHARED_TASKS.lock().get_by_pid(pid)
    }
}

fn signal_index(signal: Signal) -> Result<usize, TaskError> {
    let signal = signal as usize;
    if signal == 0 || signal > MAX_SIGNAL {
        Err(TaskError::InvalidSignal)
    } else {
        Ok(signal)
    }
}

fn validate_futex_address(address: u64) -> Result<(), TaskError> {
    if address == 0 || address & 3 != 0 {
        Err(TaskError::InvalidFutexAddress)
    } else {
        Ok(())
    }
}

fn checked_deadline(now_ns: u64, duration_ns: u64) -> Result<u64, TaskError> {
    if duration_ns == 0 {
        return Err(TaskError::InvalidTimeout);
    }
    now_ns
        .checked_add(duration_ns)
        .ok_or(TaskError::InvalidTimeout)
}

fn cpu_mask(online_cpus: usize) -> u64 {
    let count = online_cpus.clamp(1, MAX_CPUS);
    (1u64 << count) - 1
}

fn least_loaded_cpu(mask: u64, loads: &[u16; MAX_CPUS]) -> Option<usize> {
    (0..MAX_CPUS)
        .filter(|cpu| mask & (1u64 << cpu) != 0)
        .min_by_key(|cpu| (loads[*cpu], *cpu))
}

fn compare_runnable(left: &Task, right: &Task) -> core::cmp::Ordering {
    let left_class = u8::from(left.scheduling_policy != SchedulingPolicy::Normal);
    let right_class = u8::from(right.scheduling_policy != SchedulingPolicy::Normal);
    left_class
        .cmp(&right_class)
        .then_with(|| left.rt_priority.cmp(&right.rt_priority))
        .then_with(|| right.nice.cmp(&left.nice))
        .then_with(|| right.runtime_ticks.cmp(&left.runtime_ticks))
        .then_with(|| right.pid.cmp(&left.pid))
}

#[inline(never)]
pub fn lifecycle_self_test() -> bool {
    let mut tasks = TASK_SELF_TEST.lock();
    tasks.reset();
    let Ok(init) = tasks.spawn(None, "init-test", TaskKind::Kernel) else {
        return false;
    };
    let Ok(parent) = tasks.spawn(Some(init), "parent-test", TaskKind::Shell) else {
        return false;
    };
    let Ok(child) = tasks.spawn(Some(parent), "child-test", TaskKind::User) else {
        return false;
    };
    let Ok(grandchild) = tasks.spawn(Some(child), "orphan-test", TaskKind::User) else {
        return false;
    };
    let Ok(thread) = tasks.clone_task(
        parent,
        "thread-test",
        CLONE_VM | CLONE_SIGHAND | CLONE_THREAD,
    ) else {
        return false;
    };
    if tasks.get_by_pid(thread).map(|task| task.tgid) != Some(parent)
        || tasks
            .get_by_pid(thread)
            .and_then(|task| task.exit_signal)
            .is_some()
        || tasks.execve(parent, "/bin/hello").is_err()
    {
        return false;
    }
    if tasks.exit(child, 7).is_err()
        || !matches!(
            tasks.get_by_pid(child).map(|task| task.state),
            Some(TaskState::Zombie(7))
        )
        || tasks.get_by_pid(grandchild).and_then(|task| task.parent) != Some(init)
        || tasks.wait(parent, Some(grandchild)) != Err(WaitError::NoChild)
    {
        return false;
    }
    let Ok(reaped) = tasks.wait(parent, Some(child)) else {
        return false;
    };
    if reaped
        != (WaitResult {
            pid: child,
            status: 7,
        })
        || tasks.get_by_pid(child).is_some()
        || tasks.exit(init, 0) != Err(TaskError::InitCannotExit)
    {
        return false;
    }
    let handler = SignalAction {
        handler: 0x401000,
        flags: 0,
        mask: 0,
    };
    if tasks.set_signal_action(parent, SIGUSR1, handler).is_err()
        || tasks.send_signal(parent, SIGUSR1).is_err()
        || tasks.deliver_signal(parent).map(|frame| frame.signum) != Ok(SIGUSR1)
        || tasks.set_process_group(parent, 0).is_err()
        || tasks.setsid(parent).map(|sid| sid) != Ok(parent)
    {
        return false;
    }
    if tasks.set_nice(parent, -5).is_err()
        || tasks
            .set_scheduler(thread, SchedulingPolicy::RoundRobin, 20)
            .is_err()
        || tasks.set_affinity(parent, 0b11, 2) != Ok(0b11)
        || tasks.set_affinity(thread, 0b10, 2) != Ok(0b10)
        || tasks.rebalance(2).is_err()
        || tasks.pick_next(1) != Some(thread)
        || tasks.account_tick(thread).is_err()
        || tasks.get_by_pid(thread).map(|task| task.runtime_ticks) != Some(1)
        || tasks.futex_wait(thread, 0x1000, 7, 7).is_err()
        || tasks.futex_wake(0x1000, 1) != Ok(1)
        || tasks.get_by_pid(thread).map(|task| task.state) != Some(TaskState::Ready)
        || tasks.futex_wait(thread, 0x1000, 8, 7) != Err(TaskError::FutexValueChanged)
    {
        return false;
    }
    if tasks.wait_queue_wait(thread, 0x44).is_err()
        || tasks.wait_queue_wait(parent, 0x44).is_err()
        || tasks.wait_queue_wait(thread, 0x45) != Err(TaskError::TaskNotRunnable)
        || tasks.wait_queue_wake(0x44, 1) != Ok(1)
        || tasks.get_by_pid(thread).map(|task| task.state) != Some(TaskState::Ready)
        || tasks.get_by_pid(parent).map(|task| task.state) != Some(TaskState::WaitQueue(0x44))
        || tasks.wait_queue_wake(0x44, 1) != Ok(1)
        || tasks.sleep_for(thread, 1_000, 500).is_err()
        || tasks.sleep_for(thread, 1_000, 500) != Err(TaskError::TaskNotRunnable)
        || tasks.next_timer_deadline() != Some(1_500)
        || tasks.poll_timers(1_499) != TimerPollResult::default()
        || tasks.poll_timers(1_500)
            != (TimerPollResult {
                expirations: 1,
                woken: 1,
                rearmed: 0,
            })
        || tasks.get_by_pid(thread).map(|task| task.state) != Some(TaskState::Ready)
    {
        return false;
    }
    let Ok(periodic_timer) = tasks.arm_timer(parent, 2_000, 100, 50) else {
        return false;
    };
    if tasks.poll_timers(2_100)
        != (TimerPollResult {
            expirations: 1,
            woken: 0,
            rearmed: 1,
        })
        || tasks.get_by_pid(parent).map(|task| task.timer_expirations) != Some(1)
        || tasks.poll_timers(2_260)
            != (TimerPollResult {
                expirations: 3,
                woken: 0,
                rearmed: 1,
            })
        || tasks.get_by_pid(parent).map(|task| task.timer_expirations) != Some(4)
        || tasks.cancel_timer(parent, periodic_timer).is_err()
        || tasks.timer_count() != 0
        || tasks.sleep_for(thread, 3_000, 100).is_err()
        || tasks.send_signal(thread, SIGUSR1).is_err()
        || tasks.get_by_pid(thread).map(|task| task.state) != Some(TaskState::Ready)
        || tasks.timer_count() != 0
    {
        return false;
    }
    let Ok(signal_parent) = tasks.spawn(Some(init), "signal-parent", TaskKind::User) else {
        return false;
    };
    let Ok(signal_child) = tasks.spawn(Some(signal_parent), "signal-child", TaskKind::User) else {
        return false;
    };
    if tasks.send_signal(signal_parent, SIGTERM).is_err()
        || tasks.get_by_pid(signal_parent).map(|task| task.state)
            != Some(TaskState::Zombie(128 + SIGTERM as i32))
        || tasks.get_by_pid(signal_child).and_then(|task| task.parent) != Some(init)
        || tasks
            .wait(init, Some(signal_parent))
            .map(|result| result.status)
            != Ok(128 + SIGTERM as i32)
        || tasks.exit(signal_child, 0).is_err()
        || tasks.wait(init, Some(signal_child)).is_err()
        || tasks.exit(thread, 0).is_err()
        || tasks.get_by_pid(thread).is_some()
    {
        return false;
    }
    let Ok(reused_slot_pid) = tasks.spawn(Some(init), "slot-test", TaskKind::User) else {
        return false;
    };
    reused_slot_pid > signal_child && tasks.active_count() == 4 && tasks.zombie_count() == 0
}
