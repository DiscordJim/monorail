use std::{cell::Cell, future::Future, marker::PhantomData, ops::Deref, rc::Rc};

use anyhow::{anyhow, Result};
use asyncnal::{EventSetter, LocalEvent};
use flume::{Receiver, Sender};
use futures::channel::oneshot;
use smol::{future::race, Task};

use crate::{core::{
    actor::{manager::{Addr, AnonymousAddr}, signals::{SignalBus, SignalPriority}}, alloc::MonoVec, channels::promise::Promise, executor::{
        helper::{select2, Select2Result},
        scheduler::Executor,
    }, shard::{
        shard::{self, submit_to},
        state::ShardId,
    }, task::{Init, TaskControlBlock, TaskControlBlockVTable, TaskControlHeader}
}, monovec};

pub trait ActorCall<M>: Actor {
    type Output;

    fn call<'a>(
        this: SelfAddr<'a, Self>,
        msg: M,
        state: &'a mut Self::State,
    ) -> impl Future<Output = Self::Output>;
}

pub struct SupervisorMessage {
    pub source: AnonymousAddr,
    pub msg_type: SupervisorMsgType
}

pub enum SupervisorMsgType {
    Stopped,
    Killed
}



struct SubTaskHandle {
    task: Task<()>,
    handle: Rc<SignalBus>,
}

struct InternalActorRunCtx {
    executor: &'static Executor<'static>,
    tasks: MonoVec<SubTaskHandle>,
    foreigns: MonoVec<AnonymousAddr>,
}



// impl SignalBus {
//     pub async fn wait(&self, level: InterruptLevel) -> ActorSignal {
//         // let mut flag = 
//         while self.flag.get().is_none() {
//             self.event.wait().await;
//         }
//         self.flag.take().unwrap()
//     }
//     pub fn stop(&self) {
//         self.
//     }
// }


pub struct SelfAddr<'a, A>
where
    A: Actor,
{
    addr: &'a LocalAddr<A>,
    internal: &'a mut InternalActorRunCtx,
}

impl<'a, A> SelfAddr<'a, A>
where
    A: Actor,
{
    pub async fn spawn_linked_foreign<B>(
        &mut self,
        core: ShardId,
        arguments: B::Arguments,
    ) -> Result<Addr<B>>
    where
        B: Actor,
        B::Message: Send + 'static,
        B::Arguments: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        submit_to(core, async || {
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

        self.internal.foreigns.push(handler.clone().downgrade());

        Ok(handler)
    }
    pub async fn spawn_linked<B>(&mut self, arguments: B::Arguments) -> Result<()>
    where
        B: Actor,
    {
        let (actor, handle) = spawn_actor::<B>(self.internal.executor, arguments)?;
        self.internal.tasks.push(SubTaskHandle {
            handle: actor.raw.signal_handle.clone(),
            task: handle,
        });

        Ok(())
    }
    pub async fn external_event<F, FUT>(&mut self, func: F) -> Result<()>
    where
        F: FnOnce(LocalAddr<A>) -> FUT,
        FUT: Future<Output = ()> + 'static,
    {
        let fut = func(self.addr.clone());

        // let (s_tx, s_rx) = flume::unbounded();

        let signal = Rc::new(SignalBus::new());

        let ks = signal.clone();
        let loopa = self.internal.executor.spawn(async move {
            let _ = race(fut, async {
                ks.wait(SignalPriority::NonCritical).await;
                // let _ = select2(s_rx.recv_async(), async { ks.wait(SignalPriority::Interrupt).await; } ).await;
            })
            .await;
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
    type Target = LocalAddr<A>;
    fn deref(&self) -> &Self::Target {
        self.addr
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ActorSignal {
    Stop,
    Kill,
}

// #[derive(Clone)]
// pub(crate) struct SignalHandle {
//     signal_channel: Sender<ActorSignal>,
//     kill_signal: Rc<LocalEvent>,
// }


// #[derive(Clone)]
pub(crate) struct RawHandle<A>
where
    A: Actor,
{
    signal_handle: Rc<SignalBus>,
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
    signal: &Rc<SignalBus>,
) -> Result<ActorMessage<A>, flume::RecvError>
where
    A: Actor,
{
    let multi = select2(msg.recv_async(), signal.wait(SignalPriority::NonCritical)).await;
    match multi {
        Select2Result::Left(a) => match a? {
            NormalActorMessage::Call(call) => Ok(ActorMessage::Call(call)),
            NormalActorMessage::Message(me) => Ok(ActorMessage::Message(me)),
        },
        Select2Result::Right(b) => Ok(ActorMessage::Signal(b)),
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

    fn handle_child_msg(
        this: SelfAddr<'_, Self>,
        state: &mut Self::State
    ) -> impl Future<Output = Result<()>> {
        async { Ok(()) }
    }

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



pub struct LocalAddr<T>
where
    T: Actor,
{
    raw: Rc<RawHandle<T>>,
    _marker: PhantomData<T>,
}





impl<T> LocalAddr<T>
where
    T: Actor,
{
    /// SAFETY: The pointer needs to be non-null.
    pub(crate) unsafe fn from_raw(this: *const RawHandle<T>) -> Self
    where
        T: Actor,
    {
        Self {
            raw: Rc::from_raw(this.cast_mut()),
            _marker: PhantomData,
        }
    }
    pub(crate) fn to_raw_ptr(self) -> *const RawHandle<T> {
        Rc::into_raw(self.raw)
    }
    pub async fn send(&self, message: T::Message) -> Result<(), T::Message> {
        if let Err(e) = self
            .raw
            .msg_channel
            .send_async(NormalActorMessage::Message(message))
            .await
        {
            // retu
            if let NormalActorMessage::Message(m) = e.into_inner() {
                return Err(m);
            }
        }
        Ok(())
    }
    pub fn stop(&self)  {
        self
            .raw
            .signal_handle
            .raise(ActorSignal::Stop, SignalPriority::NonCritical);
    }
    pub fn kill(&self) {
        self
            .raw
            .signal_handle
            .raise(ActorSignal::Kill, SignalPriority::Interrupt);
    }
}

impl<T> LocalAddr<T>
where
    T: Actor,
{
    pub async fn call<P>(&self, param: P) -> Result<<T as ActorCall<P>>::Output, anyhow::Error>
    where
        T: ActorCall<P>,
        P: 'static,
    {

        let (rx, tx) = Promise::new();
        let b: TaskControlBlockVTable<Init> = TaskControlBlock::<_, _, _, _>::create_local(TaskControlHeader::FireAndForget, async move |package: &mut Option<(SelfAddr<'_, T>, &mut <T as Actor>::State)>| {

            let (addr, state) = package.take().unwrap();

            let fut = <T as ActorCall<P>>::call(addr, param, state).await;


            let _ = tx.resolve(fut);
        });

        let _ = self.raw
            .msg_channel
            .send_async(NormalActorMessage::Call(b))
            .await;

        Ok(rx.await.map_err(|_| anyhow!("Call canceled"))?)
        
    

        // let (tx, rx) = Promise::new();
        // let boxed: BoxedCall<T> = Box::new(move |this, state| {
        //     // println!("{} [D] Entering call block. {:?}", get_test_name(), thread::current().id());
        //     let fut = <T as ActorCall<P>>::call(this, param, state);
        //     // println!("{} [D] Created a call. {:?}", get_test_name(), thread::current().id());
        //     Box::pin(async move {
        //         // println!("{} [D] Entering async. {:?}", get_test_name(), thread::current().id());

        //         let re = fut.await;
        //         //  println!("{} [D] Resolved fucn async. {:?}", get_test_name(), thread::current().id());

        //         let _ = rx.resolve(re);
        //         // println!("{} [D] Exiting async. {:?}", get_test_name(), thread::current().id());
        //     })
        // });

        // self.raw
        //     .msg_channel
        //     .send_async(NormalActorMessage::Call(boxed))
        //     .await
        //     .map_err(|_| anyhow!("Failed to send."))?;
        // // println!("{} [D] Sent a call. {:?}", get_test_name(), thread::current().id());

        // tx.await.map_err(|_| anyhow!("Call canceled."))
    }
}

impl<T> Clone for LocalAddr<T>
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

pub(crate) enum ActorMessage<T: Actor> {
    Signal(ActorSignal),
    Call(TaskControlBlockVTable<Init>),
    Message(T::Message),
}

enum NormalActorMessage<T>
where
    T: Actor,
{
    Call(TaskControlBlockVTable<Init>),
    Message(T::Message),
}

async fn actor_action_loop<A>(
    addr: &LocalAddr<A>,
    ctx: &mut InternalActorRunCtx,

    msg_rx: &Receiver<NormalActorMessage<A>>,
    sig_rx: &Rc<SignalBus>,
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
                    // println!("Received a call...");
                    let mut package: Option<(SelfAddr<'_, A>, &mut <A as Actor>::State)> = Some((SelfAddr {
                            addr: addr,
                            internal: ctx,
                        }, state));
                    
                    match unsafe { call.run(&mut package) } {
                        Ok(v) => {
                            match v.await {
                                Ok(t) => {
                                    
                                }
                                Err(e) => {

                                }
                            }
                        }
                        Err(e) => {
                            
                        }
                    };
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
            Err(_) => {
                // println!("Failed to receive!");
            }
        }
    }

    // Ok(())
}

pub fn spawn_actor<A>(
    executor: &'static Executor<'static>,
    arguments: A::Arguments,
) -> anyhow::Result<(LocalAddr<A>, Task<()>)>
where
    A: Actor,
{
    let (msg_tx, msg_rx) = flume::unbounded();
    // let (sig_tx, sig_rx) = flume::unbounded();
    let mut state = A::pre_start(arguments)?;
    let mut context = InternalActorRunCtx {
        executor,
        tasks: monovec![],
        foreigns: monovec![],
    };

    let addr = LocalAddr {
        raw: Rc::new(RawHandle {
            msg_channel: msg_tx,
            signal_handle: Rc::new(SignalBus::new()),
        }),
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
                actor_action_loop(&addr, &mut context, &msg_rx, &addr.raw.signal_handle, &mut state),
                async {
                    addr.raw.signal_handle.wait(SignalPriority::Interrupt).await;
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
                    for task in &*context.tasks {
                        task
                            .handle
                            .raise(ActorSignal::Kill, SignalPriority::Interrupt);
                    }
                    for task in &*context.foreigns {
                        let _ = task.kill().await;
                    }
                }
                ActorSignal::Stop => {
                    for task in &*context.tasks {
                        let _ = task
                            .handle
                            
                            .raise(ActorSignal::Stop, SignalPriority::NonCritical);
                    }
                    for task in &*context.foreigns {
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


#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use flume::Sender;
    use futures::{channel::oneshot, future::pending};

    use crate::core::{
        actor::base::{spawn_actor, Actor, ActorCall},
        executor::scheduler::Executor,
        shard::state::ShardId,
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
            actor.stop();

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

            actor.stop();

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

            actor.stop();
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
                // println!("gone");
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
            let (actor, handle) = spawn_actor::<IntervalCancelActor>(executor, tx.clone())?;

            assert_eq!(rx.recv_async().await?, 4);

            actor.stop();
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
            fn pre_start(_: Self::Arguments) -> anyhow::Result<Self::State> {
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

            actor.stop();
            handle.await;

            Ok::<_, anyhow::Error>(())
        }))
        .unwrap();

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
