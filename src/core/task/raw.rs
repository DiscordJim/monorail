use std::{
    any::Any, cell::UnsafeCell, future::Future, marker::PhantomData, mem::MaybeUninit, panic::AssertUnwindSafe, pin::Pin, task::{Context, Poll}
};

use crate::core::shard::state::ShardId;

pub type Task = Box<dyn FnOnce() + Send + 'static>;

pub enum TaskControlHeader {
    FireAndForget,
    WithReturn { origin: ShardId, ticket: usize },
}

pub struct Init;
pub struct Pollable<'a>(PhantomData<&'a ()>);

pub struct Panicked;
pub struct Ready;

#[repr(C)]
pub struct TaskControlBlock<T, A, F, O> {
    header: TaskControlHeader,
    allocation: TaskAllocation<T, F, O>,
    _marker: PhantomData<A>
}

#[inline]
unsafe fn task_read_future<'a, T, A: 'a, F, O>(ptr: *const ()) -> &'a mut TaskControlBlock<T, A, F, O>
where
    T: FnOnce(&'a mut A) -> F + 'static,
    F: Future<Output = O> + 'a,
    O: 'a,
{
    unsafe { &mut *ptr.cast_mut().cast() }
}

// #[inline]
fn task_poll<'a, T, A: 'a, F, O>(ptr: *const (), ctx: &mut Context<'_>) -> (Poll<()>, bool)
where
    T: FnOnce(&'a mut A) -> F + 'static,
    F: Future<Output = O> + 'a,
    O: 'a,
{
    unsafe {
        let future = task_read_future::<T, A, F, O>(ptr);

        let TaskAllocation::Future(o) = &mut future.allocation else {
            panic!("not fut.");
        };

        let pinned = Pin::new_unchecked(o);
        match std::panic::catch_unwind(AssertUnwindSafe(|| pinned.poll(ctx))) {
            Ok(poll) => match poll {
                Poll::Pending => (Poll::Pending, true),
                Poll::Ready(o) => {
                    future.allocation = TaskAllocation::Result(o);
                    (Poll::Ready(()), true)
                }
            },
            Err(e) => {
                future.allocation = TaskAllocation::Panic(e);
                (Poll::Ready(()), false)
            }
        }
        // match pinned.poll(ctx) {
        //     Poll::Pending => (Poll::Pending, true),
        //     Poll::Ready(o) => {
        //         future.allocation = TaskAllocation::Result(o);
        //         Poll::Ready(())
        //     }
        // }
    }
}

// #[inline]
fn task_move_result<'a, T, A: 'a, F, O>(ptr: *const (), slot: *mut MaybeUninit<()>, is_panic: bool)
where
    T: FnOnce(&'a mut A) -> F + 'static,
    F: Future<Output = O> + 'a,
    O: 'a,
{
    unsafe {
        if is_panic {
            let future = task_read_future::<T, A, F, O>(ptr);
            let sl = std::mem::replace(&mut future.allocation, TaskAllocation::Empty);
            let TaskAllocation::Panic(o) = sl else {
                panic!("R")
            };

            (&mut *slot.cast::<MaybeUninit<Box<dyn Any + Send + 'static>>>()).write(o);
        } else {
            let future = task_read_future::<T, A, F, O>(ptr);
            let sl = std::mem::replace(&mut future.allocation, TaskAllocation::Empty);
            let TaskAllocation::Result(o) = sl else {
                panic!("R")
            };

            (&mut *slot.cast::<MaybeUninit<O>>()).write(o);
        }
    }
}

// #[inline]
fn task_control_drop<'a, T, A: 'a, F, O>(ptr: *const ())
where
    T: FnOnce(&'a mut A) -> F + 'static,
    F: Future<Output = O> + 'a,
    O: 'a,
{
    unsafe {
        let _ = Box::<TaskControlBlock<T, A, F, O>>::from_raw(ptr.cast_mut().cast());
    }
}

// #[inline]
fn task_execute<'a, T, A: 'a, F, O>(ptr: *const (), args: *const ()) -> bool
where
    T: FnOnce(&'a mut A) -> F + 'static,
    F: Future<Output = O> + 'a,
    O: 'a,
{
    unsafe {
        let mandrop = task_read_future::<T, A, F, O>(ptr);
        let TaskAllocation::Functor(func) =
            std::mem::replace(&mut mandrop.allocation, TaskAllocation::Empty)
        else {
            panic!("Failed to remove the functor.");
        };

        let arg_ref = &mut *args.cast_mut().cast::<A>();

        match std::panic::catch_unwind(AssertUnwindSafe(|| func(arg_ref))) {
            Ok(a) => {
                mandrop.allocation = TaskAllocation::Future(a);
                true
            }
            Err(e) => {
                mandrop.allocation = TaskAllocation::Panic(e);
                false
            }
        }
    }
}

impl<'a, T, A: 'a, FUT, O> TaskControlBlock<T, A, FUT, O>
where
    T: FnOnce(&'a mut A) -> FUT + 'static,
    FUT: Future<Output = O> + 'a,
    O: 'static,
{
    pub(crate) fn create_local(header: TaskControlHeader, functor: T) -> TaskControlBlockVTable<Init> {
        let block = Box::new(TaskControlBlock {
            header,
            allocation: TaskAllocation::<T, FUT, O>::Functor(functor),
            _marker: PhantomData::<A>
        });

        TaskControlBlockVTable {
            payload: Box::into_raw(block).cast(),
            // read_header: |payload|
            drop: |payload| task_control_drop::<T, A, FUT, O>(payload),
            run: |payload, args| task_execute::<T, A, FUT, O>(payload, args),
            move_result: |payload, slot, is_panic| {
                task_move_result::<T, A, FUT, O>(payload, slot, is_panic)
            },
            poll: |payload, ctx| task_poll::<T, A, FUT, O>(payload, ctx),
            owns: true,
            _state: PhantomData,
        }
    }
    
    // pub fn run(self) ->
}

impl<T, FUT, O> TaskControlBlock<T, (), FUT, O>
where
    T: FnOnce(&mut ()) -> FUT + Send + 'static,
    FUT: Future<Output = O> + 'static,
    O: Send + 'static,
{
    pub(crate) fn create(header: TaskControlHeader, functor: T) -> TaskControlBlockVTable<Init> {
        let block = Box::new(TaskControlBlock {
            header,
            allocation: TaskAllocation::<T, FUT, O>::Functor(functor),
            _marker: PhantomData::<()>
        });

        TaskControlBlockVTable {
            payload: Box::into_raw(block).cast(),
            // read_header: |payload|
            drop: |payload| task_control_drop::<T, (),  FUT, O>(payload),
            run: |payload, args| task_execute::<T, (), FUT, O>(payload, args),
            move_result: |payload, slot, is_panic| {
                task_move_result::<T, (), FUT, O>(payload, slot, is_panic)
            },
            poll: |payload, ctx| task_poll::<T, (), FUT, O>(payload, ctx),
            owns: true,
            _state: PhantomData,
        }
    }
    
    // pub fn run(self) ->
}

pub(crate) unsafe fn build_tcb_vtable<'a, T, A, F, O>(
    header: TaskControlHeader,
    task: T
) -> TaskControlBlockVTable<Init>
where 
    T: FnOnce(&'a mut A) -> F + 'static,
    A: 'a,
    F: Future<Output = O> + 'a,
    O: 'a
{
    let block = Box::new(TaskControlBlock {
            header,
            allocation: TaskAllocation::<T, F, O>::Functor(task),
            _marker: PhantomData::<A>
        });

        TaskControlBlockVTable {
            payload: Box::into_raw(block).cast(),
            // read_header: |payload|
            drop: |payload| task_control_drop::<T, A, F, O>(payload),
            run: |payload, args| task_execute::<T, A, F, O>(payload, args),
            move_result: |payload, slot, is_panic| {
                task_move_result::<T, A, F, O>(payload, slot, is_panic)
            },
            poll: |payload, ctx| task_poll::<T, A, F, O>(payload, ctx),
            owns: true,
            _state: PhantomData,
        }

}

impl<M> TaskControlBlockVTable<M> {
    pub fn retype<N>(&self) -> TaskControlBlockVTable<N> {
        TaskControlBlockVTable {
            payload: self.payload,
            run: self.run,
            move_result: self.move_result,
            drop: self.drop,
            owns: true,
            _state: PhantomData,
            poll: self.poll,
        }
    }
}

impl TaskControlBlockVTable<Init> {
    pub unsafe fn run_without_args(mut self) -> Result<TaskControlBlockVTable<Pollable<'static>>, TaskControlBlockVTable<Panicked>> {
        static NULL_PLACEHOLDER: () = ();
        // static NULL_PLACEHOLDER: UnsafeCell<()> = const { UnsafeCell::new(()) };
        unsafe {
            // let null = ()
            let mut null = ();
            std::mem::transmute(self.run::<()>(&mut null))
        }
    }
    pub unsafe fn run<'a, A>(
        mut self,
        args: &'a mut A
    ) -> Result<TaskControlBlockVTable<Pollable<'a>>, TaskControlBlockVTable<Panicked>> {
        let reso = if (self.run)(self.payload, args as *mut _ as *const ()) {
            Ok(self.retype::<Pollable>())
        } else {
            Err(self.retype::<Panicked>())
        };
        self.owns = false;
        reso
        // let out = TaskControlBlockVTable {
        //     drop: self.drop,
        //     payload: self.payload,
        //     poll: self.poll,
        //     run: self.run,
        //     move_result: self.move_result,
        //     owns: true,
        //     _state: PhantomData,
        // };
        // // std::mem::forget(self);

        // self.owns = false;
        // out
    }
}

impl<'a> Future for TaskControlBlockVTable<Pollable<'a>> {
    type Output = Result<TaskControlBlockVTable<Ready>, TaskControlBlockVTable<Panicked>>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let (poll, is_healthy) = (self.poll)(self.payload, cx);

        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(_) => {
                // self.owns = false;

                let reso = if is_healthy {
                    Ok(self.retype::<Ready>())
                } else {
                    Err(self.retype::<Panicked>())
                };

                self.owns = false;

                Poll::Ready(reso)

                // let o = Poll::Ready(TaskControlBlockVTable {
                //     drop: self.drop,
                //     _state: PhantomData,
                //     payload: self.payload,
                //     poll: self.poll,
                //     owns: true,
                //     move_result: self.move_result,
                //     run: self.run,
                // });
                // self.owns = false;
                // o
            }
        }
    }
}

/// A tagged union representing the internal state
/// of the task control block.
pub enum TaskAllocation<T, F, O> {
    Functor(T),
    Future(F),
    Result(O),
    Panic(Box<dyn Any + Send + 'static>),
    Empty,
}

// impl Con/

pub(crate) struct TaskControlBlockVTable<M> {
    /// The actual task control block, which has
    /// had it's type erased. This is always allocated
    /// via box.
    payload: *const (),
    /// Executes the function, turning the internal
    /// control block state machine into a future.
    run: fn(*const (), *const ()) -> bool,
    /// Polls the future produced by the function,
    /// returning the result. On [Poll::Ready] it will
    /// have automatically stored the result.
    poll: fn(*const (), &mut Context<'_>) -> (Poll<()>, bool),
    /// Moves the result from the [TaskControlBlock]
    /// into the [MaybeUninit] field, allowing for the extraction
    /// of the result.
    move_result: fn(*const (), *mut MaybeUninit<()>, bool),
    /// Drops the [TaskControlBlock] pointed to by this VTable.
    drop: fn(*const ()),
    /// Indicates whether we own the result. I would ideally like to get
    /// rid of this field because it adds a whole byte to the size. This is
    /// necessary to prevent it from being dropped as part of a future.
    owns: bool,
    /// The stage of the [TaskControlBlock] state machine.
    _state: PhantomData<M>,
}

impl<M> TaskControlBlockVTable<M> {
    pub fn header(&self) -> &TaskControlHeader {
        unsafe { &*self.payload.cast::<TaskControlHeader>() }
    }
}

unsafe impl Send for TaskControlBlockVTable<Init> {}
unsafe impl Send for TaskControlBlockVTable<Ready> {}
unsafe impl Send for TaskControlBlockVTable<Panicked> {}

impl TaskControlBlockVTable<Ready> {
    /// This consumes the block's V-table and
    /// moves the result out of it's slot.
    ///
    /// SAFETY: The type `O` must be the same type as
    /// what the task control block was created for.
    pub unsafe fn get_result<O>(self) -> O {
        unsafe {
            let mut slot = MaybeUninit::uninit();
            (self.move_result)(
                self.payload,
                &mut slot as *mut _ as *mut MaybeUninit<()>,
                false,
            );
            let result = slot.assume_init();
            result
        }
    }
}

impl TaskControlBlockVTable<Panicked> {
    pub fn get_panic(self) -> Box<dyn Any + Send + 'static> {
        unsafe {
            let mut slot = MaybeUninit::uninit();
            (self.move_result)(
                self.payload,
                &mut slot as *mut _ as *mut MaybeUninit<()>,
                true,
            );
            let result = slot.assume_init();
            result
        }
    }
}

impl<M> Drop for TaskControlBlockVTable<M> {
    fn drop(&mut self) {
        if self.owns {
            (self.drop)(self.payload)
        }
    }
}

#[cfg(test)]
mod tests {
    use smol::LocalExecutor;

    use crate::core::task::{raw::TaskControlHeader, TaskControlBlock};

    #[test]
    pub fn task_control_block_basic() {
        let executor = LocalExecutor::new();

        smol::future::block_on(executor.run(async move {
            let task =
                TaskControlBlock::<_, (), _, usize>::create(TaskControlHeader::FireAndForget, |_| {
                    // println!("Future create...");
                    async move {
                        // println!("hello");
                        3
                    }
                });

            // println!("hi! SIZE={:?}", size_of_val(&task));

            let Ok(val) = unsafe { task.run::<()>(&mut ()) }.map_err(|_| ()).unwrap().await else {
                panic!("Future panicked unexpectedly.");
            };
            let reso = unsafe { val.get_result::<usize>() };
            // println!("result: {}", reso);
            assert_eq!(reso, 3);
        }));
    }

    #[test]
    pub fn task_control_closure_panic() {
        let executor = LocalExecutor::new();

        smol::future::block_on(executor.run(async move {
            let task =
                TaskControlBlock::<_, (), _, usize>::create(TaskControlHeader::FireAndForget, |_| {
                    panic!("I am a panic!");
                    #[allow(unreachable_code)]
                    async {
                        3
                    }
                });

            let mut null = ();

            // println!("hi! SIZE={:?}", size_of_val(&task));

            let val = unsafe { task.run(&mut null) };
            assert!(val.is_err());
            // let reso = unsafe { val.get_result::<usize>() };
            // println!("result: {}", reso);
            // assert_eq!(reso, 3);
        }));
    }

    #[test]
    pub fn task_control_future_panic() {
        let executor = LocalExecutor::new();

        smol::future::block_on(executor.run(async move {
            let task =
                TaskControlBlock::<_, (), _, usize>::create(TaskControlHeader::FireAndForget, |_| {
                    // panic!("I am a panic!");
                    async move {
                        panic!("I panicked :(");
                    }
                });

            // println!("hi! SIZE={:?}", size_of_val(&task));

            let mut null = ();

            // let val = task.run();
            unsafe {
                let Ok(task) = task.run(&mut null) else {
                panic!("Task errored prematurely.");
            };

            match task.await {
                Ok(v) => panic!("Task was supposed to fail."),
                Err(e) => {
                    let bo = e.get_panic();
                    match bo.downcast::<&str>() {
                        Ok(st) => {
                            assert_eq!(*st, "I panicked :(");
                        }
                        Err(e) => panic!("Could not downcast box."),
                    }
                }
            }
            }
            

            // let o = task.await;

            // assert!(val.is_err());
            // let reso = unsafe { val.get_result::<usize>() };
            // println!("result: {}", reso);
            // assert_eq!(reso, 3);
        }));
    }
}
