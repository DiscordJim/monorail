use std::{future::Future, marker::PhantomData, ops::{Add, Deref}, rc::Rc};

use anyhow::{anyhow, Result};
use flume::{Receiver, Sender, TrySendError};
use futures::channel::oneshot;
use smol::{future::race, Task};

use crate::{core::{
    actor::{manager::{ActorIndex, Addr, AnonymousAddr, ThreadActorManager}, signals::{SignalBus, SignalPriority}}, alloc::MonoVec, channels::promise::{Promise, PromiseError}, executor::{
        helper::{select2, Select2Result},
        scheduler::Executor,
    }, shard::{
        shard::{self, submit_to},
        state::ShardId,
    }, task::{Init, TaskControlBlock, TaskControlBlockVTable, TaskControlHeader}
}, monolib, monovec};

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


pub enum SubTaskHandle {
    EventLoop {
        bus: Rc<SignalBus>,
        task: Option<Task<()>>
    },
    Handle(AnonymousAddr)
}

impl SubTaskHandle {
    pub async fn kill(&self) -> anyhow::Result<()> {
        match self {
            Self::Handle(handle) => {
                handle.kill().await?;
            },
            Self::EventLoop { bus, .. } => {
                bus.raise(ActorSignal::Kill, SignalPriority::Interrupt);
            }
        }
        Ok(())
    }
    pub async fn stop(&self) -> anyhow::Result<()> {
        match self {
            Self::Handle(handle) => {
                handle.stop().await?;
            }
            Self::EventLoop { bus, .. } => {
                bus.raise(ActorSignal::Stop, SignalPriority::NonCritical);
            }
        }
        Ok(())
    }
    pub async fn wait(&mut self) {
        match self {
            Self::EventLoop { bus, task } => {
                task.take().unwrap().await;
            }
            Self::Handle(handle) => {
                // ...
                let _ = handle.await_death().await;
            }
        }
    }
}

// struct SubTaskHandle {
//     // task: Task<()>,
//     handle: AnonymousAddr
// }

struct InternalActorRunCtx {
    executor: &'static Executor<'static>,
    tam: &'static ThreadActorManager,
    tasks: MonoVec<SubTaskHandle>,
    foreigns: MonoVec<AnonymousAddr>,
    parent: Option<AnonymousAddr>
}

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

            let actor = shard::spawn_actor::<B>(arguments);
            let _ = tx.send(actor.downgrade());
            // let (foreign, local) = shard::spawn_actor(arguments);
        });

        let handler = rx.await?;

        self.internal.foreigns.push(handler.clone().downgrade());

        Ok(handler)
    }
    pub async fn spawn_linked<B>(&mut self, arguments: B::Arguments) -> Result<()>
    where
        B: Actor,
    {
        
        let child = self.internal.tam.spawn_actor_with_parent::<B>(self.internal.executor, arguments, self.anon);
        self.internal.tasks.push(SubTaskHandle::Handle(child.downgrade()));

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
        let ks2 = ks.clone();
        let loopa = self.internal.executor.spawn(async move {
            let _ = race(fut, async {
                ks2.wait(SignalPriority::NonCritical).await;
                // let _ = select2(s_rx.recv_async(), async { ks.wait(SignalPriority::Interrupt).await; } ).await;
            })
            .await;
        });

        self.internal.tasks.push(SubTaskHandle::EventLoop {
            bus: ks.clone(),

            task: Some(loopa)
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

impl<A> RawHandle<A>
where 
    A: Actor
{
    #[inline]
    pub fn stop(&self) {
        self.signal_handle.raise(ActorSignal::Stop, SignalPriority::NonCritical);
    }
    #[inline]
    pub fn kill(&self) {
        self.signal_handle.raise(ActorSignal::Kill, SignalPriority::Interrupt);
    }
    #[inline]
    pub fn supervsn_msg(&self, msg: SupervisorMessage) -> Result<(), flume::TrySendError<SupervisorMessage>> {
        match self.msg_channel.try_send(NormalActorMessage::Supervision(msg)) {
            Ok(_) => Ok(()),
            Err(e) => match e {
                flume::TrySendError::Disconnected(e) => if let NormalActorMessage::Supervision(msg) = e {
                    Err(TrySendError::Disconnected(msg))
                } else {
                    panic!("Failed to send.")
                }
                TrySendError::Full(e) => if let NormalActorMessage::Supervision(msg) = e {
                    Err(TrySendError::Full(msg))
                } else {
                    panic!("Failed to extract.")
                }
            },
        }
        // Ok(())
    }
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
            NormalActorMessage::Supervision(superv) => Ok(ActorMessage::Supervision(superv))
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
    // fn name() -> &'static str;

    fn pre_start(arguments: Self::Arguments) -> impl Future<Output = Self::State>;

    fn handle_child_msg(
        this: SelfAddr<'_, Self>,
        state: &mut Self::State,
        message: SupervisorMessage
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
    anon: AnonymousAddr
}





impl<T> LocalAddr<T>
where
    T: Actor,
{
    /// SAFETY: The pointer needs to be non-null.
    pub(crate) unsafe fn from_raw(this: *const RawHandle<T>, anon: AnonymousAddr) -> Self
    where
        T: Actor,
    {
        Self {
            raw: Rc::from_raw(this.cast_mut()),
            anon
            // _marker: PhantomData,
        }
    }
    #[inline]
    pub fn downgrade(self) -> Addr<T> {
        unsafe { Addr::from_parts(self.anon) }
    }
    pub(crate) fn to_raw_ptr(self) -> *const RawHandle<T> {
        Rc::into_raw(self.raw)
    }
    pub(crate) async fn send_supervision_message(&self, message: SupervisorMessage) -> anyhow::Result<(), SupervisorMessage> {
        if let Err(e) = self
            .raw
            .msg_channel
            .send_async(NormalActorMessage::Supervision(message))
            .await
        {
            // retu
            if let NormalActorMessage::Supervision(m) = e.into_inner() {
                return Err(m);
            }
        }
        Ok(())
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
        println!("send the message");
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
    pub async fn await_death(&self) -> Result<(), PromiseError> {
        self.anon.await_death().await
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
            anon: self.anon.clone()
            // _marker: PhantomData,
        }
    }
}

pub(crate) enum ActorMessage<T: Actor> {
    Signal(ActorSignal),
    Call(TaskControlBlockVTable<Init>),
    Message(T::Message),
    Supervision(SupervisorMessage)
}

enum NormalActorMessage<T>
where
    T: Actor,
{
    Call(TaskControlBlockVTable<Init>),
    Message(T::Message),
    Supervision(SupervisorMessage)
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

                    println!("Acotr was signalled");
                    break Ok(sig);
                }
                ActorMessage::Supervision(supervision) => {
                    match A::handle_child_msg(SelfAddr { addr: addr, internal: ctx }, state, supervision).await {
                        Ok(_) => {

                        }
                        Err(e) => {

                        }
                    }
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
}

// pub fn spawn_actor<A>(
//     executor: &'static Executor<'static>,
//     arguments: A::Arguments
// ) -> anyhow::Result<(LocalAddr<A>, Task<()>)>
// where 
//     A: Actor
// {
//     Ok(spawn_actor_full(shexecutor, arguments, None))
// }

pub fn spawn_actor_full<A>(
    reservation: AnonymousAddr,
    tam: &'static ThreadActorManager,
    executor: &'static Executor<'static>,
    arguments: A::Arguments,
    parent: Option<AnonymousAddr>
) -> (LocalAddr<A>, Task<()>)
where
    A: Actor,
{
    let (msg_tx, msg_rx) = flume::unbounded();
    // let (sig_tx, sig_rx) = flume::unbounded();
    
    let mut context = InternalActorRunCtx {
        executor,
        tasks: monovec![],
        foreigns: monovec![],
        tam,
        parent
    };

    let addr = LocalAddr {
        raw: Rc::new(RawHandle {
            msg_channel: msg_tx,
            signal_handle: Rc::new(SignalBus::new()),
        }),
        anon: reservation,
        // _marker: PhantomData::<A>,
    };

    let task = executor.spawn({
        
        let addr = addr.clone();
        async move {
            // let mut death_reason = None;

            let mut state = A::pre_start(arguments).await else {
                return;
            };

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
            
            println!("actor did die");

            // if context.

            match death_reason {
                ActorSignal::Kill => {
                    for task in &*context.tasks {
                        let _ = task
                            .kill().await;
                    }
                    for task in &*context.foreigns {
                        let _ = task.kill().await;
                    }
                }
                ActorSignal::Stop => {
                    for task in &*context.tasks {
                        let _ = task.stop().await;
                    }
                    for task in &*context.foreigns {
                        let _ = task.stop().await;
                    }
                }
            }
            for mut task in &mut *context.tasks {
                task.wait().await;
            }

            context.tam.unmanage(&addr.clone().anon);

            
            let _ = A::post_stop(
                SelfAddr {
                    addr: &addr,
                    internal: &mut context,
                },
                &mut state,
            )
            .await;

            if let Some(parent) = context.parent {
                let a = match death_reason {
                    ActorSignal::Kill => SupervisorMsgType::Killed,
                    ActorSignal::Stop => SupervisorMsgType::Stopped
                };

                let _ = parent.send_supervision_message(SupervisorMessage {
                    msg_type: a,
                    source: addr.anon
                }).await;
            }
        }
    });

    (addr, task)
}


#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, sync::{Arc, Mutex}, time::Duration};

    use flume::Sender;
    use futures::{channel::oneshot, future::pending};

    use crate::{core::{
        actor::base::{Actor, ActorCall},
        executor::scheduler::Executor,
        shard::{shard::{access_shard_ctx_ref, signal_monorail, spawn_actor}, state::ShardId}, topology::MonorailTopology,
    }, monolib};

    #[test]
    pub fn test_actor_basic() {
        /// Define an actor that adds numbers to a base.
        struct BasicActor;

        impl Actor for BasicActor {
            type Message = (i32, oneshot::Sender<i32>);
            type Arguments = i32;
            type State = i32;


            async fn pre_start(arguments: Self::Arguments) -> Self::State {
                arguments
            }
            async fn handle(
                _: super::SelfAddr<'_, Self>,
                message: Self::Message,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                let (to_add, returner) = message;
                let result = *state + to_add;
                let _ = returner.send(result);
                *state += 1;
                Ok(())
            }
            async fn post_stop(
                    _: super::SelfAddr<'_, Self>,
                    _: &mut Self::State,
                ) -> anyhow::Result<()> {
                signal_monorail(Ok(()));
                Ok(())
            }
        }

        MonorailTopology::normal(async || {
            let actor = monolib::spawn_actor::<BasicActor>(5);

            // access_shard_ctx_ref().executor.sleep(Duration::from_millis(100)).await;

            let (tx, rx) = oneshot::channel();
            // println!("hello..")
            actor.send((10, tx)).await.unwrap();
            let result = rx.await.unwrap();
            assert_eq!(result, 15);

            // Now it should increase by one, so the base should
            // be 6.
            let (tx, rx) = oneshot::channel();
            actor.send((10, tx)).await.unwrap();
            let result = rx.await.unwrap();
            assert_eq!(result, 16);

            // println!("homes");

            // Now we kill the actor.
            actor.stop();

            // handle.await;
        }).unwrap();
    }

    #[test]
    pub fn test_delayed_startup() {

        struct BasicActor;

        impl Actor for BasicActor {
            type Message = (i32, oneshot::Sender<i32>);
            type Arguments = i32;
            type State = i32;


            async fn pre_start(arguments: Self::Arguments) -> Self::State {
                monolib::sleep(Duration::from_millis(100)).await;
                arguments
            }
            async fn handle(
                _: super::SelfAddr<'_, Self>,
                message: Self::Message,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                let (to_add, returner) = message;
                let result = *state + to_add;
                // println!("cal")
                 returner.send(result).unwrap();
                //  panic!("what");
                *state += 1;
                Ok(())
            }
            async fn post_stop(
                    _: super::SelfAddr<'_, Self>,
                    _: &mut Self::State,
                ) -> anyhow::Result<()> {
                signal_monorail(Ok(()));
                Ok(())
            }
        }


        MonorailTopology::normal(async || {

            let actor = monolib::spawn_actor::<BasicActor>(5);


            let (tx, rx) = oneshot::channel();
            actor.send((10, tx)).await.unwrap();
            let result = rx.await.unwrap();
            assert_eq!(result, 15);

            signal_monorail(Ok(()));

        }).unwrap();
    }


    #[test]
    pub fn test_actor_kill() {
        // let executor = Executor::new(ShardId::new(0)).leak();

        struct DeathActor;

        impl Actor for DeathActor {
            type Arguments = ();
            type Message = ();
            type State = ();

            async fn pre_start(arguments: Self::Arguments) -> Self::State {
                arguments
            }
            async fn handle(
                _: super::SelfAddr<'_, Self>,
                _: Self::Message,
                _: &mut Self::State,
            ) -> anyhow::Result<()> {
                Ok(())
            }
        }

        MonorailTopology::normal(async || {
            let actor = monolib::spawn_actor::<DeathActor>(());
            actor.kill();

            // TODO: wait hgeere

            signal_monorail(Ok(()));
        }).unwrap();

    }

    #[test]
    pub fn test_actor_child_supervise() {
        struct DeathActor;

        impl Actor for DeathActor {
            type Arguments = bool;
            type Message = ();
            type State = bool;
            async fn pre_start(arguments: Self::Arguments) -> Self::State {
                arguments   
            }
            async fn handle(
                    _: super::SelfAddr<'_, Self>,
                    _: Self::Message,
                    _: &mut Self::State,
                ) -> anyhow::Result<()> {
                Ok(())
            }
            async fn post_start(
                    mut this: super::SelfAddr<'_, Self>,
                    state: &mut Self::State,
                ) -> anyhow::Result<()> {
                if !*state {
                    this.spawn_linked::<DeathActor>(true).await?;
                } else {
                    this.stop();
                }
                Ok(()) 
            }
            async fn handle_child_msg(
                    _: super::SelfAddr<'_, Self>,
                    state: &mut Self::State,
                    _: super::SupervisorMessage
                ) -> anyhow::Result<()> {
                if !*state {
                    // println!("got a chhild message;");
                    signal_monorail(Ok(()));
                }
                Ok(())
            }
        }

        MonorailTopology::normal(async || {
            let _ = monolib::spawn_actor::<DeathActor>(false);




        }).unwrap();
    }

    #[test]
    pub fn test_actor_children_stop() {
        let holder = Arc::new(Mutex::new(Vec::new()));

        // let executor = Executor::new(ShardId::new(0)).leak();

        // let mut collector = vec![];
        struct ChildActor;

        impl Actor for ChildActor {
            type Arguments = (usize, Arc<Mutex<Vec<usize>>>);
            type Message = ();
            type State = (usize, Arc<Mutex<Vec<usize>>>);
          
            async fn pre_start(arguments: Self::Arguments) -> Self::State {
                arguments
            }
            async fn post_start(
                mut this: super::SelfAddr<'_, Self>,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                println!("hello {:?}", state);
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
                println!("bye {:?}", state);
                let (mut a, b) = state;
                // let
                b.lock().unwrap().push(a);
                // println!("Stop {}", *state);
                Ok(())
            }
        }

        let holder2 = holder.clone();


        MonorailTopology::normal(async move || {
            let actor = monolib::spawn_actor::<ChildActor>((0, holder.clone()));
            
            // Give the actor time to start up it's stuff.
            monolib::sleep(Duration::from_millis(10)).await;


            
            actor.stop();

            actor.await_death().await.unwrap();

            signal_monorail(Ok(()));
            
        }).unwrap();



        let result = holder2.lock().unwrap().clone();
        assert_eq!(&result, &[2, 1, 0]);
        // println!("RESULT: {:?}", result);
    }

    #[test]
    pub fn test_actor_children_kill() {
        let holder = Arc::new(Mutex::new(Vec::new()));

        // let executor = Executor::new(ShardId::new(0)).leak();

        // let mut collector = vec![];
        struct ChildActor;

        impl Actor for ChildActor {
            type Arguments = (usize, Arc<Mutex<Vec<usize>>>);
            type Message = ();
            type State = (usize, Arc<Mutex<Vec<usize>>>);

            async fn pre_start(arguments: Self::Arguments) -> Self::State {
                arguments
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
                b.lock().unwrap().push(a);
                // println!("Stop {}", *state);
                Ok(())
            }
        }

        let holder2 = holder.clone();


        MonorailTopology::normal(async move || {
            let actor = spawn_actor::<ChildActor>((0, holder.clone()));

            actor.kill();

            actor.await_death().await.unwrap();

            signal_monorail(Ok(()));
        }).unwrap();

        

        let result = holder2.lock().unwrap().clone();
        assert_eq!(&result, &[2, 1, 0]);
        // println!("RESULT: {:?}", result);
    }

    #[test]
    pub fn test_actor_interval_basic() {
        struct BasicIntervalActor;

        impl Actor for BasicIntervalActor {
            type Arguments = oneshot::Sender<usize>;
            type Message = ();
            type State = Option<oneshot::Sender<usize>>;



            async fn pre_start(arguments: Self::Arguments) -> Self::State {
                // println!("pre starting..");
                Some(arguments)
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

        MonorailTopology::normal(async move || {
            let (tx, rx) = oneshot::channel();
            let actor = monolib::spawn_actor::<BasicIntervalActor>(tx);
            assert_eq!(rx.await.unwrap(), 5);

            actor.stop();

            actor.await_death().await.unwrap();

            signal_monorail(Ok(()));

            // Ok(())
        }).unwrap();

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

            async fn pre_start(arguments: Self::Arguments) -> Self::State {
                arguments.clone()
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

        MonorailTopology::normal(async move || {
            let (tx, rx) = flume::unbounded();
            let actor = monolib::spawn_actor::<IntervalCancelActor>(tx.clone());
            // assert_eq!()

            assert_eq!(rx.recv_async().await.unwrap(), 4);

            actor.stop();

            actor.await_death().await.unwrap();

            signal_monorail(Ok(()));

        }).unwrap();
    }

    #[test]
    pub fn test_actor_call() {
        struct BasicActor;

        impl Actor for BasicActor {
            type Message = ();
            type Arguments = ();
            type State = ();
    
            async fn pre_start(_: Self::Arguments) -> Self::State {
                ()
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

        MonorailTopology::normal(async || {
            let actor = monolib::spawn_actor::<BasicActor>(());

            let hi = actor.call(3).await.unwrap();
            assert_eq!(hi, 4);

            assert_eq!(actor.call(true).await.unwrap(), false);
            assert_eq!(actor.call(false).await.unwrap(), true);

            actor.stop();
            actor.await_death().await.unwrap();

            signal_monorail(Ok(()));


        }).unwrap();

        

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
