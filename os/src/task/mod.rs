//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the whole operating system.
//!
//! A single global instance of [`Processor`] called `PROCESSOR` monitors running
//! task(s) for each core.
//!
//! A single global instance of `PID_ALLOCATOR` allocates pid for user apps.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.
mod context;
mod id;
mod manager;
mod processor;
mod switch;
mod task;

#[allow(clippy::module_inception)]
#[allow(rustdoc::private_intra_doc_links)]
use crate::fs::{open_file, OpenFlags};
use crate::{
    config::BIG_STRIDE,
    mm::translated_refmut,
    syscall::{kernel_get_time, TaskInfo, TimeVal},
};
use alloc::sync::Arc;
pub use context::TaskContext;
use core::panic;
pub use id::{kstack_alloc, pid_alloc, KernelStack, PidHandle};
use lazy_static::*;
pub use manager::add_task;
pub use manager::{fetch_task, TaskManager};
pub use processor::{
    current_task, current_trap_cx, current_user_token, run_tasks, schedule, set_proc_prio,
    take_current_task, Processor,
};
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};
/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    // There must be an application running.
    let task = take_current_task().unwrap();

    // ---- access current TCB exclusively
    let mut task_inner = task.inner_exclusive_access();
    task_inner.proc_stride += BIG_STRIDE / task_inner.proc_prio;
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    // Change status to Ready
    task_inner.task_status = TaskStatus::Ready;
    drop(task_inner);
    // ---- release current PCB

    // push back to ready queue.
    add_task(task);
    // jump to scheduling cycle
    schedule(task_cx_ptr);
}

/// pid of usertests app in make run TEST=1
pub const IDLE_PID: usize = 0;

// [liuzl 2024年10月26日10:31:36] 整个项目之中唯一对于Zombie状态的处理存在于下面的
// 函数之中，那么我现在有一个问题：难道用户态程序在最后都要手动调用exit()函数来标记自己
// 为僵尸进程吗？

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next(exit_code: i32) {
    // take from Processor
    let task = take_current_task().unwrap();

    let pid = task.getpid();
    if pid == IDLE_PID {
        println!(
            "[kernel] Idle process exit with exit_code {} ...",
            exit_code
        );
        panic!("All applications completed!");
    }

    // **** access current TCB exclusively
    let mut inner = task.inner_exclusive_access();
    // Change status to Zombie
    inner.task_status = TaskStatus::Zombie;
    // Record exit code
    inner.exit_code = exit_code;
    // do not move to its parent but under initproc

    // ++++++ access initproc TCB exclusively
    {
        // 将当前进程的所有子进程挂在初始进程initproc下面，便利每一个子进程，修改其父进程为初始进程，
        // 并加入初始进程的孩子向量之中。
        let mut initproc_inner = INITPROC.inner_exclusive_access();
        for child in inner.children.iter() {
            child.inner_exclusive_access().parent = Some(Arc::downgrade(&INITPROC));
            initproc_inner.children.push(child.clone());
        }
    }
    // ++++++ release parent PCB

    inner.children.clear();
    // deallocate user space
    // 这里调用 MemorySet::recycle_data_pages 就只是将地址空间之中的逻辑段累表 areas 清空，
    // 这将导致地址空间被回收（也就是进程的数据和代码对应的物理页帧都被回收），但是用来存放页表的
    // 那些物理页帧此时还不会被回收（由父进程最后回收子进程剩余的占用资源时回收）
    inner.memory_set.recycle_data_pages();
    drop(inner);
    // +++++++ release current PCB
    // drop task manually to maintain rc correctly
    drop(task);
    // we do not have to save task context
    let mut _unused = TaskContext::zero_init();
    schedule(&mut _unused as *mut _);
}

lazy_static! {
    /// Creation of initial process
    ///
    /// the name "initproc" may be changed to any other app name like "usertests",
    /// but we have user_shell, so we don't need to change it.
    pub static ref INITPROC: Arc<TaskControlBlock> = Arc::new({
        let inode = open_file("ch6b_initproc", OpenFlags::RDONLY).unwrap();
        let v = inode.read_all();
        TaskControlBlock::new(v.as_slice())
    });
}

///Add init process to the manager
pub fn add_initproc() {
    add_task(INITPROC.clone());
}

/// Update syscall cnt of current running task
pub fn update_syscall_cnt(syscall_id: usize) {
    if let Some(task) = current_task() {
        let mut inner = task.inner_exclusive_access();
        inner.update_syscall_cnt(syscall_id);
    } else {
        panic!(
            "Try to update current running task's syscall counter array, \
                But there isn't any running task in Task Manager!"
        )
    }
}

/// Get current running task's status
pub fn get_current_task_info(ti: *mut TaskInfo) {
    if let Some(task) = current_task() {
        let inner = task.inner_exclusive_access();
        let task_info = inner.get_task_info();

        let user_ptr = translated_refmut(inner.memory_set.token(), ti);
        *user_ptr = task_info;
    } else {
        panic!(
            "Try to get current running task info, \
                But there isn't any running task in Task Manager!"
        )
    }
}

/// Get time for current tunning task
pub fn get_time_task(ts: *mut TimeVal) {
    if let Some(task) = current_task() {
        let inner = task.inner_exclusive_access();
        let mut sys_time = TimeVal::default();

        kernel_get_time(&mut sys_time as *mut TimeVal, usize::default());

        let user_ptr = translated_refmut(inner.memory_set.token(), ts);
        *user_ptr = sys_time;
    } else {
        panic!("There isn't any running task!")
    }
}
