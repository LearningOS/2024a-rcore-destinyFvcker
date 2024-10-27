//! 服务于文件相关系统调用的索引节点层的代码在 vfs.rs 中。
//!
//! EasyFileSystem 实现了磁盘布局并能够将磁盘块有效的管理起来。但是对于文件系统的使用者而言，
//! 他们往往不关心磁盘布局是如何实现的，而是更希望能够直接看到目录树结构中逻辑上的文件和目录。
//! 为此需要设计索引节点 Inode 暴露给文件系统的使用者，让他们能够直接对文件和目录进行操作。
//!
//! Inode 和 DiskInode 的区别从它们的名字中就可以看出：
//! DiskInode 放在磁盘块中比较固定的位置，而 Inode 是放在内存中的记录文件索引节点信息的数据结构。

use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use bitflags::bitflags;
use spin::{Mutex, MutexGuard};

/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,     // 对应的DiskInode保存在磁盘上的块号
    block_offset: usize, // 对应的DiskInode保存在磁盘上的块内偏移量
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Call a function over a disk inode to read it
    /// 主要用于简化对于Inode对应的磁盘上的DiskInode的访问流程
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    /// 主要用于简化对于Inode对应的磁盘上的DiskInode的访问流程
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    /// 返回目标文件的 Inode Id
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }

    // 包括find在内，所有暴露给文件系统的使用者的文件系统操作（还有接下来要介绍的几种），
    // 全程都需要持有 EasyFileSystem 的互斥锁（相对而言，文件系统内部的操作，如之前的Inode::new
    // 还是上面的 find_inode_id，都是嘉定在已经持有efs锁的情况下才被调用的，因此它们不应尝试获取
    // 锁）。这能够保证在多核情况下，同时最多只能有一个核在进行文件系统的相关操作。
    //
    // 这样也许会带来一些不必要的性能损失，但我们目前暂时先这样做。如果我们在这里加锁的话，
    // 其实就能够保证块缓存的互斥访问了。

    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                // 在这里最要注意的一点是 inode_id 不是 block_id，它们之间的粒度是不一样的
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                v.push(String::from(dirent.name()));
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }

    /// Read stat from current inode
    pub fn read_stat(&self, st: &mut Stat) {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            st.dev = 0;
            st.ino = self.block_id as u64;
            st.mode = if disk_inode.is_dir() {
                StatMode::DIR
            } else {
                StatMode::FILE
            };

            st.nlink = disk_inode.nlink as u32;
        });
    }

    /// Link a new dir entry to a file.
    /// warn: this method must be called by dir inode.
    pub fn link(&self, old_name: &str, new_name: &str) -> Result<(), &'static str> {
        let mut fs = self.fs.lock();

        let old_inode_id =
            self.read_disk_inode(|root_inode| self.find_inode_id(old_name, root_inode));

        if let Some(old_inode_id) = old_inode_id {
            let (block_id, block_offset) = fs.get_disk_inode_pos(old_inode_id);

            get_block_cache(block_id as usize, Arc::clone(&self.block_device))
                .lock()
                // Increase the `nlink` of target DiskInode
                .modify(block_offset, |n: &mut DiskInode| n.nlink += 1);

            // Insert `newname` into directory.
            self.modify_disk_inode(|root_inode| {
                let file_count = (root_inode.size as usize) / DIRENT_SZ;
                let new_size = (file_count + 1) * DIRENT_SZ;
                self.increase_size(new_size as u32, root_inode, &mut fs);
                let dirent = DirEntry::new(new_name, old_inode_id);
                root_inode.write_at(
                    file_count * DIRENT_SZ,
                    dirent.as_bytes(),
                    &self.block_device,
                );
            });

            block_cache_sync_all();
            Ok(())
        } else {
            Err("Can't find target file!")
        }
    }

    /// unlink a dir entry of a file
    pub fn unlink(&self, name: &str) -> Result<(), &'static str> {
        let mut fs = self.fs.lock();

        let mut inode_id: Option<u32> = None;
        let mut v: Vec<DirEntry> = Vec::new();

        // [destinyfvcker] 首先将要 unlink 的 inode_id 找到
        // 这里再次注意一下，inode_id 是索引节点的编号，而不是索引块的编号！
        self.modify_disk_inode(|root_inode| {
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    root_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device),
                    DIRENT_SZ,
                );
                if dirent.name() != name {
                    v.push(dirent);
                } else {
                    inode_id = Some(dirent.inode_id());
                }
            }
        });

        if let Some(inode_id) = inode_id {
            // [destinyfvcker] 修改调用 unlink 方法的目录项（实际上就是根目录）
            self.modify_disk_inode(|root_inode| {
                let size = root_inode.size;
                // 直接清空整个目录项，因为就现在的信息来说，没有能力直接找到对应的目录项并删除，
                // 所以现在的逻辑就是，清空 + 恢复，唯独不恢复要 unlink 的目录项
                //
                // 返回值是一个数组，其中包含了这个目录项所有占用的块的块号.
                let data_blocks_dealloc = root_inode.clear_size(&self.block_device);

                // 检查一下是否其他的实现发生错误
                assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);

                // 清除这些块的内容
                for data_block in data_blocks_dealloc.into_iter() {
                    fs.dealloc_data(data_block);
                }

                // 恢复
                self.increase_size((v.len() * DIRENT_SZ) as u32, root_inode, &mut fs);
                for (i, dirent) in v.iter().enumerate() {
                    root_inode.write_at(i * DIRENT_SZ, dirent.as_bytes(), &self.block_device);
                }
            });

            // Get position of old inode.
            let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);

            // Find target `DiskInode` then modify!
            get_block_cache(block_id as usize, Arc::clone(&self.block_device))
                .lock()
                .modify(block_offset, |n: &mut DiskInode| {
                    // Decrease `nlink`.
                    n.nlink -= 1;
                    // If `nlink` is zero, free all data_block through `clear_size()`.
                    if n.nlink == 0 {
                        let size = n.size;
                        let data_blocks_dealloc = n.clear_size(&self.block_device);
                        assert!(
                            data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize
                        );
                        for data_block in data_blocks_dealloc.into_iter() {
                            fs.dealloc_data(data_block);
                        }
                    }
                });

            // Since we may have writed the cached block, we need to flush the cache.
            block_cache_sync_all();
            Ok(())
        } else {
            Err("Can't find target file!")
        }
    }
}

bitflags! {
    /// The mode of a inode
    /// whether a directory or a file
    pub struct StatMode: u32 {
        /// null
        const NULL  = 0;
        /// directory
        const DIR   = 0o040000;
        /// ordinary regular file
        const FILE  = 0o100000;
    }
}

/// The state of a inode(file)
#[repr(C)]
#[derive(Debug)]
pub struct Stat {
    /// ID of device containing file
    pub dev: u64,
    /// inode number
    pub ino: u64,
    /// file type and mod
    pub mode: StatMode,
    /// number of hard links
    pub nlink: u32,
    /// unused pad
    pad: [u64; 7],
}

impl Default for Stat {
    fn default() -> Self {
        Self {
            dev: Default::default(),
            ino: Default::default(),
            mode: StatMode::NULL,
            nlink: Default::default(),
            pad: Default::default(),
        }
    }
}
