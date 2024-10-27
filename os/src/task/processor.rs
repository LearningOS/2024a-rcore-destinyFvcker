//!Implementation of [`Processor`] and Intersection of control flow
//!
//! Here, the continuous operation of user apps in CPU is maintained,
//! the current running state of CPU is recorded,
//! and the replacement and transfer of control flow of different applications are executed.

use super::__switch;
use super::{fetch_task, TaskStatus};
use super::{TaskContext, TaskControlBlock};
use crate::sync::UPSafeCell;
use crate::syscall::{kernel_get_time, TimeVal};
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::*;

/// Processor management structure
pub struct Processor {
    ///The task currently executing on the current processor
    current: Option<Arc<TaskControlBlock>>,

    ///The basic control flow of each core, helping to select and switch process
    idle_task_cx: TaskContext,
}

impl Processor {
    ///Create an empty Processor
    pub fn new() -> Self {
        Self {
            current: None,
            idle_task_cx: TaskContext::zero_init(),
        }
    }

    ///Get mutable reference to `idle_task_cx`
    fn get_idle_task_cx_ptr(&mut self) -> *mut TaskContext {
        &mut self.idle_task_cx as *mut _
    }

    ///Get current task in moving semanteme
    pub fn take_current(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.current.take()
    }

    ///Get current task in cloning semanteme
    pub fn current(&self) -> Option<Arc<TaskControlBlock>> {
        self.current.as_ref().map(Arc::clone)
    }
}

lazy_static! {
    pub static ref PROCESSOR: UPSafeCell<Processor> = unsafe { UPSafeCell::new(Processor::new()) };
}

// [liuzl 2024年10月26日10:28:00]
// 可以看到这里和ch4之中的风格完全不一样了，在ch4之中，任务有几个不同的状态，而且无论任务的状态是
// 什么样的，都会驻留在内存之中，不对，应该换一种说法，都会被调度器轮询到，但是实际上在ch5之中，所有
// 进程控制块都被保存在 manager.rs 之中的一个 VecDeque 之中，当一个进程exit之后，它在 VecDeque
// 之中就消失了，虽然它仍然驻留在内存之中，但是现在永远都不会轮询到它了。
//
// 比如说下一个函数的消失：
// fn find_next_task(&self) -> Option<(usize, TaskStatus)> {
//     let inner = self.inner.exclusive_access();
//     let current = inner.current_task;
//     (current + 1..current + self.num_app + 1)
//         .map(|id| id % self.num_app)
//         .find(|id| {
//             inner.tasks[*id].task_status == TaskStatus::Ready
//                 || inner.tasks[*id].task_status == TaskStatus::Init
//         })
//         .map(|id| (id, inner.tasks[id].task_status))
// }

///The main part of process execution and scheduling
///Loop `fetch_task` to get the process that needs to run, and switch the process through `__switch`
pub fn run_tasks() {
    loop {
        let mut processor = PROCESSOR.exclusive_access();
        if let Some(task) = fetch_task() {
            let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
            // access coming task TCB exclusively
            let mut task_inner = task.inner_exclusive_access();
            let next_task_cx_ptr = &task_inner.task_cx as *const TaskContext;

            if task_inner.task_status == TaskStatus::UnInit {
                kernel_get_time(
                    &mut task_inner.start_up_time as *mut TimeVal,
                    usize::default(),
                );
            }

            task_inner.task_status = TaskStatus::Running; // Running 和 Ready 之间有一些区别啊

            // release coming task_inner manually
            drop(task_inner);
            // release coming task TCB manually
            processor.current = Some(task);
            // release processor manually
            drop(processor);
            unsafe {
                __switch(idle_task_cx_ptr, next_task_cx_ptr);
            }
        } else {
            warn!("no tasks available in run_tasks");
        }
    }
}

/// Get current task through take, leaving a None in its place
pub fn take_current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().take_current()
}

/// Get a copy of the current task
pub fn current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().current()
}

/// Get the current user token(addr of page table)
pub fn current_user_token() -> usize {
    let task = current_task().unwrap();
    task.get_user_token()
}

///Get the mutable reference to trap context of current task
pub fn current_trap_cx() -> &'static mut TrapContext {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .get_trap_cx()
}

///Return to idle control flow for new scheduling
pub fn schedule(switched_task_cx_ptr: *mut TaskContext) {
    let mut processor = PROCESSOR.exclusive_access();
    let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
    drop(processor);
    unsafe {
        __switch(switched_task_cx_ptr, idle_task_cx_ptr);
    }
}

/// set priority of current running process
pub fn set_proc_prio(prio: usize) {
    let current_task = current_task().unwrap();

    let mut inner = current_task.inner_exclusive_access();
    inner.proc_prio = prio;
}
