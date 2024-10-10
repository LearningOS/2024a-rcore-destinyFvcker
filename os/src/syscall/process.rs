//! Process management syscalls
use core::ops::Sub;

use crate::{
    config::MAX_SYSCALL_NUM,
    task::{
        current_task_info, exit_current_and_run_next, suspend_current_and_run_next, TaskStatus,
    },
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
/// A structure representing a time value, consisting of seconds and microseconds.
///
/// This structure is commonly used to represent time intervals or timestamps, where
/// the `sec` field stores the whole seconds part and the `usec` field stores the
/// fractional part in microseconds.
///
/// ## Fields
///
/// - `sec` - The seconds part of the time value.
/// - `usec` - The microseconds part of the time value.
///
/// This structure follows the memory layout of C (`#[repr(C)]`) for compatibility with C APIs.
pub struct TimeVal {
    ///  表示时间值之中的秒
    pub sec: usize,
    /// 表示时间值之中的微秒部分
    pub usec: usize,
}

/// 为 TimeVal 实现减号运算符重载，返回间隔时长（单位 ms）
impl Sub for TimeVal {
    type Output = usize;
    fn sub(self, rhs: Self) -> Self::Output {
        let self_total_msec = (self.sec * 1_000_000 + self.usec) / 1_000;
        let other_total_msec = (rhs.sec * 1_000_000 + rhs.usec) / 1_000;

        self_total_msec - other_total_msec
    }
}

/// Task information
#[allow(dead_code)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    status: TaskStatus,
    /// The numbers of syscall called by task
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    time: usize,
}

impl TaskInfo {
    /// Construct a new TaskInfo instance
    pub fn new(status: TaskStatus, syscall_times: [u32; MAX_SYSCALL_NUM], time: usize) -> Self {
        TaskInfo {
            status,
            syscall_times,
            time,
        }
    }
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

/// get information of current task
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");
    unsafe { *ti = current_task_info() }
    0
}
