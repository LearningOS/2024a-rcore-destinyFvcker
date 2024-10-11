//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::config::MAX_APP_NUM;
use crate::loader::{get_num_app, init_app_cx};
use crate::sync::UPSafeCell;
use crate::syscall::process::{sys_get_time, TaskInfo};
use crate::syscall::TimeVal;
use lazy_static::*;
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// Inner of Task Manager
pub struct TaskManagerInner {
    /// task list
    tasks: [TaskControlBlock; MAX_APP_NUM],
    /// id of current `Running` task
    current_task: usize,
}

lazy_static! {
    /// Global variable: TASK_MANAGER
    pub static ref TASK_MANAGER: TaskManager = {
        let num_app = get_num_app();
        // init task vec with default value
        let mut tasks = [TaskControlBlock::new(); MAX_APP_NUM];
        for (i, task) in tasks.iter_mut().enumerate() {
            task.task_cx = TaskContext::goto_restore(init_app_cx(i));
            task.task_status = TaskStatus::Init;
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch3, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let task0 = &mut inner.tasks[0];

        // 对于现在没有 idle task 的情况，要在 run_first_task 之中单独
        // 构造第一个运行的 task 的启动时间
        sys_get_time(&mut task0.start_up_time as *mut TimeVal, usize::default());

        task0.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &task0.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut TaskContext, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    /// 现在 return type 修改为了下一个 task 在 TASK_MANAGER 之中的索引还有
    /// 该 Task 现在的状态，可能为 `Ready` 或 `Init`, 我们借助这个来判断 task 是不是第一
    /// 次运行，更新第一次运行的时间
    fn find_next_task(&self) -> Option<(usize, TaskStatus)> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        // 循环遍历下一个应该运行的task，[liuzl note: 我感觉这个实现真的很有价值!]
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| {
                inner.tasks[*id].task_status == TaskStatus::Ready
                    || inner.tasks[*id].task_status == TaskStatus::Init
            })
            .map(|id| (id, inner.tasks[id].task_status))
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some((next, task_status)) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;

            // 设定下一个需要运行的 task 的相关信息
            if task_status == TaskStatus::Init {
                // 假如在这之前当前 task 一次都没有运行过，则设置启动时间
                sys_get_time(
                    &mut inner.tasks[next].start_up_time as *mut TimeVal,
                    usize::default(),
                );
            }
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }

    /// When current task use syscall, update the syscall_cnt array in it's TaskControlBlock
    fn update_syscall_cnt(&self, syscall_id: usize) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].update_syscall_cnt(syscall_id);
    }

    /// Get information of current task
    fn current_task_info(&self) -> TaskInfo {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;

        let current_task_ref = &inner.tasks[current];

        let mut current_time = TimeVal::default();
        sys_get_time(&mut current_time as *mut TimeVal, usize::default());

        TaskInfo::new(
            current_task_ref.task_status,
            current_task_ref.syscall_counter.clone(),
            current_time - current_task_ref.start_up_time,
        )
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// 2024年10月10日18:30:51
/// update syscall_cnt in current task's TaskControlBlock
pub fn update_syscall_cnt(syscall_id: usize) {
    TASK_MANAGER.update_syscall_cnt(syscall_id);
}

/// 2024年10月10日23:03:58
/// get information of current task
pub fn current_task_info() -> TaskInfo {
    TASK_MANAGER.current_task_info()
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}
