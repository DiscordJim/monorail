use std::{
    any::Any,
    cell::RefCell,
    future::Future,
    marker::PhantomData,
    mem::ManuallyDrop,
    pin::Pin,
    ptr::null_mut,
    sync::{
        atomic::{AtomicPtr, Ordering},
        Arc,
    },
    task::{Poll, Waker},
};

use heapless::spsc::{Consumer, Producer, Queue};
use slab::Slab;

use crate::core::{
    channels::promise::{Promise, PromiseError, PromiseResolver},
    shard::{
        shard::{access_shard_ctx_ref, get_shard_route, shard_id, submit_to},
        state::{ShardId, ShardRoute},
    }, task::{CrossCoreTcb, Init, Ready, TaskControlBlock, TaskControlBlockVTable, TaskControlHeader},
};



// pub enum BridgedTask {
//     FireAndForget(TaskControlBlockVTable<TcbInit>),
//     FireWithTicket {
//         task: TaskControlBlockVTable<TcbInit>,
//         ticket_id: usize,
//         origin: ShardId,
//     },
// }

// struct Br/

pub type BridgeMessage = CrossCoreTcb<Init>;

pub struct Tx;
pub struct Rx;



pub struct BridgeProducer<M> {
    queue: Producer<'static, CrossCoreTcb<Init>>,
    waker: BridgeWakeCtx, // waker: Arc<AtomicPtr<Waker>>
    _marker: PhantomData<M>,
}

// pub struct BridgeMessage {
//     task: BridgedTask,
//     // origin: ShardId,
// }

pub struct BridgeConsumer<M> {
    queue: Consumer<'static, CrossCoreTcb<Init>>,
    waker: BridgeWakeCtx,
    _marker: PhantomData<M>,
}

pub(crate) struct BridgeConsumerFut<'a, M> {
    consumer: &'a mut BridgeConsumer<M>,
}

impl<'a, M> Future for BridgeConsumerFut<'a, M> {
    type Output = CrossCoreTcb<Init>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match self.consumer.queue.dequeue() {
            Some(a) => Poll::Ready(a),
            None => {
                if !self.consumer.waker.will_wake(cx.waker()) {
                    self.consumer.waker.set_waker(cx.waker());
                }
                Poll::Pending
            }
        }
    }
}

impl<M> BridgeConsumer<M> {
    pub fn recv(&mut self) -> BridgeConsumerFut<'_, M> {
        BridgeConsumerFut { consumer: self }
    }
}

impl<M> BridgeProducer<M> {
    pub fn send(&mut self, task: CrossCoreTcb<Init>) -> Result<(), CrossCoreTcb<Init>> {
        self.queue
            .enqueue(task)?;
        self.waker.wake_by_ref();
        Ok(())
    }
    // pub fn put_backlog(&mut self, task: BridgedTask) -> Result<()> {
    //     self
    // }
}

struct BridgeWakeCtx {
    waker: Arc<AtomicPtr<Waker>>,
}

impl BridgeWakeCtx {
    pub fn set_waker(&self, waker: &Waker) {
        let old = self.waker.swap(
            Arc::into_raw(Arc::new(waker.clone())).cast_mut(),
            Ordering::Release,
        );
        if !old.is_null() {
            unsafe { Arc::from_raw(old) };
        }
    }
    pub fn will_wake(&self, wkr: &Waker) -> bool {
        match self.load_optional_arc() {
            Some(a) => a.will_wake(wkr),
            None => false,
        }
    }
    #[inline]
    fn load_optional_arc(&self) -> Option<ManuallyDrop<Arc<Waker>>> {
        let ptr = self.waker.load(Ordering::Acquire);
        if ptr.is_null() {
            None
        } else {
            Some(ManuallyDrop::new(unsafe { Arc::from_raw(ptr) }))
        }
    }
    pub fn wake_by_ref(&self) {
        if let Some(wk) = self.load_optional_arc() {
            wk.wake_by_ref();
        }
    }
}

pub struct Bridge {
    send_queue: RefCell<BridgeProducer<Rx>>,
    recv_queue: RefCell<BridgeConsumer<Tx>>,
    back_log: Vec<CrossCoreTcb<Init>>,
    arena: RefCell<Slab<PromiseResolver<CrossCoreTcb<Ready>>>>,
}

// pub struct BridgeDeck {
    
// }

impl Bridge {
    pub fn create_queues<A, B>() -> (BridgeProducer<A>, BridgeConsumer<B>) {
        let queue = Box::leak(Box::new(Queue::<CrossCoreTcb<Init>, 128>::new()));
        // Producer::
        let (producer, consumer) = queue.split();

        // let consumer: Consumer<'static, TaskControlBlockVTable<TcbResult>> = unsafe { std::mem::transmute(consumer) };

        let wake = Arc::new(AtomicPtr::new(null_mut()));
        let (a, b) = (
            BridgeWakeCtx {
                waker: wake.clone(),
            },
            BridgeWakeCtx {
                waker: wake.clone(),
            },
        );

        (
            BridgeProducer {
                queue: producer,
                waker: a,
                _marker: PhantomData,
            },
            BridgeConsumer {
                queue: consumer,
                waker: b,
                _marker: PhantomData,
            },
        )
    }
    pub fn create(sender: BridgeProducer<Rx>, receiver: BridgeConsumer<Tx>) -> Self {
        Self {
            send_queue: sender.into(),
            recv_queue: receiver.into(),
            arena: Slab::new().into(),
            back_log: Vec::new(),
        }
    }
}


impl Bridge {
    pub fn fire_and_forget<F, FUT>(&self, task: F)
    where 
        F: FnOnce() -> FUT + Send + 'static,
        FUT: Future<Output = ()> + 'static
    {

        let block = CrossCoreTcb::build(TaskControlHeader::FireAndForget, task);


        let _ = self
            .send_queue
            .borrow_mut()
            .send(block);
    }
    pub async fn fire_with_ticket<T, F, O>(
        &self,
        task: T,
    ) -> Result<O, PromiseError>
    where 
        T: FnOnce() -> F + Send + 'static,
        F: Future<Output = O> + 'static,
        O: Send + 'static
    {

        let (rx, tx) = Promise::new();
        let ticket_id = self.arena.borrow_mut().insert(tx);
        let block = CrossCoreTcb::build(TaskControlHeader::WithReturn {
            origin: shard_id(),
            ticket: ticket_id
        }, task);
        
        let _ = self
            .send_queue
            .borrow_mut()
            .send(block);

        let payload = unsafe { rx.await?.extract::<O>() };

        Ok(payload)
    }
    pub async fn run_bridge(&self) {
        let mut rcv = self.recv_queue.borrow_mut();
        loop {
            let r = rcv.recv().await;
            // println!("I am {:?} receiving a control block.", shard_id());
            match *r.header() {
                TaskControlHeader::FireAndForget => {
                    // println!("Rcv FF task");
                    // let t = r.run().await;
                    // println!("Terminated FF task");
                    access_shard_ctx_ref().executor.spawn(r.run().map_err(|e| ()).unwrap()).detach();
                }
                TaskControlHeader::WithReturn {
                    // task,
                    // ticket_id,
                    ticket: ticket_id,
                    origin: source,
                } => {
                    let current = shard_id();
                    access_shard_ctx_ref()
                        .executor
                        .spawn(async move {
                            // let task = access_shard_ctx_ref().executor.spawn(async move {
                                let mut guard = BridgeDropCancelGuard {
                                    origin: source,
                                    current,
                                    slot: ticket_id,
                                    resolved: false
                                };
                                let reso = r.run().map_err(|_| ()).unwrap().await;
                                // let reso = unsafe { reso.get_result::<O>() };

                                // let ticket = *ticket_id;

                               
                                // println!("Done (B)");
                                // if let Some(bridge) = get_shard_route(current) {
                                //     submit_to(source, async move || {});
                                // }
                                submit_to(source, async move || {
                                    // if let ShardRoute::Bridge(bridge) = 
                                    if let Some(ShardRoute::Bridge(bridge)) = get_shard_route(current) {
                                        match reso {
                                        Err(e) => {
                                            let _ = bridge.arena.borrow_mut().remove(ticket_id).reject_panic(e);
                                        }
                                        Ok(v) => {
                                            let _ = bridge.arena.borrow_mut().remove(ticket_id).resolve(v);
                                        }
                                    }
                                    }
                                    
                                 

                                    // let _ =
                                    //     bridge.arena.borrow_mut().remove(ticket_id).resolve(reso);
                                });

                                 guard.resolved = true;
              
                            // });

                          

                            // task.detach();
                        })
                        .detach();
                }
            }
        }
    }
    // pub async fn tick()
}

struct BridgeDropCancelGuard {
    origin: ShardId,
    current: ShardId,
    slot: usize,
    resolved: bool
}

impl Drop for BridgeDropCancelGuard {
    fn drop(&mut self) {
        if self.resolved {
            return;
        }
        let origin = self.origin;
        let current = self.current;
        let slot = self.slot;
        submit_to(origin, async move || {
            if let Some(ShardRoute::Bridge(bridge)) = get_shard_route(current) {
                let _ = bridge.arena.borrow_mut().remove(slot);
            }
            // let bridge = get_remote_brige(current);
            
        });
    }
}

impl BridgeDropCancelGuard {}



#[cfg(test)]
mod tests {
    // use futures::channel::oneshot;

    use crate::core::{
        // channels::bridge::Bridge,
        shard::{
            shard::{call_on, shard_id, signal_monorail, spawn_async_task},
            state::{ShardId},
        },
        topology::{MonorailConfiguration, MonorailTopology},
    };

    #[test]
    pub fn test_bridge_demultiplex() {
        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(2)
                .build(),
            async || {
                spawn_async_task(async {
                    let ru = call_on(ShardId::new(1), async move || shard_id())
                        .await
                        .unwrap();
                    // println!("Hello...");

                    assert_eq!(ru.as_usize(), 1);
                    signal_monorail(Ok(()));
                    // println!("RU: {:?}", ru);
                })
                .detach();
            },
        )
        .unwrap();
    }
}

// pub struct BridgeResolver {

// }

// fn test() {

//     let q: (Producer<'_, _>, heapless::spsc::Consumer<'_, _>) = Queue::<Task, 32>::new().split_const();

//     let b = Bridge {
//         send_queue: q.0,
//         recv_queue: q.1
//     };
// }
