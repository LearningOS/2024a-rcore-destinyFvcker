//! Memory management implementation
//!
//! SV39 page-based virtual-memory architecture for RV64 systems, and
//! everything about memory management, like frame allocator, page table,
//! map area and memory set, is implemented here.
//!
//! Every task or process has a memory_set to control its virtual memory.
mod address;
mod frame_allocator;
mod heap_allocator;
mod memory_set;
mod page_table;

use address::VPNRange;
pub use address::{PhysAddr, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
use frame_allocator::is_enough;
pub use frame_allocator::{frame_alloc, frame_dealloc, FrameTracker};
pub use memory_set::remap_test;
use memory_set::CrossType;
pub use memory_set::{kernel_token, mmap, munmap, MapPermission, MemorySet, KERNEL_SPACE};
use page_table::PTEFlags;
pub use page_table::{
    translated_byte_buffer, translated_ref, translated_refmut, translated_str, PageTable,
    PageTableEntry, UserBuffer, UserBufferIterator,
};

use crate::task::current_task;

/// initiate heap allocator, frame allocator and kernel space
pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}

/// 判断是否提供的虚拟地址分配范围和当前运行进程已分配的地址空间有冲突
pub fn alloc_check(
    start: usize,
    end: usize,
    port: usize,
) -> Result<(VirtAddr, VirtAddr), &'static str> {
    if let Some(task) = current_task() {
        let inner = task.inner_exclusive_access();

        let start_vpa = VirtAddr::from(start);
        let end_vpa = VirtAddr::from(end);
        let start_vpn: VirtPageNum = start_vpa.floor();
        let end_vpn: VirtPageNum = end_vpa.ceil();

        if !start_vpa.aligned() {
            return Err("start_vpa does't aligned!!!");
        }
        if port & !0x7 != 0 {
            return Err(
                "port: Bit 0 indicates whether it is readable, bit 1 indicates whether it is writable, \
                    and bit 2 indicates whether it is executable. Other bits are invalid and must be 0."
            );
        }
        if port & 0x7 == 0 {
            return Err(
                "port = 0 means that it is not readable、not writable and not executable, it's meaningless."
            );
        }
        if !is_enough(end_vpn.0 - start_vpn.0) {
            return Err("pysical memory is not enough right now!");
        }

        inner.memory_set.is_conflict(start_vpn, end_vpn)?;

        Ok((start_vpa, end_vpa))
    } else {
        panic!(
            "Try to judge virtual address space is conflict with virtual segment \
                that user program want to allocate. \
                But there isn't any running task in Task Manager!"
        )
    }
}

/// 当前运行的进程要请求内存空间分配时需要进行的一系列检查
pub fn dealloc_check(start: VirtPageNum, end: VirtPageNum) -> Result<CrossType, &'static str> {
    if let Some(task) = current_task() {
        let inner = task.inner_exclusive_access();
        inner.memory_set.is_vmm_fully_mapped(start, end)
    } else {
        panic!(
            "Try to check whether the given address range is fully mapped. \
                But there isn't any running task in Task Manager!"
        )
    }
}

/// 为当前运行的进程分配内存
fn alloc_mm(start: VirtAddr, end: VirtAddr, port: MapPermission) {
    if let Some(task) = current_task() {
        let mut inner = task.inner_exclusive_access();
        inner.memory_set.insert_framed_area(start, end, port);
    } else {
        panic!("There isn't any running task in Task Manager!")
    }
}

/// 为当前运行的进程回收内存
fn dealloc_mm(start: VirtPageNum, end: VirtPageNum, cross_type: CrossType) {
    if let Some(task) = current_task() {
        let mut inner = task.inner_exclusive_access();
        inner.memory_set.free(start, end, cross_type);
    } else {
        panic!("There isn't any running task in Task Manager!")
    }
}
