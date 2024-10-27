# 实验三报告-chapter5 练习

-   [实验三报告-chapter5 练习](#实验三报告-chapter5-练习)
    -   [实现总结](#实现总结)
    -   [简答作业](#简答作业)
        -   [问题分析](#问题分析)
            -   [1. stride 算法轮转问题分析](#1-stride-算法轮转问题分析)
            -   [2. STRIDE_MAX - STRIDE_MIN 问题分析](#2-stride_max---stride_min-问题分析)
        -   [代码实现](#代码实现)
        -   [代码解释](#代码解释)
    -   [荣誉准则](#荣誉准则)

## 实现总结

spawn 实际上是直接新建一个 TaskControlBlock，然后修改它的 parent，并在 parent 的 childern 列表之中加入它

stride 调度算法方面我通过 BinaryHeap 实现了一个小顶堆，每次从堆顶获取最小的元素返回就可以了

## 简答作业

### 问题分析

#### 1. stride 算法轮转问题分析

在 stride 调度算法中，每个进程有一个 stride 和 pass 值。pass 值是进程执行时间的累计值。进程的 pass 值越小，它越有优先权在下一轮中被选中执行。

-   设进程 p1 的 stride 为 255，进程 p2 的 stride 为 250。
-   当 p2 执行一个时间片后，其 pass 值增加 250。
-   初始状态下，假设 p1.pass = 0 和 p2.pass = 0。
-   经过一个时间片后，p2.pass = 250，而 p1.pass 仍然为 0。

因此，在下一轮调度时，由于 p1.pass (0) < p2.pass (250)，理论上轮到 p1 执行。

#### 2. STRIDE_MAX - STRIDE_MIN 问题分析

对于任意两个进程 \( p_1 \) 和 \( p_2 \) 的 stride 值，如果我们保证所有进程的优先级（stride 值的倒数）都不小于 2，那么我们可以确保：

\[ \text{STRIDE_MAX} - \text{STRIDE_MIN} \leq \frac{\text{BigStride}}{2} \]

理由如下：

-   假设 \( \text{BigStride} \) 是 stride 的最大表示范围。
-   在不考虑溢出的情况下，任何进程的 stride 值的差值不会超过 \( \text{BigStride} / 2 \)，因为最小的 stride 值是 2（当优先级为 2 时），最大 stride 值是 \( \text{BigStride} \)。

当优先级至少为 2 时，stride 值范围限制在 \( [2, \text{BigStride}] \)。保证最大差值不超过 \( \frac{\text{BigStride}}{2} \) 的原因是由于算法设计，在每次调度选择时，较小的 stride 值和较大的 stride 值之间的 pass 值差不会超过 \( \text{BigStride} / 2 \)。

### 代码实现

考虑溢出的情况下，我们需要设计一个比较器，让二叉堆能够正确比较 stride。我们需要处理无符号整型溢出的问题。具体方法是利用无符号整型的性质（模 256），进行比较时只需考虑高 7 位（忽略最低位的溢出情况）。

实现代码如下：

```rust
use core::cmp::Ordering;

struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // 使用 8 bits 存储 stride
        // 计算相对差值, 需要考虑溢出
        let max_stride = 255u64; // 表示 BigStride
        let half_max_stride = max_stride / 2;

        let self_val = self.0 & max_stride;
        let other_val = other.0 & max_stride;

        if self_val < other_val && other_val - self_val <= half_max_stride {
            Some(Ordering::Less)
        } else if self_val > other_val && self_val - other_val <= half_max_stride {
            Some(Ordering::Greater)
        } else if self_val < other_val {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Less)
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        false
    }
}

fn main() {
    let stride1 = Stride(255);
    let stride2 = Stride(250);

    assert_eq!(stride1.partial_cmp(&stride2), Some(Ordering::Greater));
    assert_eq!(stride2.partial_cmp(&stride1), Some(Ordering::Less));

    println!("Comparison works correctly.");
}
```

### 代码解释

-   `partial_cmp` 函数：比较两个 stride 值，考虑到 8-bit 存储的溢出情况。使用模 256（`max_stride`）处理溢出，通过比较高 7 位（忽略最低位的溢出情况）来决定顺序。
-   如果两个 stride 之间的差值不超过 \( \text{BigStride} / 2 \)，正常比较。
-   如果差值超过 \( \text{BigStride} / 2 \)，则反向比较结果来处理溢出情况。

这个比较器保证了即使在存在溢出的情况下，也能正确地比较和选择最小的 stride，从而实现正确的调度顺序。

## 荣誉准则

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

    - 我并没有和任何人进行交流，单独完成了此次任务

2. 此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    - rCore-Tutorial- Book-v3 3.6.0- alpha.1 文档

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
