//! Types related to task management
use core::fmt::Debug;

use super::TaskContext;
use crate::config::{MAX_SYSCALL_NUM, TRAP_CONTEXT_BASE};
use crate::mm::{
    kernel_stack_position, MapPermission, MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE,
};
use crate::syscall::process::TimeVal;
use crate::trap::{trap_handler, TrapContext};

/// The task control block (TCB) of a task.
pub struct TaskControlBlock {
    /// Save task context
    pub task_cx: TaskContext,

    /// Maintain the execution status of the current process
    pub task_status: TaskStatus,

    /// Application address space
    pub memory_set: MemorySet,

    /// The phys page number of trap context
    pub trap_cx_ppn: PhysPageNum,

    /// The size(top addr) of program which is loaded from elf file
    pub base_size: usize,

    /// Heap bottom, 表示堆内存的起始地址
    pub heap_bottom: usize,

    // 在这个 TaskControlBlock 结构体中，program_brk 是指进程的程序断点（Program Break），
    // 用于管理进程的堆（heap）内存。堆是动态分配内存的区域，程序可以在运行时通过系统调用（如 brk 或 sbrk）调整其大小。
    // 具体来说，program_brk 通常表示堆的顶部（或者堆的结束地址），即分配给进程的可用堆内存的最高地址。
    // 随着进程需要更多的动态内存，program_brk 的值可以增加，以扩大堆的大小。反之，如果程序不再需要那么多内存，
    // program_brk 可以缩小，释放部分堆内存。
    // 因此，program_brk 用于跟踪堆的当前边界，以便任务在需要时正确管理内存分配和释放。
    /// Program break
    pub program_brk: usize,

    /// Task syscall cnt array
    pub syscall_cnt: [u32; MAX_SYSCALL_NUM],

    /// Task start up time
    pub start_up_time: TimeVal,
}

impl TaskControlBlock {
    /// get the trap context
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    /// get the user token
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    /// Based on the elf info in program, build the contents of task in a new address space
    pub fn new(elf_data: &[u8], app_id: usize) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        let task_status = TaskStatus::Init;
        // map a kernel-stack in kernel space
        let (kernel_stack_bottom, kernel_stack_top) = kernel_stack_position(app_id);
        KERNEL_SPACE.exclusive_access().insert_framed_area(
            kernel_stack_bottom.into(),
            kernel_stack_top.into(),
            MapPermission::R | MapPermission::W,
        );
        let task_control_block = Self {
            task_status,
            task_cx: TaskContext::goto_trap_return(kernel_stack_top),
            memory_set,
            trap_cx_ppn,
            base_size: user_sp,
            heap_bottom: user_sp,
            program_brk: user_sp,
            syscall_cnt: [0; MAX_SYSCALL_NUM],
            start_up_time: TimeVal::default(),
        };
        // prepare TrapContext in user space
        let trap_cx = task_control_block.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }
    /// change the location of the program break. return None if failed.
    pub fn change_program_brk(&mut self, size: i32) -> Option<usize> {
        let old_break = self.program_brk;
        let new_brk = self.program_brk as isize + size as isize;
        if new_brk < self.heap_bottom as isize {
            return None;
        }
        let result = if size < 0 {
            self.memory_set
                .shrink_to(VirtAddr(self.heap_bottom), VirtAddr(new_brk as usize))
        } else {
            self.memory_set
                .append_to(VirtAddr(self.heap_bottom), VirtAddr(new_brk as usize))
        };
        if result {
            self.program_brk = new_brk as usize;
            Some(old_break)
        } else {
            None
        }
    }
    /// update syscall statistics of current task
    pub fn update_syscall_cnt(&mut self, syscall_id: usize) {
        self.syscall_cnt[syscall_id] += 1;
    }
}

#[derive(Copy, Clone, PartialEq)]
/// task status: UnInit, Ready, Running, Exited
pub enum TaskStatus {
    /// uninitialized
    UnInit,
    /// ready to run
    Ready,
    /// running
    Running,
    /// exited
    Exited,
    /// initialized, but never ran
    Init,
}

impl Debug for TaskStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnInit => write!(f, "status: Ready"),
            Self::Ready => write!(f, "status: Ready"),
            Self::Running => write!(f, "status: Running"),
            Self::Exited => write!(f, "status: Exited"),
            Self::Init => write!(f, "status: Init"),
        }
    }
}
