//! 在 easy-fs 布局中存在两类不同的位图，分别对索引节点和数据块进行管理。每个位图都由若干个块组成，每个块大小为 512 bytes，即 4096 bits。每个 bit 都代表一个索引节点/数据块的分配状态，
//! 0 意味着未分配，而 1 则意味着已经分配出去。位图所要做的事情是通过基于 bit 为单位的分配（寻找一个为 0 的bit位并设置为 1）和回收（将bit位清零）来进行索引节点/数据块的分配和回收。

use super::{get_block_cache, BlockDevice, BLOCK_SZ};
use alloc::sync::Arc;

/// A bitmap block
///
/// 将位图区域之中的一个磁盘块解释为长度为64的一个 u64 数组，
/// 每个 u64 打包了一组 64 bits，于是整个数组包含 64x64 = 4096 bits
/// 并且可以以组为单位进行操作
type BitmapBlock = [u64; 64];
/// Number of bits in a block
const BLOCK_BITS: usize = BLOCK_SZ * 8;

/// A bitmap
///
/// 驻留在内存之中，表示索引节点/数据块区域之中的那些磁盘块的分配情况
pub struct Bitmap {
    /// 所在区域的起始块编号
    start_block_id: usize,
    /// 区域的长度为多少个块
    blocks: usize,
}

/// Decompose bits into (block_pos, bits64_pos, inner_pos)
///
/// 将bit编号 bit 分解为区域中的块编号 block_pos 、块内的组编号 bits64_pos
/// 以及组内编号 inner_pos 的三元组，这样就能精确定位待回收的bit
fn decomposition(mut bit: usize) -> (usize, usize, usize) {
    let block_pos = bit / BLOCK_BITS;
    bit %= BLOCK_BITS;
    (block_pos, bit / 64, bit % 64)
}

impl Bitmap {
    /// A new bitmap from start block id and number of blocks
    pub fn new(start_block_id: usize, blocks: usize) -> Self {
        Self {
            start_block_id,
            blocks,
        }
    }

    /// Allocate a new block from a block device.
    ///
    /// This function tries to find a free block from the block bitmap and allocate it.
    /// The function works by iterating over each block in the bitmap,
    /// checking whether there are any available bits (which correspond to free blocks),
    /// and if a free bit is found, it marks it as allocated.
    ///
    /// # Arguments
    ///
    /// - `&self`: A reference to the current bitmap structure that manages block allocations.
    /// This structure contains information about the total number of blocks (`self.blocks`)
    /// and the starting block ID (`self.start_block_id`).
    /// - `block_device`: A reference-counted (`Arc`) trait object representing the block device
    /// on which the bitmap and block data reside. The `BlockDevice` trait must be implemented
    /// by the underlying block device.
    ///
    /// # Returns
    ///
    /// - `Option<usize>`: Returns `Some(usize)` containing the index of the allocated block if a
    /// free block is successfully allocated. Returns `None` if no free block is found.
    ///
    /// # Behavior
    ///
    /// 1. The function iterates over all blocks managed by the bitmap (`self.blocks`).
    /// 2. For each `block_id`, it retrieves the corresponding bitmap block using `get_block_cache`.
    ///    - `get_block_cache` provides access to the cached block that tracks the allocation status of a group of blocks.
    ///    - The bitmap block is locked for modification (via `lock().modify`) to ensure thread safety.
    /// 3. The bitmap block (`BitmapBlock`) is a collection of 64-bit unsigned integers (`u64`), where each
    ///    bit represents the allocation status of a specific block.
    ///    - A value of `0` means the block is free, while `1` indicates the block is already allocated.
    /// 4. The function scans through each 64-bit value in the bitmap block to find the first one that has any
    ///    free bits (i.e., not all bits are set to `1`).
    ///    - It uses `find` and `enumerate` to locate the first `u64` that has at least one bit set to `0` (free block).
    ///    - If such a `u64` is found, the position of the first free bit within the 64-bit value is determined using `trailing_ones()`.
    /// 5. Once a free bit is found:
    ///    - The corresponding bit in the bitmap block is marked as allocated by setting the bit to `1` (via a
    pub fn alloc(&self, block_device: &Arc<dyn BlockDevice>) -> Option<usize> {
        for block_id in 0..self.blocks {
            let pos = get_block_cache(
                block_id + self.start_block_id as usize,
                Arc::clone(block_device),
            )
            .lock()
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                if let Some((bits64_pos, inner_pos)) = bitmap_block
                    .iter()
                    .enumerate()
                    .find(|(_, bits64)| **bits64 != u64::MAX)
                    .map(|(bits64_pos, bits64)| (bits64_pos, bits64.trailing_ones() as usize))
                {
                    // modify cache
                    bitmap_block[bits64_pos] |= 1u64 << inner_pos;
                    // 返回分配的bit所在的位置，等同于索引节点/数据块的编号
                    Some(block_id * BLOCK_BITS + bits64_pos * 64 + inner_pos as usize)
                } else {
                    None
                }
            });
            if pos.is_some() {
                return pos;
            }
        }
        None
    }

    /// Deallocate a block
    pub fn dealloc(&self, block_device: &Arc<dyn BlockDevice>, bit: usize) {
        // dealloc 方法首先调用 decomposition 函数将bit编号 bit 分解为区域中的块编号 block_pos 、
        // 块内的组编号 bits64_pos 以及组内编号 inner_pos 的三元组，这样就能精确定位待回收的bit，随后将其清零即可。
        let (block_pos, bits64_pos, inner_pos) = decomposition(bit);
        get_block_cache(block_pos + self.start_block_id, Arc::clone(block_device))
            .lock()
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                assert!(bitmap_block[bits64_pos] & (1u64 << inner_pos) > 0);
                bitmap_block[bits64_pos] -= 1u64 << inner_pos; // ? 话说清零原来是这样清的吗？
            });
    }

    /// Get the max number of allocatable blocks
    pub fn maximum(&self) -> usize {
        self.blocks * BLOCK_BITS
    }
}
