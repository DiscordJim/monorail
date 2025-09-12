pub mod raw;

pub type Task = Box<dyn FnOnce() + Send + 'static>;

use std::any::Any;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Poll;

pub(crate) use raw::TaskControlBlockVTable;
pub use raw::{TaskControlBlock, TaskControlHeader};
pub use raw::{Init, Pollable, Ready};

use crate::core::task::raw::build_tcb_vtable;

#[pin_project::pin_project]
pub struct CrossCoreTcb<M> {
    #[pin]
    table: TaskControlBlockVTable<M>
}

impl CrossCoreTcb<Init> {
    pub fn build<T, F, O>
    (
        header: TaskControlHeader,
        task: T
    ) -> CrossCoreTcb<Init>
    where 
        T: FnOnce() -> F + Send + 'static,
        F: Future<Output = O> + 'static,
        O: Send + 'static
    {
        CrossCoreTcb {
            table: unsafe {
                build_tcb_vtable::<_, (), _, _>(header, |_| task())
            }
        }
    }
    pub fn run(self) -> Result<CrossCoreTcb<Pollable<'static>>, Box<dyn Any + Send + 'static>> {
        match unsafe { self.table.run_without_args() } {
            Ok(future) => {
                Ok(CrossCoreTcb {
                    table: future
                })
            }
            Err(e) => {
                Err(e.get_panic())
            }
        }
    }
}

impl<M> CrossCoreTcb<M> {
    pub fn header(&self) -> &TaskControlHeader {
        self.table.header()
    }
}

impl Future for CrossCoreTcb<Pollable<'static>> {
    type Output = Result<CrossCoreTcb<Ready>, Box<dyn Any + Send + 'static>>;
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        let this = self.project();
        match this.table.poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => match result {
                Ok(v) => Poll::Ready(Ok(CrossCoreTcb {
                    table: v
                })),
                Err(e) => Poll::Ready(Err(e.get_panic()))
            }
        }
    }
}

impl CrossCoreTcb<Ready> {
    /// Turns the TCB into a result.
    /// 
    /// SAFETY: `O` must be the same type as creain.
    pub unsafe fn extract<O>(self) -> O {
        unsafe { self.table.get_result() }
    }
}

#[pin_project::pin_project]
pub struct LocalTcb<M>(#[pin] TaskControlBlockVTable<M>);

impl LocalTcb<Init> {
    pub fn build<'a, A, T, F, O>
    (
        header: TaskControlHeader,
        task: T
    ) -> CrossCoreTcb<Init>
    where 
        T: FnOnce(&mut A) -> F + Send + 'static,
        A: 'a,
        F: Future<Output = O> + 'a,
        O: 'static
    {
        CrossCoreTcb {
            table: unsafe {
                build_tcb_vtable::<_, _, _, _>(header, task)
            }
        }
    }
    pub unsafe fn run<'a, A>(self, args: &'a mut A) -> Result<LocalTcb<Pollable<'a>>, Box<dyn Any + Send + 'static>>
    where 
        A: 'a
    {
        match  unsafe { self.0.run(args) } {
            Ok(v) => Ok(LocalTcb(v)),
            Err(e) => Err(e.get_panic())
        }
    }
}

// impl<'a> Future for LocalTcb<Pollable<'a>> {
//     type Output = Result<(), Box<>;
//     fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
//         let this = self.project();
//         match this.0.poll(cx) {
//             Poll::Pending => Poll::Pending,
//             Poll::Ready(result) => match result {
//                 Err(e) => Poll::Ready(())
//             }
//         }
//     }
// }