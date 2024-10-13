# lab1

-   [lab1](#lab1)
    -   [简答作业](#简答作业)
        -   [第一问](#第一问)
            -   [ch2b_bad_address.rs](#ch2b_bad_addressrs)
            -   [ch2b_bad_instructions.rs](#ch2b_bad_instructionsrs)
            -   [ch2b_bad_register.rs](#ch2b_bad_registerrs)
        -   [第二问](#第二问)
            -   [L40：刚进入 `__restore` 时，`a0` 代表了什么值。请指出 `__restore` 的两种使用情景](#l40刚进入-__restore-时a0-代表了什么值请指出-__restore-的两种使用情景)
            -   [L43-L48：这几行汇编代码特殊处理了哪些寄存器？这些寄存器的的值对于进入用户态有何意义？请分别解释](#l43-l48这几行汇编代码特殊处理了哪些寄存器这些寄存器的的值对于进入用户态有何意义请分别解释)
            -   [L50-L56：为何跳过了 `x2` 和 `x4`？](#l50-l56为何跳过了-x2-和-x4)
            -   [L60：该指令之后，`sp` 和 `sscratch` 中的值分别有什么意义？](#l60该指令之后sp-和-sscratch-中的值分别有什么意义)
            -   [`__restore`：中发生状态切换在哪一条指令？为何该指令执行之后会进入用户态？](#__restore中发生状态切换在哪一条指令为何该指令执行之后会进入用户态)
            -   [L13：该指令之后，`sp` 和 `sscratch` 中的值分别有什么意义？](#l13该指令之后sp-和-sscratch-中的值分别有什么意义)
            -   [从 U 态进入 S 态是哪一条指令发生的？](#从-u-态进入-s-态是哪一条指令发生的)
    -   [荣誉准则](#荣誉准则)

## 简答作业

### 第一问

首先来看输出：可以发现这三个 bad 测例在程序开始时就会被加载运行，然后立即因为相对应的致命错误退出，此时操作系统应该会调用 `TASK_MANAGER` 的 `exit_current_and_run_next()` 函数，
加载下一个 task 运行（实际上这里）也不好说是“加载下一个程序运行”，因为应用程序实际上是被直接链接在操作系统内核之中的，在操作系统被加载到内存之后，用户态程序就已经跟着
存在在内存之中了。

```Text
[kernel] PageFault in application, bad addr = 0x0, bad instruction = 0x804003a4, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
```

#### ch2b_bad_address.rs

先来看看第一个出错的用户程序里头写了什么：

```Rust
#![no_std]
#![no_main]

extern crate user_lib;

/// 由于 rustsbi 的问题，该程序无法正确退出
/// > rustsbi 0.2.0-alpha.1 已经修复，可以正常退出

#[no_mangle]
pub fn main() -> isize {
    unsafe {
        #[allow(clippy::zero_ptr)]
        (0x0 as *mut u8).write_volatile(0);
    }
    panic!("FAIL: T.T\n");
}
```

可以很明显地发现这是在尝试向内存 0 地址写值，此时内核应该进入异常控制流，在书上可以发现：异常控制流可以分为中断和异常，而异常又可以分为 **故障**、**陷阱** 和 **终止**。
中断是异步的，异常是同步的，所以这应该归于异常；并且这是不可恢复的致命错误，所以是异常之中的终止。

此时 CPU 会给自己一个中断号，然后跳转到我们在这里编写的简单“中断描述符表”之中：

```Rust
/// trap handler
#[no_mangle]
pub fn trap_handler(cx: &mut TrapContext) -> &mut TrapContext {
    let scause = scause::read(); // get trap cause
    let stval = stval::read(); // get extra value
                               // trace!("into {:?}", scause.cause());
    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            // ...
        }
        Trap::Exception(Exception::StoreFault) | Trap::Exception(Exception::StorePageFault) => {
            println!("[kernel] PageFault in application, bad addr = {:#x}, bad instruction = {:#x}, kernel killed it.", stval, cx.sepc);
            exit_current_and_run_next();
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            println!("[kernel] IllegalInstruction in application, kernel killed it.");
            exit_current_and_run_next();
        }
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            set_next_trigger();
            suspend_current_and_run_next();
        }
        _ => {
            panic!(
                "Unsupported trap {:?}, stval = {:#x}!",
                scause.cause(),
                stval
            );
        }
    }
    cx
}
```

这是一个存储错误，所以可以可以读取 scause 寄存器之中的值来进入对应的处理分支，打印对应的错误信息，然后切到下一个用户程序运行。

#### ch2b_bad_instructions.rs

```Rust
#![no_std]
#![no_main]

extern crate user_lib;

/// 由于 rustsbi 的问题，该程序无法正确退出
/// > rustsbi 0.2.0-alpha.1 已经修复，可以正常退出

#[no_mangle]
pub fn main() -> ! {
    unsafe {
        core::arch::asm!("sret");
    }
    panic!("FAIL: T.T\n");
}
```

先来复习一下在 rCore-Tutorial-Book-v3 之中的关于 `sret` 之中的知识吧：

> 而当 CPU 完成 Trap 处理准备返回的时候，需要通过一条 S 特权级的特权指令 sret 来完成，这一条指令具体完成以下功能：
>
> -   CPU 会将当前的特权级按照 sstatus 的 SPP 字段设置为 U 或者 S ；
> -   CPU 会跳转到 sepc 寄存器指向的那条指令，然后继续执行。

所以这是一个专门从 S 特权级返回的指令，而用户程序肯定只是出于 U 特权级，无权执行 S 特权级的指令，这也是一个没有办法恢复的 **异常 - 终止**。
然后 CPU 给自己一个中断号，流程还是和上面的一样。

#### ch2b_bad_register.rs

```Rust
#![no_std]
#![no_main]

extern crate user_lib;

/// 由于 rustsbi 的问题，该程序无法正确退出
/// > rustsbi 0.2.0-alpha.1 已经修复，可以正常退出

#[no_mangle]
pub fn main() -> ! {
    let mut sstatus: usize;
    unsafe {
        core::arch::asm!("csrr {}, sstatus", out(reg) sstatus);
    }
    panic!("(-_-) I get sstatus:{:x}\nFAIL: T.T\n", sstatus);
}
```

尝试跨特权级读取控制状态寄存器 sstatus，这就不用我多说了吧？

### 第二问

#### L40：刚进入 `__restore` 时，`a0` 代表了什么值。请指出 `__restore` 的两种使用情景

这里先说说 `__restore` 的两种使用场景吧：

1. 用于在系统调用之中从系统态恢复到用户态
2. 用于实现分时多任务的切换

关于刚进入 `__restore` 时，`a0` 代表的值，说实话一开始我有点懵，这是什么意思捏？在第二章：批处理系统之中，确实是使用了 `__restore` 来作为函数进行调用的：

```Rust
// os/src/batch.rs

pub fn run_next_app() -> ! {
    let mut app_manager = APP_MANAGER.exclusive_access();
    let current_app = app_manager.get_current_app();
    unsafe {
        app_manager.load_app(current_app);
    }
    app_manager.move_to_next_app();
    drop(app_manager);
    // before this we have to drop local variables related to resources manually
    // and release the resources
    extern "C" { fn __restore(cx_addr: usize); }
    unsafe {
        __restore(KERNEL_STACK.push_context(
            TrapContext::app_init_context(APP_BASE_ADDRESS, USER_STACK.get_sp())
        ) as *const _ as usize);
    }
    panic!("Unreachable in batch::run_current_app!");
}
```

此时 `a0` 寄存器之中存储的就是函数调用的参数 —— 一个指向构造出的内核栈上的下一个运行任务的任务上下文结构 `TrapContext` 的指针，此时在 `__restore`
过程之中也确实有对应的处理操作：

```asm
__restore:
    # case1: start running app by __restore
    # case2: back to U after handling trap
    mv sp, a0
    # now sp->kernel stack(after allocated), sscratch->user stack
    # restore sstatus/sepc
    ld t0, 32*8(sp)
    ld t1, 33*8(sp)
    ld t2, 2*8(sp)
    csrw sstatus, t0
    csrw sepc, t1
    csrw sscratch, t2
    # restore general-purpuse registers except sp/tp
    ld x1, 1*8(sp)
    ld x3, 3*8(sp)
    .set n, 5
    .rept 27
        LOAD_GP %n
        .set n, n+1
    .endr
    # release TrapContext on kernel stack
    addi sp, sp, 34*8
    # now sp->kernel stack, sscratch->user stack
    csrrw sp, sscratch, sp
    sret
```

而在第二章处理 trap 返回的过程之中，`mv sp, a0` 实际上是没有作用的，在 `trap.S`之中，实际上 `__alltraps` 和 `__restore` 都是连续存放的，
所以在 `__alltraps` 最后一行汇编 `call trap_handler` 的下一行代码就是 `__restore` 过程的一行代码，此时 `a0` 里面存的就是在 `call trap_handler`
上一行之中的 `mv a0, sp`.

实际在这里还有一个含义：`trap_handler` 实际上是一个函数，所以这里需要给它一个参数，怎么把这个参数传给它呢？就是通过这个 `a0` 寄存器，此时实际上可以理解为
它里面存放了指向 `TaskContext` 的指针。

```Rust
/// trap handler
#[no_mangle]
pub fn trap_handler(cx: &mut TrapContext) -> &mut TrapContext {
    // ...
}
```

> 但是我们在上面讨论的都是在第二章：批处理系统之中的内容，实际上在第三章：多道程序与分时多任务之中，相关的代码有变动：

首先就是我们现在不再将 `__restore` 来作为函数进行调用了，而是调用的新添加的 `__switch`，这个 `__switch` 过程的两个参数分别是 **指向被切换出的任务的 TaskContext 的指针**  
和 **指向待切换入的任务的 TaskContext 的指针**，而此时 `__restore` 是存储在 `TaskContext` 之中的：

```Rust
// context.rs

#[derive(Copy, Clone)]
#[repr(C)]
/// task context structure containing some registers
pub struct TaskContext {
    /// Ret position after task switching
    ra: usize,
    /// Stack pointer
    sp: usize,
    /// s0-11 register, callee saved
    s: [usize; 12],
}

impl TaskContext {
    /// Create a new task context with a trap return addr and a kernel stack pointer
    pub fn goto_restore(kstack_ptr: usize) -> Self {
        extern "C" {
            fn __restore();
        }
        Self {
            ra: __restore as usize,
            sp: kstack_ptr,
            s: [0; 12],
        }
    }
}
```

`ra` 就是在函数调用结束之后应该返回到的地址，这个 `ra` 在程序第一次运行之前会使用 `goto_restore` 进行初始化，而在 `goto_restore` 之中，`ra` 被初始化为 `__restore`。

所以在程序第一次被 `__switch` 切换到之后， 最后的 `ret` 指令会跳转到 `__restore` 的位置继续执行。

由于 `ret` 指令实际上根本就不会影响任何寄存器的值，所以此时 `a0` 寄存器之中的值应该还是等于传给 `__switch` 的指针。

在之后的时钟中断过程之中，ra 寄存器的值会发生改变,这实际上是一个非常长的函数调用过程，在第三章之中多道程序与分时多任务的实际上是这样的：

和使用系统调用一样，实际上时钟中断就是自己给自己一个中断号，最后都是要走 **trap 模块** 之中的 `trap_handler` 逻辑的，所以实际上，这里的控制流实际上是这样进行转换的：`time interrupt` -> `trap_handler` -> `suspend_current_and_run_next` -> `TaskManager::run_next_task` -> `__switch`.

在 `__switch` 之中会把 `ra` 寄存器的值保存到 `TaskContext` 之中。

所以一开始是从 `trap_handler` 之中被切出去的，最后回到哪了呢，就是回到了 `TaskManager::run_next_task` 之中，然后函数一层一层返回到 `trap_handler` 之中，最后从 `call trap_handler` 之中返回，进入 `__restore` 之中。

此时 `a0` 寄存器的值应该会继承在 `__alltraps` 之中设置的值, 也就是栈顶指针。

<!-- 由于 `__switch` 是我们手写的汇编函数，编译器并不能插入相关的保存寄存器的代码，所以在 `__switch` 之中需要我们自己去恢复相关的寄存器。
而在 RISC-V 架构的调用约定之中，`a0` 寄存器属于调用者保存寄存器，应该在 `__switch` 调用之前就保存在了内核栈上，此时 `a0` 之中存储的应该是相关函数的参数的值 -->

#### L43-L48：这几行汇编代码特殊处理了哪些寄存器？这些寄存器的的值对于进入用户态有何意义？请分别解释

处理了三个 **CSR 寄存器**，对于进入用户态十分重要：

对于 `CSR` 而言，我们知道进入 Trap 的时候，硬件会立即覆盖掉 `scause/stval/sstatus/sepc` 的全部或是其中一部分。

`scause/stval` 的情况是：它总是在 Trap 处理的第一时间就被使用或者是在其他地方保存下来了，因此它没有被修改并造成不良影响的风险。 而对于 `sstatus/sepc` 而言，它们会在 Trap 处理的全程有意义（在 Trap 控制流最后 `sret` 的时候还用到了它们），而且确实会出现 Trap 嵌套的情况使得它们的值被覆盖掉。所以我们需要将它们也一起保存下来，并在 sret 之前恢复原样。

#### L50-L56：为何跳过了 `x2` 和 `x4`？

在这部分的代码之中，实际上做的是恢复在内核栈上存放的用户态程序的上下文信息，`x2` 也就是 `sp` 寄存器，实际上它在 L48 之中已经从 `t2` 恢复到了 `sscratch` 之中，这里没有必要再进行相关的恢复操作。

而 `x4` 则是栈帧寄存器，再这里没有用到，所以没有相应的恢复/保存操作。

#### L60：该指令之后，`sp` 和 `sscratch` 中的值分别有什么意义？

sp 重新指向用户栈栈顶，sscratch 也依然保存进入 Trap 之前的状态并指向内核栈栈顶。

#### `__restore`：中发生状态切换在哪一条指令？为何该指令执行之后会进入用户态？

sret 指令。

这一条指令具体完成以下功能：

-   CPU 会将当前的特权级按照 sstatus 的 SPP 字段设置为 U 或者 S
-   CPU 会跳转到 sepc 寄存器指向的那条指令，然后继续执行

这些基本上都是硬件不得不完成的事情，还有一些剩下的收尾工作可以都交给软件，让操作系统能有更大的灵活性。

#### L13：该指令之后，`sp` 和 `sscratch` 中的值分别有什么意义？

`sp` 现在指向对应用户态程序的内核栈，而 `sscratch` 之中保存了对应用户态程序的用户态指针

#### 从 U 态进入 S 态是哪一条指令发生的？

ecall

## 荣誉准则

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

    我没有和任何人进行交流

2. 此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    - rCore-Gamp-Guide-2024A 文档
    - rCore-Tutorial- Book-v3 3.6.0- alpha.1 文档

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
