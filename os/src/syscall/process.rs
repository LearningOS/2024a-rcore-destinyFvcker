//! Process management syscalls
use alloc::sync::Arc;
use core::ops::Sub;

use crate::{
    config::MAX_SYSCALL_NUM,
    fs::{open_file, OpenFlags},
    mm::{mmap, munmap, translated_refmut, translated_str},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        get_current_task_info, get_time_task, set_proc_prio, suspend_current_and_run_next,
        TaskStatus,
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
#[repr(C)]
#[derive(Debug, Default, Clone)]
pub struct TimeVal {
    /// sec part
    pub sec: usize,
    /// usec part
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
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
///
/// sys_waitpid 是一个立即返回的系统调用，它的返回值语义是：如果当前的进程不存在一个进程 ID
/// 为 pid（pid==-1 或 pid > 0）的子进程，则返回 -1；如果存在一个进程 ID 为 pid 的僵尸子进程，
/// 则正常回收并返回子进程的 pid，并更新系统调用的退出码参数为 exit_code 。这里还有一个 -2 的返回值，
/// 它的含义是子进程还没退出，通知用户库 user_lib （是实际发出系统调用的地方），这样用户库看到是 -2 后，
/// 就进一步调用 sys_yield 系统调用，让当前父进程进入等待状态。
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!(
        "kernel::pid[{}] sys_waitpid [{}]",
        current_task().unwrap().pid.0,
        pid
    );
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
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

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_get_time NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    get_time_task(ts);
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    trace!(
        "kernel:pid[{}] sys_task_info NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    get_current_task_info(ti);
    0
}

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    match mmap(start, len, port) {
        Ok(_) => 0,
        Err(message) => {
            println!("[kernel] sys_map error!!!, message: {:?}", message);
            -1
        }
    }
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_munmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    match munmap(start, len) {
        Ok(_) => 0,
        Err(message) => {
            println!("[kernel] sys_munmap error!!!, message: {:?}", message);
            -1
        }
    }
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_spawn", current_task().unwrap().pid.0);

    let token = current_user_token();
    let app_path = translated_str(token, path);

    if let Some(app_inode) = open_file(app_path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let current_task = current_task().unwrap();
        let spawn_task = current_task.spawn(&all_data);
        let pid = spawn_task.pid.0;

        let trap_cx = spawn_task.inner_exclusive_access().get_trap_cx();
        trap_cx.x[10] = 0;

        add_task(spawn_task);

        pid as isize
    } else {
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    if prio <= 2 {
        return -1;
    }
    set_proc_prio(prio as usize);
    prio
}
