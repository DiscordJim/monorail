use std::{
    any::Any, cell::RefCell, future::Future, marker::PhantomData, mem::ManuallyDrop, pin::Pin, ptr::null_mut, sync::{
        atomic::{AtomicPtr, Ordering},
        Arc,
    }, task::{Poll, Waker}
};

use heapless::spsc::{Consumer, Producer, Queue};
use slab::Slab;

use crate::core::{
    channels::promise::{Promise, PromiseResolver},
    shard::{shard::{access_shard_ctx_ref, shard_id}, state::ShardRuntime},
    task::Task,
};

pub type AsyncTask =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + 'static>> + Send + 'static>;

pub type AsyncTaskWithResult =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = Box<dyn Any + Send + 'static>> + 'static>> + Send + 'static>;


pub enum BridgedTask {
    FireAndForget(AsyncTask),
    FireWithTicket {
        task: AsyncTaskWithResult,
        ticket_id: usize
    }
}

// struct Br/

pub struct Tx;
pub struct Rx;

pub struct BridgeProducer<M> {
    queue: Producer<'static, BridgedTask>,
    waker: BridgeWakeCtx, // waker: Arc<AtomicPtr<Waker>>
    _marker: PhantomData<M>
}

pub struct BridgeConsumer<M> {
    queue: Consumer<'static, BridgedTask>,
    waker: BridgeWakeCtx,
    _marker: PhantomData<M>
}

pub struct BridgeConsumerFut<'a, M> {
    consumer: &'a mut BridgeConsumer<M>
}

impl<'a, M> Future for BridgeConsumerFut<'a, M> {
    type Output = BridgedTask;
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
    pub fn send(&mut self, task: BridgedTask) -> Result<(), BridgedTask> {
        self.queue.enqueue(task)?;
        self.waker.wake_by_ref();
        Ok(())
    }
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
    back_log: Vec<BridgedTask>,
    arena: RefCell<Slab<PromiseResolver<Box<dyn Any + Send>>>>,
}

impl Bridge {
    pub fn create_queues<A, B>() -> (BridgeProducer<A>, BridgeConsumer<B>) {
        let queue = Box::leak(Box::new(Queue::<BridgedTask, 128>::new()));
        let (producer, consumer) = queue.split();

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
                _marker: PhantomData
            },
            BridgeConsumer {
                queue: consumer,
                waker: b,
                _marker: PhantomData
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
    pub fn fire_and_forget(&self, task: AsyncTask) {
        let _ = self.send_queue.borrow_mut().send(BridgedTask::FireAndForget(task));
    }
    // pub async fn fire_with_ticket() {
    //     let (rx, tx) = Promise::<BridgedTask>::new();

    // }
    pub async fn run_bridge(&self) {
        let mut rcv = self.recv_queue.borrow_mut();
        loop {
            let r = rcv.recv().await;
            match r {
                BridgedTask::FireAndForget(task) => {
                    // let fut = std::pin::pin!(task(runtime));
                    // task().await;
                    access_shard_ctx_ref().executor.spawn(task()).detach();
                }
                BridgedTask::FireWithTicket { task, ticket_id } => {
                    let (rx, tx) = Promise::new();
                    let ticket = self.arena.borrow_mut().insert(tx);
                    let origin = shard_id();
                    access_shard_ctx_ref().executor.spawn(async move {

                        let reso = task().await;

                        
                        


                    }).detach();
                }
                 // BridgedTask::FireWithTicket { task, ticket_id } => {
                  //     task(runtime);
                  // }
            }
        }
    }
    // pub async fn tick()
}

pub(crate) fn box_job<F, FUT>(task: F) -> AsyncTask
where
    F: FnOnce() -> FUT + Send + 'static,
    FUT: Future<Output = ()> + 'static
{

    Box::new(move || Box::pin(task()))

}

// #[cfg(test)]
// mod tests {
//     use futures::channel::oneshot;

//     use crate::core::{
//         channels::bridge::Bridge,
//         shard::{
//             shard::{spawn_async_task, submit_task_to},
//             state::{ShardId, ShardRuntime},
//         },
//         topology::{MonorailConfiguration, MonorailTopology},
//     };

//     #[test]
//     pub fn test_bridge_demultiplex() {
//         MonorailTopology::setup(
//             MonorailConfiguration::builder()
//                 .with_core_override(2)
//                 .build(),
//             |init| {
//                 spawn_async_task(async {
//                     let (a, b) = Bridge::create_queues();

//                     let (tx, rx) = oneshot::channel();
//                     submit_task_to(ShardId::new(1), async move || {
//                         let (c, d) = Bridge::create_queues();
//                         let _ = tx.send(d);

//                         let mut br2 = Bridge::create(c, b);
//                         br2.run_bridge()
//                             .await;
//                     });
//                     let d = rx.await.unwrap();

//                     let mut br = Bridge::create(a, d);

//                     // br.fire_and_forget(Box::new(async move |runtime| {
//                     //     println!("hello");
//                     // }));
//                 })
//                 .detach();
//             },
//         )
//         .unwrap();
//     }
// }

// // pub struct BridgeResolver {

// // }

// // fn test() {

// //     let q: (Producer<'_, _>, heapless::spsc::Consumer<'_, _>) = Queue::<Task, 32>::new().split_const();

// //     let b = Bridge {
// //         send_queue: q.0,
// //         recv_queue: q.1
// //     };
// // }
