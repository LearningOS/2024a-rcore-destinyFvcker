//! Types related to task management

use super::TaskContext;
use crate::config::MAX_SYSCALL_NUM;
use crate::syscall::TimeVal;

/// The task control block (TCB) of a task.
#[derive(Copy, Clone)]
pub struct TaskControlBlock {
    /// The task status in it's lifecycle
    pub task_status: TaskStatus,
    /// The task context
    pub task_cx: TaskContext,
    /// The start up time of thie task
    pub start_up_time: TimeVal,
    /// The system conter of this task
    pub syscall_counter: [u32; MAX_SYSCALL_NUM],
}

impl TaskControlBlock {
    /// update syscall statistics of current task
    pub fn update_syscall_cnt(&mut self, syscall_id: usize) {
        self.syscall_counter[syscall_id] += 1;
    }

    /// create a new TaskControlBlock with default value
    pub fn new() -> Self {
        TaskControlBlock {
            task_status: TaskStatus::UnInit,
            task_cx: TaskContext::zero_init(),
            start_up_time: TimeVal::default(),
            syscall_counter: [0; MAX_SYSCALL_NUM],
        }
    }
}

/// The status of a task
#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    /// uninitialized
    UnInit,
    /// initialized, but never run
    Init,
    /// ready to run
    Ready,
    /// running
    Running,
    /// exited
    Exited,
}
