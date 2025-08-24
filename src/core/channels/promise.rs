use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
    thread::{current, park, Thread},
};

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

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum PromiseError {
    #[error("The promise closed on the other end.")]
    PromiseClosed,
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
    use std::{
        sync::Arc,
        task::{Context, Wake, Waker},
    };

    use crate::core::channels::promise::{PromiseError, SyncPromise};

    struct TestWaker {}

    impl Wake for TestWaker {
        fn wake(self: Arc<Self>) {}
    }

    #[test]
    pub fn test_sync_promise() {
        let (promise, resolver) = SyncPromise::<u8>::new();
        resolver.resolve(3);
        assert_eq!(promise.wait(), Ok(3));
    }

    #[test]
    pub fn test_sync_promise_dropped() {
        let (promise, resolver) = SyncPromise::<u8>::new();
        drop(resolver);
        assert_eq!(promise.wait(), Err(PromiseError::PromiseClosed));
    }
}
