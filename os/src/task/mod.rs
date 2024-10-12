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

use crate::loader::{get_app_data, get_num_app};
use crate::mm::{write_translated_buffer, MapPermission, VirtAddr, VirtPageNum};
use crate::sync::UPSafeCell;
use crate::syscall::process::{kernel_get_time, TaskInfo, TimeVal};
use crate::trap::TrapContext;
use alloc::vec::Vec;
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

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
}

lazy_static! {
    /// a `TaskManager` global instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        println!("init TASK_MANAGER");
        let num_app = get_num_app();
        println!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
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
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let next_task = &mut inner.tasks[0];

        // update task start up time
        kernel_get_time(
            &mut next_task.start_up_time as *mut TimeVal,
            usize::default(),
        );

        next_task.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<(usize, TaskStatus)> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| {
                inner.tasks[*id].task_status == TaskStatus::Ready
                    || inner.tasks[*id].task_status == TaskStatus::Init
            })
            .map(|id| (id, inner.tasks[id].task_status))
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &'static mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Change the current 'Running' task's program break
    pub fn change_current_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].change_program_brk(size)
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some((next, status)) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;

            if status == TaskStatus::Init {
                kernel_get_time(
                    &mut inner.tasks[current].start_up_time as *mut TimeVal,
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

    /// Update syscall cnt of current running task
    #[allow(unused)]
    fn update_syscall_cnt(&self, syscall_id: usize) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;

        inner.tasks[current].update_syscall_cnt(syscall_id);
    }

    /// Get task info of current running task
    #[allow(unused)]
    fn get_task_info(&self) -> TaskInfo {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;

        let current_task_ref = &inner.tasks[current];

        let mut current_time = TimeVal::default();
        kernel_get_time(&mut current_time as *mut TimeVal, usize::default());

        TaskInfo::new(
            current_task_ref.task_status,
            &current_task_ref.syscall_cnt,
            current_time - current_task_ref.start_up_time,
        )
    }

    /// Write a value to current running task's memory space
    fn write_to_cur<T: Sized>(&self, user_ptr: *mut T, val: T) {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;

        write_translated_buffer(
            inner.tasks[current].memory_set.token(),
            user_ptr as *const u8,
            val,
        );
    }

    /// Checks if there is a memory conflict in the range specified by the `start` and `end` virtual page numbers.
    ///
    /// # Arguments
    /// - `start` - The starting virtual page number of the memory range to check.
    /// - `end` - The ending virtual page number of the memory range to check.
    ///
    /// # Returns
    /// - `true` if there is a conflict in the memory range, otherwise `false`.
    pub fn is_conflict(&self, start: VirtPageNum, end: VirtPageNum) -> bool {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let task = &inner.tasks[current];

        task.memory_set.is_conflict(start, end)
    }

    /// Checks if the virtual memory manager (VMM) has mapped the specified memory range.
    ///
    /// # Arguments
    /// - `start` - The starting virtual page number of the memory range.
    /// - `end` - The ending virtual page number of the memory range.
    ///
    /// # Returns
    /// - A non-negative value if the memory range is mapped by the VMM, otherwise a negative value.
    pub fn is_vmm_mapped(&self, start: VirtPageNum, end: VirtPageNum) -> isize {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let task = &inner.tasks[current];

        task.memory_set.is_vmm_mapped(start, end)
    }

    /// Allocates memory in the specified range and maps it with the given permissions.
    ///
    /// # Arguments
    /// - `start` - The starting virtual address of the memory range to allocate.
    /// - `end` - The ending virtual address of the memory range to allocate.
    /// - `port` - The memory access permissions for the allocated range.
    ///
    /// # Description
    /// This function allocates a framed memory area for the specified range and sets the given access permissions.
    pub fn alloc_mm(&self, start: VirtAddr, end: VirtAddr, port: MapPermission) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let task = &mut inner.tasks[current];

        task.memory_set.insert_framed_area(start, end, port);
    }

    /// Deallocates the memory in the specified range and optionally considers crossing boundaries.
    ///
    /// # Arguments
    /// - `start_va` - The starting virtual page number of the memory range to deallocate.
    /// - `end_va` - The ending virtual page number of the memory range to deallocate.
    /// - `is_cross` - A flag indicating whether the range crosses a boundary (non-zero if true).
    ///
    /// # Description
    /// This function frees the memory in the specified range and handles crossing page boundaries based on the `is_cross` flag.
    pub fn dealloc_mm(&self, start_va: VirtPageNum, end_va: VirtPageNum, is_cross: isize) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let task = &mut inner.tasks[current];

        task.memory_set.free(start_va, end_va, is_cross as usize);
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

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Update the current 'Running' task's syscall_cnt
pub fn update_current_syscall_cnt(syscall_id: usize) {
    TASK_MANAGER.update_syscall_cnt(syscall_id);
}

/// Get the current 'Running' task's TaskInfo
pub fn get_current_taskinfo() -> TaskInfo {
    TASK_MANAGER.get_task_info()
}

/// Write a value to current running task's memory spac
pub fn write_to_cur<T: Sized>(user_ptr: *mut T, val: T) {
    TASK_MANAGER.write_to_cur(user_ptr, val);
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

/// Change the current 'Running' task's program break
pub fn change_program_brk(size: i32) -> Option<usize> {
    TASK_MANAGER.change_current_program_brk(size)
}
