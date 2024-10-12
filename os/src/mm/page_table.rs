//! Implementation of [`PageTableEntry`] and [`PageTable`].

use core::mem;

use super::{frame_alloc, FrameTracker, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
use alloc::vec;
use alloc::vec::Vec;
use bitflags::*;

bitflags! {
    /// page table entry flags
    pub struct PTEFlags: u8 {
        const V = 1 << 0;
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
        const G = 1 << 5;
        const A = 1 << 6;
        const D = 1 << 7;
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
/// page table entry structure
pub struct PageTableEntry {
    /// bits of page table entry
    pub bits: usize,
}

impl PageTableEntry {
    /// Create a new page table entry
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: ppn.0 << 10 | flags.bits as usize,
        }
    }
    /// Create an empty page table entry
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    /// Get the physical page number from the page table entry
    pub fn ppn(&self) -> PhysPageNum {
        (self.bits >> 10 & ((1usize << 44) - 1)).into()
    }
    /// Get the flags from the page table entry
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits(self.bits as u8).unwrap()
    }
    /// The page pointered by page table entry is valid?
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is readable?
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is writable?
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is executable?
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
}

/// page table structure
pub struct PageTable {
    root_ppn: PhysPageNum,
    frames: Vec<FrameTracker>,
}

/// Assume that it won't oom when creating/mapping.
impl PageTable {
    /// Create a new page table
    pub fn new() -> Self {
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn,
            frames: vec![frame],
        }
    }
    /// Temporarily used to get arguments from user space.
    pub fn from_token(satp: usize) -> Self {
        Self {
            root_ppn: PhysPageNum::from(satp & ((1usize << 44) - 1)),
            frames: Vec::new(),
        }
    }
    /// Find PageTableEntry by VirtPageNum, create a frame for a 4KB page table if not exist
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                let frame = frame_alloc().unwrap();
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                self.frames.push(frame);
            }
            ppn = pte.ppn();
        }
        result
    }
    /// Find PageTableEntry by VirtPageNum
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                return None;
            }
            ppn = pte.ppn();
        }
        result
    }
    /// set the map between virtual page number and physical page number
    #[allow(unused)]
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }
    /// remove the map between virtual page number and physical page number
    #[allow(unused)]
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        let pte = self.find_pte(vpn).unwrap();
        assert!(pte.is_valid(), "vpn {:?} is invalid before unmapping", vpn);
        *pte = PageTableEntry::empty();
    }
    /// get the page table entry from the virtual page number
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.find_pte(vpn).map(|pte| *pte)
    }
    /// get the token from the page table
    /// RISC-V 64位架构中的 `satp` 寄存器（Supervisor Address Translation and Protection Register）的位布局:
    ///
    /// | 63 - 60: MODE | 59 - 44: ASID | 44 - 0: PPN |
    ///
    /// - **MODE (4 bits)**: 表示地址转换模式。包括 `Bare` (直接物理地址模式), `Sv39` (39位虚拟地址转换模式), 和 `Sv48` (48位虚拟地址转换模式)。
    /// - **ASID (16 bits)**: Address Space Identifier，用于标识不同的地址空间，允许多个虚拟地址空间共存。
    /// - **PPN (44 bits)**: Physical Page Number，指向页表的物理地址，存储页表的高44位地址。
    ///
    /// `satp` 寄存器是页表基地址及其翻译模式配置的核心部分，用于控制虚拟内存到物理内存的映射。
    ///
    /// 所以这个函数的作用就是将页表根目录的物理页号转换成一个符合 satp 寄存器规范的值
    pub fn token(&self) -> usize {
        // 8usize << 60 -> set MODE = 8 -> open Sv39
        8usize << 60 | self.root_ppn.0
    }
}

/// Translate&Copy a ptr[u8] array with LENGTH len to a mutable u8 Vec through page table
pub fn translated_byte_buffer(token: usize, ptr: *const u8, len: usize) -> Vec<&'static mut [u8]> {
    let page_table = PageTable::from_token(token);
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        let start_va = VirtAddr::from(start);
        let mut vpn = start_va.floor();
        let ppn = page_table.translate(vpn).unwrap().ppn();
        vpn.step();
        let mut end_va: VirtAddr = vpn.into();
        end_va = end_va.min(VirtAddr::from(end));
        if end_va.page_offset() == 0 {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..]);
        } else {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..end_va.page_offset()]);
        }
        start = end_va.into();
    }
    v
}

/// Write a value `T` to translated `ptr[u8]` through page table
pub fn write_translated_buffer<T: Sized>(token: usize, ptr: *const u8, val: T) {
    let buffers = translated_byte_buffer(token, ptr, mem::size_of::<T>());
    let mut val_ptr = &val as *const _ as *const u8;
    for buffer in buffers {
        unsafe {
            val_ptr.copy_to(buffer.as_mut_ptr(), buffer.len());
            val_ptr = val_ptr.add(buffer.len());
        }
    }
}

/// Traslate&Copy a `ptr[u8]` array with len to value T
pub fn translated_t<T: Sized>(token: usize, ptr: *const u8, len: usize) -> T {
    let buffers = translated_byte_buffer(token, ptr, len);
    let buffers: Vec<&[u8]> = buffers.iter().map(|slice| &**slice).collect();
    convert_from_buffer(buffers)
}

/// Converts a collection of byte slices (`Vec<&[u8]>`) into a value of type `T`.
///
/// # Panics
/// Panics if the total size of the combined byte slices is smaller than the size of type `T`.
///
/// # Safety
/// This function performs an unsafe operation by interpreting raw bytes as a value of type `T`.
/// The caller must ensure that the byte slices are correctly aligned and represent a valid `T` instance.
pub fn convert_from_buffer<T>(buffers: Vec<&[u8]>) -> T {
    let mut combined: Vec<u8> = Vec::new();

    for buffer in buffers {
        combined.extend_from_slice(buffer);
    }

    assert!(
        combined.len() >= mem::size_of::<T>(),
        "Buffer is too small to hold type T"
    );

    unsafe {
        // 创建指向字节数组的指针
        let ptr = combined.as_ptr() as *const T;
        // 解引用指针，获取类型 T
        ptr.read()
    }
}
