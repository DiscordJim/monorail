use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    marker::PhantomData,
    os::fd::AsRawFd,
    sync::atomic::{AtomicPtr, Ordering},
    thread::{current, ThreadId},
    time::Duration,
};

use crate::core::{
    actor::base::{Actor, ActorSignal, LocalAddr}, executor::{backoff::{AdaptiveBackoff, BackoffResult}}, io::{fs::OpenOptions, ring::{install_polladd_multi, install_timeout, timeout, Claim, IoRingDriver}, FromRing}, shard::state::ShardId
};
use async_task::{Builder, Runnable};
use io_uring::{squeue::PushError, types::Timespec};
use lfqueue::UnboundedQueue;
use nix::sys::eventfd::{EfdFlags, EventFd};
use smol::{future::{self, yield_now}, Task};
use std::os::fd::AsFd;

pub struct Executor<'a> {
    state: ExecutorState,
    _marker: PhantomData<std::cell::UnsafeCell<&'a ()>>,
}

impl<'a> Executor<'a> {
    pub fn leak(self) -> &'static Executor<'static> {
        Box::leak(Box::new(Executor {
            state: self.state,
            _marker: PhantomData
        }))
    }
}

struct ExecutorState {
    queue: RefCell<VecDeque<Runnable>>,
    // fast_queue: Bo
    mt_queue: UnboundedQueue<Runnable>,
    // notify: LocalEvent,
    long_wakeup: *mut EventFd,
    foreign_wakeup: AtomicPtr<EventFd>,
    ring: IoRingDriver,
    // token: GhostToken<'static>,
    origin: ThreadId, // notify: RefCell<Option<Waker>>,
}

impl<'a> Drop for ExecutorState {
    fn drop(&mut self) {
        unsafe {
            let _ = Box::from_raw(self.long_wakeup);
        }
    }
}

impl<'a> Executor<'a> {
    pub fn new(core: ShardId) -> Self {
        let event_fd = Box::into_raw(Box::new(
            EventFd::from_value_and_flags(0, EfdFlags::EFD_NONBLOCK | EfdFlags::EFD_CLOEXEC)
                .unwrap(),
        ));

        let obj = Self {
            state: ExecutorState {
                // office: ShardActorOffice::new().into(),
                queue: RefCell::default(),
                mt_queue: UnboundedQueue::new(),
                // token: GhostToken::new(|token| token),
                ring: IoRingDriver::new(512).unwrap(),
                origin: current().id(),
                long_wakeup: event_fd,
                foreign_wakeup: AtomicPtr::new(std::ptr::null_mut()), // is_awake: Cell::default(),
                                                                      // notify: RefCell::default()
            },
            _marker: PhantomData,
        };

        obj
    }
    pub async fn sleep(&'a self, duration: Duration) {
        timeout(&self.state.ring, duration).await.unwrap();
    }
    pub fn io_uring(&'a self) -> &'a IoRingDriver {
        &self.state.ring
    }
    

    pub(crate) fn spawn<T: 'a>(&'a self, future: impl Future<Output = T> + 'a) -> Task<T> {
        unsafe {
            let origin = self.state.origin;
            let (runnable, task) = Builder::new()
                // .
                .propagate_panic(true)
                // .
                .spawn_unchecked(
                    |()| future,
                    move |runnable| {
                        // println!("Pushing task...");

                        if current().id() == origin {
                            // println!("hehe 2");
                            self.state.push_task(runnable);
                        } else {
                            // println!("Landing in the MT quuee...");
                            let ext = self.state.foreign_wakeup.load(Ordering::Acquire);
                            if !ext.is_null() {
                                // unsafe {
                                (*ext).write(0).unwrap();
                                // }
                            }
                            self.state.mt_queue.enqueue(runnable);

                            // self.state.long_wakeup.write(0).unwrap();
                        }
                        // self.state.push_task(runnable);
                        // self.state.notify();
                    },
                );
            runnable.schedule();
            task
        }
    }
    pub fn open_options(&'a self) -> OpenOptions<'a> {
        OpenOptions::from_ring(&self.state.ring)
    }
    pub async fn run<T: 'a>(&'a self, fut: impl Future<Output = T> + 'a) -> T {
        // const RESPONSE: Duration = Duration::from_millis(1);
        let mut runner = Runner {
            state: &self.state,
            backoff: AdaptiveBackoff::new(),
            previous_timeout: None
        };

        future::or(self.spawn(fut),  runner.run()).await
    }
    pub fn block_on<T: 'a>(&'a self, fut: impl Future<Output = T> + 'a) -> T {
   
        
        smol::block_on(self.run(fut))
    }
}

impl ExecutorState {
    pub fn pop_task(&self) -> Option<Runnable> {
        self.queue.borrow_mut().pop_front()
    }
    pub fn push_task(&self, runner: Runnable) {
        self.queue.borrow_mut().push_back(runner);
    }
    pub fn len(&self) -> usize {
        self.queue.borrow().len()
    }
}

pub(crate) struct Runner<'a> {
    /// The underlying executor this runner is managing.
    state: &'a ExecutorState,
    /// The scheduler backoff function.
    backoff: AdaptiveBackoff,
    /// The previous timeout.
    previous_timeout: Option<Claim<'a, Timespec>>,
}

impl<'a> Runner<'a> {
    #[inline]
    pub fn setup_timeout(&mut self) -> Result<(), PushError> {
        // Flag that determines if we shuld set the imeou.
        let mut should_set = false;

        match self.previous_timeout.take() {
            None => should_set = true,
            Some(chk) => match chk.check() {
                Ok(_) => should_set = true,
                Err(e) => self.previous_timeout = Some(e),
            },
        }

        if should_set {
            match self.backoff.backoff() {
                BackoffResult::Park => {
                    // println!("Parking!");
                    self.state.foreign_wakeup.store(self.state.long_wakeup, Ordering::Release);
                }
                BackoffResult::Timeout(duration) => {
                    self.previous_timeout = Some(install_timeout(&self.state.ring, duration)?);
                }
            }
        }
        Ok(())
    }
    #[inline]
    fn schedule_runnable(&mut self, runnable: Runnable) {
        self.backoff.reset();
        // println!("Starting run...");
        runnable.run();
        // println!("Finsihed run...");
    }
    // #[inline]
    // fn 
    pub async fn run<T>(&mut self) -> T {
        let _event_fd_poller = install_polladd_multi(&self.state.ring, unsafe {
            (*self.state.long_wakeup).as_fd().as_raw_fd()
        })
        .unwrap();
        self.state.ring.submit();


        loop {
            self.state.ring.drive();

            // println!("Hello...");

            if self.state.len() == 0 {
                // The state queue is empty, we can safely wait
                // for stuff to wake up.
                self.setup_timeout().unwrap();
                self.state.ring.sub_and_wait().unwrap();
            }

            for _ in 0..6 {
                let runnable = self.state.pop_task();
            
                match runnable {
                    Some(runner) => self.schedule_runnable(runner),
                    None => break
                }
            }

            for _ in 0..3 {
                let runnable = self.state.mt_queue.dequeue();
                match runnable {
                    Some(runner) => self.schedule_runnable(runner),
                    None => break
                }
            }
            // println!("Yi");
            yield_now().await;
        }
    }
}



#[cfg(test)]
mod tests {
    use std::{future::Future, task::{Poll, Waker}};

    use flume::Sender;
    use smol::future::{self};

    use crate::core::{executor::scheduler::Executor, shard::state::ShardId};


    #[test]

    pub fn test_separate_waker() {

        let (tx, rx) = flume::unbounded();

        struct Fut {
            name: usize,
            source: Sender<(usize, Waker)>
        }

        impl Future for Fut {
            type Output = ();
            fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
                
                // println!("Polling: {}", self.name);
                self.source.send((self.name, cx.waker().clone())).unwrap();

                Poll::Pending
            }
        }

        let a = Fut {
            name: 0,
            source: tx.clone()
        };

        let b = Fut {
            name: 1,
            source: tx.clone()
        };

        let executor = Executor::new(ShardId::new(0)).leak();
        executor.block_on(async move {

            executor.spawn(a).detach();
            executor.spawn(b).detach();
            let mut reso = vec![];
            for _ in 0..2 {
                reso.push(rx.recv_async().await.unwrap());
            }

            reso.sort_by(|(a, _), (b, _)| a.cmp(b));

            assert!(!reso[0].1.will_wake(&reso[1].1));

            // println!("RESO: {:?}", reso[0].1.will_wake(&reso[1].1));

            // assert!(smol::future::poll_once(a).awai)
            

            Ok::<_, anyhow::Error>(())

        }).unwrap();

    }

    #[test]
    pub fn test_basic_scheduler() {
        let executor = Executor::new(ShardId::new(0));
        let y = future::block_on(executor.run(async {
            1
        }));
        assert_eq!(y, 1);
    }
}