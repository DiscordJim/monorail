use std::{
    any::Any, cell::{Cell, UnsafeCell}, future::Future, mem::MaybeUninit, rc::Rc, sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    }, task::{Poll, Waker}, thread::{current, park, Thread}
};


pub struct Promise<T> {
    internal: Rc<PromiseInternal<T>>
}

struct PromiseInternal<T> {
    is_ready: Cell<PromiseState>,
    waker: UnsafeCell<MaybeUninit<Waker>>,
    value: UnsafeCell<MaybeUninit<PromiseSlot<T>>>,
    
}

impl PartialEq for PromiseError {
    fn eq(&self, other: &Self) -> bool {
        match self {
            PromiseError::PromiseClosed => matches!(other, PromiseError::PromiseClosed),
            PromiseError::Paniced(e) => match other {
                PromiseError::PromiseClosed => false,
                PromiseError::Paniced(f) => true
            }
        }
    }
}


enum PromiseSlot<T> {
    Value(T),
    Error(Box<dyn Any + Send + 'static>)
}

// impl PartialEq for Pro

pub struct PromiseResolver<T> {
    internal: Rc<PromiseInternal<T>>
}

impl<T> Promise<T> {
    pub fn new() -> (Promise<T>, PromiseResolver<T>) {
        let internal = Rc::new(PromiseInternal {
            is_ready: Cell::new(PromiseState::Idle),
            value: UnsafeCell::new(MaybeUninit::uninit()),
            waker: UnsafeCell::new(MaybeUninit::uninit())
        });

        // let (a, b) = static_rc::StaticRc::split::<1, 1>(internal);
        

        (Promise {
            internal: internal.clone()
        }, PromiseResolver {
            internal
        })
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromiseState {
    Idle,
    Wait,
    Ready,
    Closed,
    // Panicked
}


impl<T> PromiseResolver<T> {
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.internal.is_ready.get() == PromiseState::Closed
    }
    pub fn reject_panic(self, panic_info: Box<dyn Any + Send + 'static>) {
        let _  = self.resolve_internal(PromiseSlot::Error(panic_info));
    }
    pub fn resolve(self, value: T) -> Result<(), (T, PromiseError)> {

        match self.resolve_internal(PromiseSlot::Value(value)) {
            Ok(prom ) => Ok(()),
            Err((slot, error)) => match slot {
                PromiseSlot::Error(e) => unreachable!(),
                PromiseSlot::Value(v) => Err((v, error))
            }
        }

    }
    fn resolve_internal(self, value: PromiseSlot<T>) -> Result<(), (PromiseSlot<T>, PromiseError)> {
        println!("Resolving a promise...");
        match self.internal.is_ready.get() {
            PromiseState::Closed => {
                Err((value, PromiseError::PromiseClosed))
            }
            PromiseState::Idle => {
                // In this case there is no waker present yet.
                unsafe { (&mut *self.internal.value.get()).write(value) };
                // println!("Set the ")
                self.internal.is_ready.set(PromiseState::Ready);
                Ok(())

            }
            PromiseState::Ready => {
                Err((value, PromiseError::PromiseClosed))
            }
            PromiseState::Wait => {
                unsafe {
                    (&mut *self.internal.value.get()).write(value);
                    self.internal.is_ready.set(PromiseState::Ready);
                    (&*self.internal.waker.get()).assume_init_ref().wake_by_ref();
                    Ok(())
                 }
            }

        }
    }
}

impl<T> Drop for PromiseResolver<T> {
    fn drop(&mut self) {
        // println!("ropping a promise resolver... {:?}", self.internal.is_ready.get());
        match self.internal.is_ready.get() {
            PromiseState::Closed => {
                // Do nothing.
            }
            PromiseState::Idle => {
                self.internal.is_ready.set(PromiseState::Closed);
                // unsafe { (&*self.internal.waker.get()).assume_init_ref().wake_by_ref() };
            }
            PromiseState::Wait => {
                self.internal.is_ready.set(PromiseState::Closed);
                unsafe { (&*self.internal.waker.get()).assume_init_ref().wake_by_ref() };
            }
            PromiseState::Ready => {
                // Do nothing.
            }
        }
    }
}

impl<T> Drop for Promise<T> {
    fn drop(&mut self) {
        match self.internal.is_ready.get() {
            PromiseState::Closed | PromiseState::Ready => {
                // Nothing.
            }
            PromiseState::Idle | PromiseState::Wait => {
                self.internal.is_ready.set(PromiseState::Closed);
            }
        }
    }
}

// pub enum Pro

impl<T> Future for Promise<T> {
    type Output = Result<T, PromiseError>;
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        match self.internal.is_ready.get() {
            PromiseState::Idle => {

                unsafe {
                    (&mut *self.internal.waker.get()).write(cx.waker().clone());
                }
                self.internal.is_ready.set(PromiseState::Wait);
                Poll::Pending
            }
            PromiseState::Closed => {
                Poll::Ready(Err(PromiseError::PromiseClosed))

            }
            PromiseState::Ready => {
                self.internal.is_ready.set(PromiseState::Closed);

                let value = unsafe { (&mut *self.internal.value.get()).assume_init_read() };

                match value {
                    PromiseSlot::Error(e) => Poll::Ready(Err(PromiseError::Paniced(e))),
                    PromiseSlot::Value(v) => Poll::Ready(Ok(v))
                }

                // Poll::Ready(Ok(unsafe {  }))
                // Poll::Ready(Err())
            }
            PromiseState::Wait => {
                Poll::Pending
            }
        }

    }
}

pub struct SyncPromise<T>(Arc<SyncPromiseInternal<T>>);

const WAIT: u8 = 0;
const READY: u8 = 1;
const CLOSED: u8 = 2;

pub struct SyncPromiseResolver<T> {
    origin: Thread,
    internal: Arc<SyncPromiseInternal<T>>,
}


struct SyncPromiseInternal<T> {
    is_ready: AtomicU8,
    location: UnsafeCell<MaybeUninit<T>>,
}

impl<T> SyncPromise<T> {
    pub fn new() -> (SyncPromise<T>, SyncPromiseResolver<T>) {
        let internal = Arc::new(SyncPromiseInternal {
            is_ready: AtomicU8::new(WAIT),
            location: UnsafeCell::new(MaybeUninit::uninit()),
        });

        (
            SyncPromise(internal.clone()),
            SyncPromiseResolver {
                origin: current(),
                internal,
            },
        )
    }
    pub fn wait(self) -> Result<T, PromiseError> {
        let mut loaded = self.0.is_ready.load(Ordering::Acquire);
        while loaded == WAIT {
            park();

            loaded = self.0.is_ready.load(Ordering::Acquire);
        }
        if loaded == READY {
            return Ok(unsafe { (&*self.0.location.get()).assume_init_read() });
        } else {
            return Err(PromiseError::PromiseClosed);
        }
    }
}

impl<T> SyncPromiseResolver<T> {
    pub fn resolve(self, item: T) {
        unsafe { (&mut *self.internal.location.get()).write(item) };
        self.internal.is_ready.store(READY, Ordering::Release);
        self.origin.unpark();
    }
    pub fn forget(self) {
        self.internal.is_ready.store(CLOSED, Ordering::Release);   
    }
}

impl<T> Drop for SyncPromiseResolver<T> {
    fn drop(&mut self) {
        if self
            .internal
            .is_ready
            .compare_exchange(WAIT, CLOSED, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            self.origin.unpark();
        }
        // self.internal.is_ready.store(CLOSED, Ordering::Release);
    }
}

unsafe impl<T: Send> Send for SyncPromiseResolver<T> {}

#[derive(thiserror::Error, Debug)]
pub enum PromiseError {
    #[error("The promise closed on the other end.")]
    PromiseClosed,
    #[error("The promise has panicked.")]
    Paniced(Box<dyn Any + Send + 'static>)
}

// impl<T> SyncPromise<T> {
//     pub fn new() -> (SyncPromise<T>, )
// }

// fn par() {

//     std::thread::current().

//     let wow = park();
// }

#[cfg(test)]
mod tests {
    use std::{future::Future, task::{Context, Poll, Waker}};

    use crate::core::channels::promise::{Promise, PromiseError, SyncPromise};



    #[test]
    pub fn test_sync_promise() {
        let (promise, resolver) = SyncPromise::<u8>::new();
        resolver.resolve(3);
        assert_eq!(promise.wait().unwrap(), 3);
    }

    #[test]
    pub fn test_sync_promise_dropped() {
        let (promise, resolver) = SyncPromise::<u8>::new();
        drop(resolver);
        assert_eq!(promise.wait(), Err(PromiseError::PromiseClosed));
    }



    #[test]
    pub fn test_async_promise_normal() {

        let (promise, resolver) = Promise::<usize>::new();
        
        let mut ctx = Context::from_waker(Waker::noop());

        let mut waiting = std::pin::pin!(promise);
        assert!(waiting.as_mut().poll(&mut ctx).is_pending());

        assert!(resolver.resolve(3).is_ok());

        assert_eq!(waiting.as_mut().poll(&mut ctx), Poll::Ready(Ok(3)));


    }

    #[test]
    pub fn test_async_promise_preload() {

        let (promise, resolver) = Promise::<usize>::new();
        
        let mut ctx = Context::from_waker(Waker::noop());

        let mut waiting = std::pin::pin!(promise);
        assert!(resolver.resolve(3).is_ok());
        assert_eq!(waiting.as_mut().poll(&mut ctx), Poll::Ready(Ok(3)));


     
        

    }

     #[test]
    pub fn test_async_promise_close_sender() {

        let (promise, resolver) = Promise::<usize>::new();
        drop(promise);
        // drop(promise);
        
        assert!(resolver.resolve(3).is_err());
    }

     #[test]
    pub fn test_async_promise_close_receiver_before_poll() {

        let (promise, resolver) = Promise::<usize>::new();
        
        let mut ctx = Context::from_waker(Waker::noop());

        let mut waiting = std::pin::pin!(promise);
        
        drop(resolver);
        // drop(promise);
        
        // assert!(resolver.resolve(3).is_err());
        assert_eq!(waiting.as_mut().poll(&mut ctx), Poll::Ready(Err(PromiseError::PromiseClosed)));


     
        

    }

     #[test]
    pub fn test_async_promise_close_receiver() {

        let (promise, resolver) = Promise::<usize>::new();
        
        let mut ctx = Context::from_waker(Waker::noop());

        let mut waiting = std::pin::pin!(promise);

        let _ = waiting.as_mut().poll(&mut ctx);
        
        drop(resolver);
        // drop(promise);
        
        // assert!(resolver.resolve(3).is_err());
        assert_eq!(waiting.as_mut().poll(&mut ctx), Poll::Ready(Err(PromiseError::PromiseClosed)));


     
        

    }

}
