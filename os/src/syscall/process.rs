//! Process management syscalls
use core::ops::Sub;

use crate::{
    config::MAX_SYSCALL_NUM,
    mm,
    task::{
        change_program_brk, exit_current_and_run_next, get_current_taskinfo,
        suspend_current_and_run_next, write_to_cur, TaskStatus,
    },
    timer::get_time_us,
};

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
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TimeVal {
    /// 表示时间值之中的秒
    pub sec: usize,
    /// 表示时间值之中的微秒
    pub usec: usize,
}

impl Sub for TimeVal {
    type Output = usize;

    /// Calculates the result of subtracting two `TimeVal``
    /// return type's unit is ms
    fn sub(self, rhs: Self) -> Self::Output {
        let self_ms = (self.sec * 1_000_000 + self.usec) / 1_000;
        let rhs_ms = (rhs.sec * 1_000_000 + rhs.usec) / 1_000;

        self_ms - rhs_ms
    }
}

/// Task information
#[allow(dead_code)]
#[derive(Debug)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    status: TaskStatus,
    /// The numbers of syscall called by task
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    time: usize,
}

impl TaskInfo {
    /// Construct a new `TaskInfo`
    pub fn new(status: TaskStatus, syscall_times: &[u32; MAX_SYSCALL_NUM], time: usize) -> Self {
        Self {
            status,
            syscall_times: syscall_times.clone(),
            time,
        }
    }
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// Get time function for os kernel
pub fn kernel_get_time(ts: *mut TimeVal, _tz: usize) {
    trace!("kernel: get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
}

/// Get time with second and microsecond
/// reimplement it with virtual memory management.
/// [`TimeVal`] can be splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let time_val = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    write_to_cur(ts, time_val);
    0
}

/// Finish sys_task_info to pass testcases
/// reimplement it with virtual memory management.
/// [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info NOT IMPLEMENTED YET!");
    let task_info = get_current_taskinfo();
    write_to_cur(ti, task_info);
    0
}

/// 系统调用 ID：222
///
/// 功能：
/// 申请长度为 `len` 字节的物理内存，将其映射到从 `start` 开始的虚拟内存，并根据 `port` 参数设置内存页属性。
/// 物理内存可以是任意位置，目标虚拟内存区的页属性由 `port` 决定。
///
/// 参数：
/// - `start`: 需要映射的虚拟内存起始地址，要求按页对齐。
/// - `len`: 映射的字节长度，允许为 0。会自动按页大小向上取整。
/// - `port`: 内存页属性，第 0 位表示是否可读，第 1 位表示是否可写，第 2 位表示是否可执行，其他位必须为 0。
///
/// 返回值：
/// - 成功时返回 `0`。
/// - 失败时返回 `-1`。
///
/// 注意事项：
/// - 为简化操作，虚拟内存区 `start` 要求按页大小对齐，`len` 自动向上取整为页大小的倍数。
/// - 不处理内存分配失败后的回收问题。
///
/// 可能的错误：
/// 1. `start` 地址未按页对齐。
/// 2. `port` 参数包含无效位，`port & !0x7 != 0`。
/// 3. `port` 设置无效，`port & 0x7 == 0`，即没有设置任何有效权限。
/// 4. 映射区间 `[start, start + len)` 中包含已经映射的页。
/// 5. 系统物理内存不足，无法分配请求的内存。
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    mm::mmap(start, len, port)
}

/// 系统调用 ID：215
///
/// 功能：
/// 取消虚拟内存区间 `[start, start + len)` 的映射。
///
/// 参数：
/// - `start`: 需要取消映射的虚拟内存起始地址，要求按页对齐。
/// - `len`: 需要取消映射的字节长度，按页大小处理。
///
/// 返回值：
/// - 成功时返回 `0`。
/// - 失败时返回 `-1`。
///
/// 注意事项：
/// - 与 `mmap` 类似，`munmap` 取消内存映射时不处理内存恢复和回收问题，即使参数错误。
/// - 实现时要特别注意 `mmap` 操作中的页表项，并理解 RISC-V 的页表项格式与 `port` 参数的区别。
/// - 在实现时需要确认是否增加了 `PTE_U`（用户态页表项标志）。
///
/// 可能的错误：
/// - 试图取消映射的虚拟内存区间 `[start, start + len)` 中包含未被映射的页。
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    mm::munmap(start, len)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
