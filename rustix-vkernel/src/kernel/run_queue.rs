use crate::sync::SpinMutex;

use super::sched::MAX_CPUS;

pub const PRIORITY_LEVELS: usize = 64;
pub const MAX_RUNNABLE_THREADS: usize = 16;

#[derive(Clone, Copy)]
pub struct RunQueue {
    nonempty: u64,
    queued: u16,
    buckets: [u16; PRIORITY_LEVELS],
    levels: [u8; MAX_RUNNABLE_THREADS],
    cursors: [u8; PRIORITY_LEVELS],
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            nonempty: 0,
            queued: 0,
            buckets: [0; PRIORITY_LEVELS],
            levels: [0; MAX_RUNNABLE_THREADS],
            cursors: [0; PRIORITY_LEVELS],
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    pub fn enqueue(&mut self, thread: usize, priority: u8) -> bool {
        if thread >= MAX_RUNNABLE_THREADS || usize::from(priority) >= PRIORITY_LEVELS {
            return false;
        }
        self.remove(thread);
        let thread_bit = 1u16 << thread;
        let level = usize::from(priority);
        self.buckets[level] |= thread_bit;
        self.nonempty |= 1u64 << level;
        self.queued |= thread_bit;
        self.levels[thread] = priority;
        true
    }

    pub fn remove(&mut self, thread: usize) -> bool {
        if thread >= MAX_RUNNABLE_THREADS {
            return false;
        }
        let thread_bit = 1u16 << thread;
        if self.queued & thread_bit == 0 {
            return false;
        }
        let level = usize::from(self.levels[thread]);
        self.buckets[level] &= !thread_bit;
        if self.buckets[level] == 0 {
            self.nonempty &= !(1u64 << level);
        }
        self.queued &= !thread_bit;
        true
    }

    pub fn pop_highest(&mut self) -> Option<usize> {
        let level = 63usize.checked_sub(self.nonempty.leading_zeros() as usize)?;
        let bucket = self.buckets[level];
        let start = usize::from(self.cursors[level]) % MAX_RUNNABLE_THREADS;
        let after_cursor = bucket & (u16::MAX << start);
        let thread = if after_cursor != 0 {
            after_cursor.trailing_zeros() as usize
        } else {
            bucket.trailing_zeros() as usize
        };
        self.cursors[level] = ((thread + 1) % MAX_RUNNABLE_THREADS) as u8;
        self.remove(thread);
        Some(thread)
    }

    pub fn len(&self) -> usize {
        self.queued.count_ones() as usize
    }
}

static RUN_QUEUES: [SpinMutex<RunQueue>; MAX_CPUS] =
    [const { SpinMutex::new(RunQueue::new()) }; MAX_CPUS];

pub fn reset() {
    for queue in &RUN_QUEUES {
        queue.lock().clear();
    }
}

pub fn enqueue(cpu: usize, thread: usize, priority: u8) -> bool {
    RUN_QUEUES
        .get(cpu)
        .is_some_and(|queue| queue.lock().enqueue(thread, priority))
}

pub fn remove(cpu: usize, thread: usize) -> bool {
    RUN_QUEUES
        .get(cpu)
        .is_some_and(|queue| queue.lock().remove(thread))
}

pub fn pop_highest(cpu: usize) -> Option<usize> {
    RUN_QUEUES.get(cpu)?.lock().pop_highest()
}

pub fn len(cpu: usize) -> usize {
    RUN_QUEUES.get(cpu).map_or(0, |queue| queue.lock().len())
}

pub fn self_test() -> bool {
    let mut queue = RunQueue::new();
    queue.enqueue(0, 1)
        && queue.enqueue(1, 63)
        && queue.enqueue(2, 63)
        && queue.len() == 3
        && queue.pop_highest() == Some(1)
        && queue.pop_highest() == Some(2)
        && queue.enqueue(0, 40)
        && queue.len() == 1
        && queue.pop_highest() == Some(0)
        && queue.pop_highest().is_none()
}
