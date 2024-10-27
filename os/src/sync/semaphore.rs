//! Semaphore

use crate::sync::UPSafeCell;
use crate::task::{block_current_and_run_next, current_task, wakeup_task, TaskControlBlock};
use alloc::{collections::VecDeque, sync::Arc};

/// semaphore Id
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct SemId(pub usize);

/// semaphore structure
pub struct Semaphore {
    /// semaphore id
    pub sem_id: SemId,
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(sem_id: usize, res_count: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            sem_id: SemId(sem_id),
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }

    /// up operation of semaphore
    pub fn up(&self) {
        trace!("kernel: Semaphore::up");
        let mut inner = self.inner.exclusive_access();
        inner.count += 1;

        let current_task = current_task().unwrap();
        let task_inner = current_task.inner_exclusive_access();

        if inner.count <= 0 {
            if let Some(task) = inner.wait_queue.pop_front() {
                wakeup_task(task);
            }
        }

        drop(task_inner);
        drop(current_task);

        // [destinyfvcker] 关于为什么这里是看到 inner.count 小于 0 才触发，
        // 因为只有 inner.count <= 0 才会有线程阻塞在这个信号量之中.
        // 当然也可以使用对应等待队列的长度来进行判断.
        if inner.count <= 0 {
            // 这里实际上是对对应线程的 need 向量进行一个管理，就是将对应的 need 值 -1，然后不要忘记了将对应的 allocation 值加一
            if let Some(task) = inner.wait_queue.pop_front() {
                let mut task_inner = task.inner_exclusive_access();

                if let Some((index, sem_count)) = task_inner
                    .need
                    .iter_mut()
                    .enumerate()
                    .find(|(_, (sem_id, _))| *sem_id == self.sem_id)
                {
                    sem_count.1 -= 1;
                    if sem_count.1 <= 0 {
                        task_inner.need.remove(index);
                    }
                } else {
                    panic!("[destinyfvcker] ======== there should be a need item be registed! ========");
                }

                if let Some((_, alloc_count)) = task_inner
                    .allocation
                    .iter_mut()
                    .find(|(sem_id, _)| *sem_id == self.sem_id)
                {
                    *alloc_count += 1;
                } else {
                    task_inner.allocation.push((self.sem_id, 1));
                }

                drop(task_inner);
                wakeup_task(task);
            }
        }
    }

    /// down operation of semaphore
    /// down operation of semaphore
    pub fn down(&self) {
        trace!("kernel: Semaphore::down");
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;

        // let current_task = current_task().unwrap();
        // let mut task_inner = current_task.inner_exclusive_access();

        // drop(task_inner);
        // drop(current_task);
        let current_task = current_task().unwrap();
        let mut task_inner = current_task.inner_exclusive_access();

        if inner.count < 0 {
            if let Some(sem_count) = task_inner
                .need
                .iter_mut()
                .find(|(sem_id, _)| *sem_id == self.sem_id)
            {
                sem_count.1 += 1;
            } else {
                task_inner.need.push((self.sem_id.clone(), 1))
            }

            drop(task_inner);
            inner.wait_queue.push_back(current_task);
            drop(inner);
            block_current_and_run_next();
        } else {
            if let Some(alloc_count) = task_inner
                .allocation
                .iter_mut()
                .find(|(sem_id, _)| *sem_id == self.sem_id)
            {
                alloc_count.1 += 1;
            } else {
                task_inner.allocation.push((self.sem_id.clone(), 1));
            }
        }
    }
}
