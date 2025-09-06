use std::{
    collections::VecDeque, future::Future, marker::PhantomData, ops::{Add, Deref}, pin::Pin, ptr::NonNull, rc::Rc
};

use anyhow::{anyhow, Result};
use asyncnal::{EventSetter, LocalEvent};
use flume::{Receiver, Sender};
use futures::channel::oneshot;
use smol::{future::race, Task};

use crate::core::{actor::futures::BoxedCall, channels::promise::Promise, executor::{
    helper::{select2, Select2Result}, mail::MailId, scheduler::Executor
}, shard::{shard::{self, get_actor_addr, shard_id, signal_actor_mailbox, spawn_async_task, submit_to}, state::{ShardId, ShardRuntime}}};

pub trait ActorCall<M>: Actor {
    type Output;

    fn call<'a>(
        this: SelfAddr<'a, Self>,
        msg: M,
        state: &'a mut Self::State,
    ) -> impl Future<Output = Self::Output>;
}

struct SubTaskHandle {
    task: Task<()>,
    handle: SignalHandle,
}



struct InternalActorRunCtx {
    executor: &'static Executor<'static>,
    tasks: Vec<SubTaskHandle>,
    foreigns: Vec<FornSignalHandle>
}

pub struct SelfAddr<'a, A>
where
    A: Actor,
{
    addr: &'a Addr<A>,
    // executor: &'a Executor<'a>,
    internal: &'a mut InternalActorRunCtx,
}

impl<'a, A> SelfAddr<'a, A>
where
    A: Actor,
{
    pub async fn spawn_linked_foreign<B>(&mut self, core: ShardId, arguments: B::Arguments) -> Result<FornAddr<B>>
    where 
        B: Actor,
        B::Message: Send + 'static,
        B::Arguments: Send + 'static
    {

        let (tx, rx) = oneshot::channel();
        submit_to(core, |_| {

            // println!("Starttin g on {:?}", shard_id());

            match shard::spawn_actor(arguments) {
                Ok((actor, _)) => {
                    let _ = tx.send(Ok(actor));
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
            // let (foreign, local) = shard::spawn_actor(arguments);
        });
        
        let handler = rx.await??;

        self.internal.foreigns.push(handler.clone().signal);


        Ok(handler)

    }
    pub async fn spawn_linked<B>(&mut self, arguments: B::Arguments) -> Result<()>
    where
        B: Actor,
    {
        let (actor, handle) = spawn_actor::<B>(self.internal.executor, arguments)?;
        self.internal.tasks.push(SubTaskHandle {
            handle: actor.raw.signal_handle,
            task: handle,
        });

        Ok(())
    }
    pub async fn external_event<F, FUT>(&mut self, func: F) -> Result<()>
    where
        F: FnOnce(Addr<A>) -> FUT,
        FUT: Future<Output = ()> + 'static,
    {
        let fut = func(self.addr.clone());

        let (s_tx, s_rx) = flume::unbounded();

        let signal = SignalHandle {
            kill_signal: Rc::new(LocalEvent::new()),
            signal_channel: s_tx,
        };

        let ks = signal.kill_signal.clone();
        let loopa = self.internal.executor.spawn(async move {
            // println!("Starting future...");
            let _ = race(fut, async {
                let _ = select2(s_rx.recv_async(), ks.wait()).await;
            })
            .await;
            // fut.await;

            // race(fut, async { })
        });

        self.internal.tasks.push(SubTaskHandle {
            handle: signal,
            task: loopa,
        });
        Ok(())
    }
}

// impl<'a,

impl<'a, A> Deref for SelfAddr<'a, A>
where
    A: Actor,
{
    type Target = Addr<A>;
    fn deref(&self) -> &Self::Target {
        self.addr
    }
}

pub enum ActorSignal {
    Stop,
    Kill,
}

#[derive(Clone)]
pub(crate) struct SignalHandle {
    signal_channel: Sender<ActorSignal>,
    kill_signal: Rc<LocalEvent>,
}

impl SignalHandle {
    pub fn stop(&self) -> Result<()> {
        self.signal_channel.send(ActorSignal::Stop).map_err(|_| anyhow::anyhow!("Failed to send signal."))?;
        Ok(())
    }
    pub fn kill(&self) {
        self.kill_signal.set_one();
    }
}

// #[derive(Clone)]
struct RawHandle<A>
where
    A: Actor,
{
    signal_handle: SignalHandle,
    msg_channel: Sender<NormalActorMessage<A>>,
}

impl<A> Clone for RawHandle<A>
where
    A: Actor,
{
    fn clone(&self) -> Self {
        Self {
            signal_handle: self.signal_handle.clone(),
            msg_channel: self.msg_channel.clone(),
        }
    }
}

async fn multiplex_actor_channels<A>(
    msg: &Receiver<NormalActorMessage<A>>,
    signal: &Receiver<ActorSignal>,
) -> Result<ActorMessage<A>, flume::RecvError>
where
    A: Actor,
{
    let multi = select2(msg.recv_async(), signal.recv_async()).await;
    match multi {
        Select2Result::Left(a) => match a? {
            NormalActorMessage::Call(call) => Ok(ActorMessage::Call(call)),
            NormalActorMessage::Message(me) => Ok(ActorMessage::Message(me))
        },
        Select2Result::Right(b) => Ok(ActorMessage::Signal(b?)),
    }
}


#[allow(unused_variables)]
/// Actors are the basic unit of concurrency in monorail. Please note that
/// we are specifically referring to concurrency and not parallelization, as
/// actors may never themselves cross a thread boundary.
pub trait Actor: Sized + 'static {
    type Arguments;
    type Message;
    type State;
    /// The actor must define a unique name. This is for trace monitoring.
    fn name() -> &'static str;

    fn pre_start(arguments: Self::Arguments) -> Result<Self::State>;

    fn post_start(
        this: SelfAddr<'_, Self>,
        state: &mut Self::State,
    ) -> impl Future<Output = Result<()>> {
        async { Ok(()) }
    }
    fn post_stop(
        this: SelfAddr<'_, Self>,
        state: &mut Self::State,
    ) -> impl Future<Output = Result<()>> {
        async { Ok(()) }
    }
    fn handle(
        this: SelfAddr<'_, Self>,
        message: Self::Message,
        state: &mut Self::State,
    ) -> impl Future<Output = Result<()>> {
        async { Ok(()) }
    }
}



pub struct Addr<T>
where
    T: Actor,
{
    raw: RawHandle<T>,
    _marker: PhantomData<T>,
}

impl<T> Addr<T>
where
    T: Actor,
{
    pub(crate) fn downgrade(self) -> SignalHandle {
        self.raw.signal_handle
    }
    pub async fn send(&self, message: T::Message) -> Result<(), T::Message> {
        if let Err(e) = self.raw.msg_channel.send_async(NormalActorMessage::Message(message)).await {
            // retu
            if let NormalActorMessage::Message(m) = e.into_inner() {
                return Err(m);
            }
        }
        Ok(())
    }
    pub async fn stop(&self) -> Result<()> {
        if self
            .raw
            .signal_handle
            .signal_channel
            .send_async(ActorSignal::Stop)
            .await
            .is_err()
        {
            return Err(anyhow!("Failed to send a stop signal to the actor."));
        }
        Ok(())
    }
    pub fn kill(&self) {
        self.raw.signal_handle.kill_signal.set_one();
    }
}






impl<T> Addr<T>
where
    T: Actor,
{
    pub async fn call<P>(&self, param: P) -> Result<<T as ActorCall<P>>::Output, anyhow::Error>
    where
        T: ActorCall<P>,
        P: 'static,
    {
        let (tx, rx) = Promise::new();
        let boxed: BoxedCall<T> = Box::new(move |this, state| {
            let fut = <T as ActorCall<P>>::call(this, param, state);
            Box::pin(async move {
                let _ = rx.resolve(fut.await);
            })
        });

        self.raw
            .msg_channel
            .send_async(NormalActorMessage::Call(boxed))
            .await
            .map_err(|_| anyhow!("Failed to send."))?;

        tx.await.map_err(|_| anyhow!("Call canceled."))
    }
}

impl<T> Clone for Addr<T>
where
    T: Actor,
{
    fn clone(&self) -> Self {
        Self {
            raw: self.raw.clone(),
            _marker: PhantomData,
        }
    }
}

pub enum ActorMessage<T: Actor> {
    Signal(ActorSignal),
    Call(BoxedCall<T>),
    Message(T::Message),
}

enum NormalActorMessage<T>
where 
    T: Actor
{
    Call(BoxedCall<T>),
    Message(T::Message)
}

async fn actor_action_loop<A>(
    addr: &Addr<A>,
    ctx: &mut InternalActorRunCtx,
    msg_rx: &Receiver<NormalActorMessage<A>>,
    sig_rx: &Receiver<ActorSignal>,
    state: &mut A::State,
) -> Result<ActorSignal>
where
    A: Actor,
{
    loop {
        match multiplex_actor_channels::<A>(msg_rx, sig_rx).await {
            Ok(msg) => match msg {
                ActorMessage::Signal(sig) => {
                    break Ok(sig);
                }
                ActorMessage::Call(call) => {
                    println!("Received a call...");
                    call(SelfAddr { addr: addr, internal: ctx }, state).await;
                    // call(SelfAddr { addr: addr, internal: ctx }, state);

                }
                ActorMessage::Message(msg) => {
                    let _ = A::handle(
                        SelfAddr {
                            addr: &addr,
                            internal: ctx,
                        },
                        msg,
                        state,
                    )
                    .await;
                }
            },
            Err(e) => {
                println!("Failed to receive!");
            }
        }
    }

    // Ok(())
}

pub fn spawn_actor<A>(
    executor: &'static Executor<'static>,
    arguments: A::Arguments,
) -> anyhow::Result<(Addr<A>, Task<()>)>
where
    A: Actor,
{
    let (msg_tx, msg_rx) = flume::unbounded();
    let (sig_tx, sig_rx) = flume::unbounded();
    let mut state = A::pre_start(arguments)?;
    let mut context = InternalActorRunCtx {
        executor,
        tasks: vec![],
        foreigns: vec![]
    };

    let addr = Addr {
        raw: RawHandle {
            msg_channel: msg_tx,
            signal_handle: SignalHandle {
                signal_channel: sig_tx,
                kill_signal: Rc::new(LocalEvent::new()),
            },
        },
        _marker: PhantomData::<A>,
    };

    let task = executor.spawn({
        let addr = addr.clone();
        async move {
            // let mut death_reason = None;
            let _ = A::post_start(
                SelfAddr {
                    addr: &addr,
                    internal: &mut context,
                },
                &mut state,
            )
            .await;
            let death_reason = smol::future::race(
                actor_action_loop(&addr, &mut context, &msg_rx, &sig_rx, &mut state),
                async {
                    addr.raw.signal_handle.kill_signal.wait().await;
                    // println!("triggered???");
                    // death_reason = Some(ActorSignal::Kill);
                    Ok(ActorSignal::Kill)
                },
            )
            .await;
            let death_reason = death_reason.expect("No death reason.");
            let _ = A::post_stop(
                SelfAddr {
                    addr: &addr,
                    internal: &mut context,
                },
                &mut state,
            )
            .await;
            match death_reason {
                ActorSignal::Kill => {
                    for task in &context.tasks {
                        task.handle.kill_signal.set_one();
                    }
                    for task in &context.foreigns {
                        let _ = task.kill().await;
                    }
                }
                ActorSignal::Stop => {
                    for task in &context.tasks {
                        let _ = task
                            .handle
                            .signal_channel
                            .send_async(ActorSignal::Stop)
                            .await;
                    }
                    for task in &context.foreigns {
                        let _ = task.stop().await;
                    }
                }
            }
            for task in context.tasks {
                task.task.await;
            }
        }
    });

    Ok((addr, task))
}


#[derive(Debug)]
// Foreign Addresses -- Allow us to 
// send to an actor from another thread.
pub struct FornAddr<A>
where 
    A: Actor
{
    pub(crate) signal: FornSignalHandle,
    pub(crate) _marker: PhantomData<A>
}

#[derive(Debug)]
pub struct FornSignalHandle {
    pub(crate) shard: ShardId,
    pub(crate) mail: MailId
}


impl<A> Clone for FornAddr<A>
where 
    A: Actor
{
    fn clone(&self) -> Self {
        Self {
            signal: FornSignalHandle {
                mail: self.signal.mail,
                shard: self.signal.shard
            },
            _marker: PhantomData,
        }
    }
}



impl FornSignalHandle {
    pub async fn stop(&self) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();

        let id = self.mail;
        submit_to(self.shard, move |_| {
            match signal_actor_mailbox(id, ActorSignal::Stop) {
                Ok(v) => {
                    let _ = tx.send(Ok(v));
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });

        Ok(rx.await??)
    }

    pub async fn kill(&self) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();

        let id = self.mail;
        submit_to(self.shard, move |_| {
            match signal_actor_mailbox(id, ActorSignal::Kill) {
                Ok(v) => {
                    let _ = tx.send(Ok(v));
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });

        Ok(rx.await??)
    }
}

unsafe impl<A> Send for FornAddr<A>
where 
    A: Actor {}

impl<A> FornAddr<A>
where 
    A: Actor
{

    pub async fn stop(&self) -> anyhow::Result<()>
    {
        // let (tx, rx) = oneshot::channel::<anyhow::Result<()>>();
        
        // let addr2 = self.clone();
        // submit_to(self.shard, |_| {
        //     let Some(addr) = get_actor_addr(addr2) else {
        //         let _ = tx.send(Err((anyhow!("Failed to actually find the actor on the other core."))));
        //         return;
        //     };

        //     spawn_async_task(async move {
        //          match addr.stop().await {
        //             Ok(v) => {
        //                 let _ = tx.send(Ok(v));
        //             }
        //             Err(e) => {
        //                 let _ = tx.send(Err((anyhow!("Failed to send."))));
        //             }
        //         }
        //     }).detach();
        // });

        // match rx.await {
        //     Ok(a) => {
        //         return a;
        //     }
        //     Err(e) => return Err(anyhow!("Failed"))
        // }
        // Ok(())

        self.signal.stop().await
        // Ok(rx.await??)
        // Ok(rx.await??)
    }

    // async fn access_actor_remotely(&self, F)
    pub async fn kill(&self) -> anyhow::Result<()>
    // where
        // A::Message: Send + 'static
    {
        // let (tx, rx) = oneshot::channel::<anyhow::Result<()>>();
        
        // let addr2 = self.clone();
        // submit_to(self.shard, |_| {
        //     let Some(addr) = get_actor_addr(addr2) else {
        //         // let _ = tx.send(Err((anyhow!("Failed to actually find the actor on the other core."))));
        //         return;
        //     };
        //     addr.kill();
        // });
        self.signal.kill().await
    }
    pub async fn send(&self, message: A::Message) -> anyhow::Result<()>
    where
        A::Message: Send + 'static
    {
        let (tx, rx) = oneshot::channel::<anyhow::Result<()>>();
        
        let addr2 = self.clone();
        submit_to(self.signal.shard, |_| {
            let Some(addr) = get_actor_addr(addr2) else {
                let _ = tx.send(Err((anyhow!("Failed to actually find the actor on the other core."))));
                return;
            };

            spawn_async_task(async move {
                 match addr.send(message).await {
                    Ok(v) => {
                        let _ = tx.send(Ok(v));
                    }
                    Err(e) => {
                        let _ = tx.send(Err((anyhow!("Failed to send."))));
                    }
                }
            }).detach();
        });

        match rx.await {
            Ok(a) => {
                return a;
            }
            Err(e) => return Err(anyhow!("Failed"))
        }
        Ok(())

        // Ok(rx.await??)
        // Ok(rx.await??)
    }
    pub async fn call<P>(&self, param: P) -> Result<<A as ActorCall<P>>::Output, anyhow::Error>
    where
        A: ActorCall<P>,
        A::Output: Send,
        P: Send + 'static,
        A::Message: Send + 'static
    {

        let addr2 = self.clone();

        let (tx, rx) = oneshot::channel();


        submit_to(self.signal.shard, |_| {
            // spawn_async_task(future)
            let Some(addr) = get_actor_addr(addr2) else {
                let _ = tx.send(Err(anyhow!("Failed to actually find the actor on the other core.")));
                return;
            };

            spawn_async_task(async move {
                 match addr.call(param).await {
                    Ok(v) => {
                        let _ = tx.send(Ok(v));
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                    }
                }
            }).detach();

           

            // let output = addr.call(param).await

        });



     

        Ok(rx.await??)

    }
}


#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use flume::Sender;
    use futures::{channel::oneshot, future::pending};

    use crate::core::{
        actor::base::{spawn_actor, Actor, ActorCall},
        executor::scheduler::Executor, shard::state::ShardId,
    };

    #[test]
    pub fn test_actor_basic() {
        let executor = Executor::new(ShardId::new(0)).leak();

        /// Define an actor that adds numbers to a base.
        struct BasicActor;

        impl Actor for BasicActor {
            type Message = (i32, oneshot::Sender<i32>);
            type Arguments = i32;
            type State = i32;
            fn name() -> &'static str {
                "BasicActor"
            }
            fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
                Ok(arguments)
            }
            async fn handle(
                this: super::SelfAddr<'_, Self>,
                message: Self::Message,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                let (to_add, returner) = message;
                let result = *state + to_add;
                let _ = returner.send(result);
                *state += 1;
                Ok(())
            }
        }

        smol::future::block_on(executor.run(async {
            let (actor, handle) = spawn_actor::<BasicActor>(executor, 5)?;

            let (tx, rx) = oneshot::channel();
            actor.send((10, tx)).await.unwrap();
            let result = rx.await?;
            assert_eq!(result, 15);

            // Now it should increase by one, so the base should
            // be 6.
            let (tx, rx) = oneshot::channel();
            actor.send((10, tx)).await.unwrap();
            let result = rx.await?;
            assert_eq!(result, 16);

            // Now we kill the actor.
            actor.stop().await?;

            handle.await;

            Ok::<_, anyhow::Error>(())
        }))
        .unwrap();
    }

    #[test]
    pub fn test_actor_kill() {
        let executor = Executor::new(ShardId::new(0)).leak();

        struct DeathActor;

        impl Actor for DeathActor {
            type Arguments = ();
            type Message = ();
            type State = ();
            fn name() -> &'static str {
                "DeathActor"
            }
            fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
                Ok(arguments)
            }
            async fn handle(
                _: super::SelfAddr<'_, Self>,
                _: Self::Message,
                _: &mut Self::State,
            ) -> anyhow::Result<()> {
                Ok(())
            }
        }

        smol::future::block_on(executor.run(async {
            let (actor, handle) = spawn_actor::<DeathActor>(executor, ())?;

            actor.kill();

            handle.await;

            Ok::<_, anyhow::Error>(())
        }))
        .unwrap();
    }

    #[test]
    pub fn test_actor_children_stop() {
        let holder = Rc::new(RefCell::new(Vec::new()));

        let executor = Executor::new(ShardId::new(0)).leak();

        // let mut collector = vec![];
        struct ChildActor;

        impl Actor for ChildActor {
            type Arguments = (usize, Rc<RefCell<Vec<usize>>>);
            type Message = ();
            type State = (usize, Rc<RefCell<Vec<usize>>>);
            fn name() -> &'static str {
                "ChildActor"
            }
            fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
                Ok(arguments)
            }
            async fn post_start(
                mut this: super::SelfAddr<'_, Self>,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                if state.0 <= 1 {
                    this.spawn_linked::<Self>((state.0 + 1, state.1.clone()))
                        .await?;
                }
                Ok(())
            }
            async fn handle(
                mut this: super::SelfAddr<'_, Self>,
                _: Self::Message,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn post_stop(
                this: super::SelfAddr<'_, Self>,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                let (mut a, b) = state;
                // let
                b.borrow_mut().push(a);
                // println!("Stop {}", *state);
                Ok(())
            }
        }

        let holder2 = holder.clone();

        smol::future::block_on(executor.run(async move {
            let (actor, handle) = spawn_actor::<ChildActor>(executor, (0, holder.clone()))?;

            actor.stop().await?;

            // let collect =
            // executor.sleep(Duration::from_millis(250)).await;

            handle.await;

            Ok::<_, anyhow::Error>(())
        }))
        .unwrap();

        let result = holder2.borrow().clone();
        assert_eq!(&result, &[0, 1, 2]);
        // println!("RESULT: {:?}", result);
    }

    #[test]
    pub fn test_actor_children_kill() {
        let holder = Rc::new(RefCell::new(Vec::new()));

        let executor = Executor::new(ShardId::new(0)).leak();

        // let mut collector = vec![];
        struct ChildActor;

        impl Actor for ChildActor {
            type Arguments = (usize, Rc<RefCell<Vec<usize>>>);
            type Message = ();
            type State = (usize, Rc<RefCell<Vec<usize>>>);
            fn name() -> &'static str {
                "ChildActor"
            }
            fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
                Ok(arguments)
            }
            async fn post_start(
                mut this: super::SelfAddr<'_, Self>,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                if state.0 <= 1 {
                    this.spawn_linked::<Self>((state.0 + 1, state.1.clone()))
                        .await?;
                }
                Ok(())
            }
            async fn handle(
                mut this: super::SelfAddr<'_, Self>,
                _: Self::Message,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn post_stop(
                this: super::SelfAddr<'_, Self>,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                let (mut a, b) = state;
                // let
                b.borrow_mut().push(a);
                // println!("Stop {}", *state);
                Ok(())
            }
        }

        let holder2 = holder.clone();

        smol::future::block_on(executor.run(async move {
            let (actor, handle) = spawn_actor::<ChildActor>(executor, (0, holder.clone()))?;

            actor.kill();

            // let collect =
            // executor.sleep(Duration::from_millis(250)).await;

            handle.await;

            Ok::<_, anyhow::Error>(())
        }))
        .unwrap();

        let result = holder2.borrow().clone();
        assert_eq!(&result, &[0, 1, 2]);
        // println!("RESULT: {:?}", result);
    }

    #[test]
    pub fn test_actor_interval_basic() {
        struct BasicIntervalActor;

        impl Actor for BasicIntervalActor {
            type Arguments = oneshot::Sender<usize>;
            type Message = ();
            type State = Option<oneshot::Sender<usize>>;

            fn name() -> &'static str {
                ""
            }

            fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
                // println!("pre starting..");
                Ok(Some(arguments))
            }
            async fn post_start(
                mut this: super::SelfAddr<'_, Self>,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                // println!("Post start called...");
                if let Some(st) = state.take() {
                    // p
                    // println!("work from home");
                    this.external_event(|_| async move {
                        // println!("hello..");
                        let _ = st.send(5);
                    })
                    .await?;
                } else {
                    panic!("No sender.");
                }
                Ok(())
            }
        }

        let executor = Executor::new(ShardId::new(0)).leak();
        smol::future::block_on(executor.run(async {
            let (tx, rx) = oneshot::channel();
            let (actor, handle) = spawn_actor::<BasicIntervalActor>(executor, tx)?;

            // executor.sleep(Duration::from_millis(1)).await;\
            assert_eq!(rx.await?, 5);

            actor.stop().await?;
            handle.await;

            // rx.await.unwrap();

            Ok::<_, anyhow::Error>(())
        }))
        .unwrap();
    }

    #[test]
    pub fn test_actor_event_cancelled() {
        struct IntervalCancelActor;

        struct DropAnchor(flume::Sender<usize>);

        impl Drop for DropAnchor {
            fn drop(&mut self) {
                println!("gone");
                let _ = self.0.send(5);
            }
        }

        impl Actor for IntervalCancelActor {
            type Arguments = Sender<usize>;
            type State = Sender<usize>;
            type Message = ();
            fn name() -> &'static str {
                ""
            }
            fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
                Ok(arguments.clone())
            }
            async fn post_start(
                mut this: super::SelfAddr<'_, Self>,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                let val = state.clone();
                this.external_event(async |_| {
                    val.send_async(4).await.unwrap();
                    let _anchor = DropAnchor(val);
                    pending::<()>().await
                })
                .await?;
                Ok(())
            }
        }

        let executor = Executor::new(ShardId::new(0)).leak();
        smol::future::block_on(executor.run(async {
            let (tx, rx) = flume::unbounded();
            let (actor, handle) =
                spawn_actor::<IntervalCancelActor>(executor, tx.clone())?;


            assert_eq!(rx.recv_async().await?, 4);

            actor.stop().await?;
            // println!("Stopped..");
            handle.await;
            // println!("Awaited handle..,.");

            assert_eq!(rx.recv_async().await.unwrap(), 5);
            // let v =

            Ok::<_, anyhow::Error>(())
        }))
        .unwrap();
    }

    #[test]
    pub fn test_actor_call() {
        struct BasicActor;

        impl Actor for BasicActor {
            type Message = ();
            type Arguments = ();
            type State = ();
            fn name() -> &'static str {
                ""
            }
            fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
                Ok(())
            }
        }

        impl ActorCall<usize> for BasicActor {
            type Output = usize;
            async fn call<'a>(
                    _: super::SelfAddr<'a, Self>,
                    msg: usize,
                    _: &'a mut Self::State,
                ) -> Self::Output {
                msg + 1
            }
        }

        impl ActorCall<bool> for BasicActor {
            type Output = bool;
            async fn call<'a>(
                    _: super::SelfAddr<'a, Self>,
                    msg: bool,
                    _: &'a mut Self::State,
                ) -> Self::Output {
                !msg
            }
        }

        let executor = Executor::new(ShardId::new(0)).leak();
        smol::future::block_on(executor.run(async {

            let (actor, handle) = spawn_actor::<BasicActor>(executor, ())?;
            
            let hi = actor.call(3).await?;
            assert_eq!(hi, 4);

            assert_eq!(actor.call(true).await?, false);
            assert_eq!(actor.call(false).await?, true);


            actor.stop().await?;
            handle.await;

            Ok::<_, anyhow::Error>(())
        })).unwrap();

        // impl ActorCall<usize> for BasicActor {
        //     fn call<'a>(
        //             this: super::SelfAddr<'a, Self>,
        //             msg: usize,
        //             state: &'a mut Self::State,
        //         ) -> Self::Fut<'a> {
                
        //     }

        // }
    }
}
