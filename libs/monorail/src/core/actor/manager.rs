use std::{
    any::TypeId,
    cell::RefCell,
    collections::HashMap,
    future::Future,
    hash::Hash,
    marker::PhantomData,
    mem::ManuallyDrop,
    ops::{Add, Index},
    rc::Rc,
};

use anyhow::anyhow;
use asyncnal::{EventSetter, LocalEvent};
use futures::{stream::FuturesUnordered, StreamExt};
use slotmap::{new_key_type, SlotMap};

use crate::{
    core::{
        actor::base::{Actor, ActorCall, ActorSignal, LocalAddr, RawHandle, SupervisorMessage},
        channels::promise::PromiseError,
        executor::scheduler::Executor,
        shard::{shard::access_shard_ctx_ref, state::ShardId},
    },
    monolib::{self, call_on},
};

struct ActorAddrVTable {
    payload: *const (),
    signal: fn(*const (), ActorSignal) -> anyhow::Result<()>,
    supervision: fn(
        *const (),
        SupervisorMessage,
    ) -> anyhow::Result<(), flume::TrySendError<SupervisorMessage>>,
    _decref: fn(*const ()),
    // drop: fn(*const ())
}

impl Drop for ActorAddrVTable {
    fn drop(&mut self) {
        // (self.)
        (self._decref)(self.payload)
    }
}

#[inline]
const fn build_addr_vtable<A>(addr: *const RawHandle<A>) -> ActorAddrVTable
where
    A: Actor,
{
    ActorAddrVTable {
        payload: addr.cast(),
        signal: |payload, signal| unsafe {
            let m = ManuallyDrop::new(Rc::from_raw(payload.cast::<RawHandle<A>>()));

            match signal {
                ActorSignal::Kill => m.kill(),
                ActorSignal::Stop => m.stop(),
            }

            Ok(())
        },
        supervision: |payload, msg| unsafe {
            let m = ManuallyDrop::new(Rc::from_raw(payload.cast::<RawHandle<A>>()));

            m.supervsn_msg(msg)
        },
        _decref: |payload| unsafe {
            let _ = Rc::from_raw(payload.cast::<RawHandle<A>>());
        },
    }
}

// impl Ac

// #[inline]
// const fn build_addr_vtable()

struct ActorSupportingCtx {
    vtable: ActorAddrVTable,
}

// #[derive(Clone, Copy)]
pub struct Addr<T>
where
    T: Actor,
{
    anonymous: AnonymousAddr,
    _marker: PhantomData<T>,
}

impl<T> Addr<T>
where
    T: Actor,
{
    pub(crate) unsafe fn from_parts(address: AnonymousAddr) -> Self {
        Self {
            anonymous: address,
            _marker: PhantomData,
        }
    }
}

// impl<T>

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnonymousAddr {
    shard: ShardId,
    local_index: ActorIndex,
}

impl<T: Actor> Copy for Addr<T> {}
impl<T: Actor> Clone for Addr<T> {
    fn clone(&self) -> Self {
        Self {
            anonymous: self.anonymous.clone(),
            _marker: PhantomData, // local_index: self.local_index,
        }
    }
}

pub struct TwoWayTranslator<K, V> {
    forward: HashMap<K, V>,
    backward: HashMap<V, K>,
}

impl<K, V> TwoWayTranslator<K, V>
where
    K: Hash + Eq + Clone,
    V: Hash + Eq + Clone,
{
    pub fn insert(&mut self, key: K, value: V) {
        self.forward.insert(key.clone(), value.clone());
        self.backward.insert(value, key);
    }

    pub fn forwards(&self, key: &K) -> Option<&V> {
        self.forward.get(key)
    }
    pub fn backwards(&self, key: &V) -> Option<&K> {
        self.backward.get(key)
    }
}

pub struct ThreadActorManager {
    shard: ShardId,
    map: RefCell<SlotMap<ActorIndex, ActorSupportingCtx>>,
    registry: RefCell<TwoWayTranslator<&'static str, (AnonymousAddr, TypeId)>>,
    // local_registry: RefCell<HashMap<&'static >>
    unmanage_bus: LocalEvent,
}

#[derive(thiserror::Error, Debug)]
pub enum ActorManagementError {
    #[error("Requested local address translation but this is not valid!")]
    NotLocal,
    #[error("Requested translation but the actor no longer exists")]
    ActorNoLongerExists,
    #[error("Error resolving promise")]
    PromiseError(#[from] PromiseError),
}

// pub(crate) async fn access_addr_global<'a, A, T, F>(addr: &Addr<A>, func: T) -> Result<(), ActorManagementError>
// where
//     T: FnOnce(AddrAccess<'a, A>) -> F + Send + 'static,
//     A: Actor
// {
//     let mut local = access_shard_ctx_ref().actors.borrow_mut();
//     if addr.shard == local.shard {
//         let addr = unsafe { local.translate_without_shard_check(addr) }?;
//         func(addr);
//     }

//     Ok(())

// }

// unsafe impl

unsafe impl<A: Actor> Send for Addr<A> {}
unsafe impl<A: Actor> Sync for Addr<A> {}

#[derive(thiserror::Error, Debug)]
pub enum ResolutionError {
    #[error("Wrong type for requested address.")]
    IncorrectType,
    #[error("Could not find registry.")]
    NoEntry,
}

pub(crate) fn tam_resolve<A>(name: &'static str) -> Result<Addr<A>, ResolutionError>
where
    A: Actor,
{
    let local = &access_shard_ctx_ref().actors;

    if let Some((addr, typeid)) = local.registry.borrow().forwards(&name) {
        if *typeid != TypeId::of::<A>() {
            return Err(ResolutionError::IncorrectType);
        }
        Ok(Addr {
            anonymous: *addr,
            _marker: PhantomData,
        })
    } else {
        Err(ResolutionError::NoEntry)
    }
}

// async fn use_anonymous_address<A, T, F, R>(addr: &Addr<A>, functor: T)
// where
//     A: Actor,
//     T: FnOnce()

async fn send_signal(addr: &AnonymousAddr, signal: ActorSignal) -> anyhow::Result<()> {
    let local = &access_shard_ctx_ref().actors;
    if local.shard == addr.shard {
        unsafe { local.signal_actor_unchecked(addr, signal) }
    } else {
        // drop(local);
        let addr = *addr;
        Ok(monolib::call_on(addr.shard, async move || {
            let remote = &access_shard_ctx_ref().actors;
            unsafe { remote.signal_actor_unchecked(&addr, signal) }
        })
        .await
        .map_err(|_| anyhow!("promise fail"))??)
    }
}

#[inline]
async fn await_death_on_thread(addr: &AnonymousAddr, tam: &ThreadActorManager) {
    while tam.is_managed(addr) {
        tam.unmanage_bus.wait().await;
    }
}

async fn await_death(addr: &AnonymousAddr) -> anyhow::Result<(), PromiseError> {
    let local = &access_shard_ctx_ref().actors;
    if local.shard == addr.shard {
        await_death_on_thread(addr, local).await;
    } else {
        let addr = *addr;
        call_on(addr.shard, async move || {
            await_death_on_thread(&addr, &access_shard_ctx_ref().actors).await;
        })
        .await?;
    }
    Ok(())
}

async fn send_supervision_message(
    addr: &AnonymousAddr,
    signal: SupervisorMessage,
) -> anyhow::Result<()> {
    let local = &access_shard_ctx_ref().actors;
    if local.shard == addr.shard {
        Ok(unsafe {
            local
                .supervsn_msg_unchecked(addr, signal)
                .map_err(|_| anyhow!("Failed to send"))?
        })
    } else {
        // drop(local);
        let addr = *addr;
        Ok(monolib::call_on(addr.shard, async move || {
            let remote = &access_shard_ctx_ref().actors;
            unsafe { remote.supervsn_msg_unchecked(&addr, signal) }
        })
        .await
        .map_err(|_| anyhow!("promise fail"))??)
    }
}

pub(crate) async fn use_actor_address<A, T, F, R>(
    addr: &Addr<A>,
    functor: T,
) -> Result<R, ActorManagementError>
where
    A: Actor,
    T: FnOnce(LocalAddr<A>) -> F + Send + 'static,
    F: Future<Output = R>,
    R: Send + 'static,
{
    let local = &access_shard_ctx_ref().actors;
    if addr.anonymous.shard == local.shard {
        let actual = unsafe { local.translate_without_shard_check(addr)? };
        // drop(local);

        Ok(functor(actual).await)
    } else {
        // drop(local);
        // let &Addr { shard, local_index, _marker } = addr;

        let addr = *addr;
        let e: Result<Result<R, _>, PromiseError> =
            monolib::call_on(addr.anonymous.shard, async move || {
                let remote = &access_shard_ctx_ref().actors;
                if addr.anonymous.shard == remote.shard {
                    let actual = unsafe { remote.translate_without_shard_check(&addr)? };
                    // drop(remote);
                    Ok::<_, ActorManagementError>(functor(actual).await)
                } else {
                    panic!("very bad");
                }

                // Err(PromiseError::PromiseClosed)

                // funtor
            })
            .await;

        Ok(e??)
    }
}

impl AnonymousAddr {
    pub async fn stop(&self) -> anyhow::Result<()> {
        send_signal(self, ActorSignal::Stop).await
    }
    pub async fn kill(&self) -> anyhow::Result<()> {
        send_signal(self, ActorSignal::Kill).await
    }
    pub(crate) async fn send_supervision_message(
        &self,
        message: SupervisorMessage,
    ) -> anyhow::Result<()> {
        send_supervision_message(self, message).await
    }
    pub async fn await_death(&self) -> Result<(), PromiseError> {
        await_death(self).await
    }
    // pub(crate) async fn supervision(&self)
    // pub async fn
}

impl<A> Addr<A>
where
    A: Actor,
{
    pub async fn send(&self, message: A::Message) -> anyhow::Result<()>
    where
        A::Message: Send,
    {
        use_actor_address(self, async move |local| local.send(message).await)
            .await
            .map_err(|_| anyhow!("Failed tos end."))?
            .map_err(|_| anyhow!("Faield to send"))?;
        Ok(())
    }
    pub async fn stop(&self) -> anyhow::Result<()> {
        // use_actor_address(self, async move |local| local.stop())
        //     .await
        //     .map_err(|_| anyhow!("Failed tos end."))??;
        self.anonymous.stop().await?;
        Ok(())
    }
    pub async fn kill(&self) -> anyhow::Result<()> {
        self.anonymous.kill().await?;
        Ok(())
    }
    pub fn upgrade(self) -> Result<LocalAddr<A>, Addr<A>> {
        match access_shard_ctx_ref()
            .actors
            // .borrow()
            .translate_local(&self)
        {
            Ok(addr) => Ok(addr),
            Err(_) => Err(self),
        }
    }
    pub fn downgrade(self) -> AnonymousAddr {
        self.anonymous
    }
}

impl<A> Addr<A>
where
    A: Actor,
{
    pub async fn call<P>(&self, param: P) -> Result<<A as ActorCall<P>>::Output, anyhow::Error>
    where
        A: ActorCall<P>,
        P: Send + 'static,
        <A as ActorCall<P>>::Output: Send + 'static,
    {
        use_actor_address(self, async move |local| local.call(param).await)
            .await
            .map_err(|_| anyhow!("Send failure."))?
    }
}

// pub(crate) async fn signal_address<A>(addr: &Addr<A>, signal: ActorSignal) -> Result<(), ActorManagementError>
// where
//     A: Actor
// {
//     let mut local = access_shard_ctx_ref().actors.borrow_mut();
//     if addr.shard == local.shard {
//         let actual = unsafe { local.translate_without_shard_check(addr)? };

//         // let promise = actual.handle.

//     } else {

//     }

//     Ok(())
// }

impl ThreadActorManager {
    pub fn new(core: ShardId) -> Self {
        Self {
            shard: core,
            map: SlotMap::with_key().into(),
            registry: RefCell::new(TwoWayTranslator {
                backward: HashMap::new(),
                forward: HashMap::new(),
            }),
            unmanage_bus: LocalEvent::new(),
        }
    }
    // TODO: Deal with registries happening at the same time.
    pub async fn register<A>(
        &self,
        name: &'static str,
        address: LocalAddr<A>,
    ) -> Result<(), PromiseError>
    where
        A: Actor,
    {
        let typ = TypeId::of::<A>();
        self.registry
            .borrow_mut()
            .insert(name, (address.clone().downgrade().downgrade(), typ));
        let topology = monolib::get_topology_info();
        let mut inflight = FuturesUnordered::new();
        for core in 0..topology.cores {
            let addr = address.clone().downgrade().downgrade();
            inflight.push(monolib::call_on(ShardId::new(core), async move || {
                access_shard_ctx_ref()
                    .actors
                    .insert_registry::<A>(name, addr);
            }));
        }
        let all = inflight
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        Ok(())
    }
    fn insert_registry<A>(&self, name: &'static str, address: AnonymousAddr)
    where
        A: Actor,
    {
        self.registry
            .borrow_mut()
            .insert(name, (address, TypeId::of::<A>()));
    }
    pub fn translate_local<A>(&self, addr: &Addr<A>) -> Result<LocalAddr<A>, ActorManagementError>
    where
        A: Actor,
    {
        if self.shard != addr.anonymous.shard {
            return Err(ActorManagementError::NotLocal);
        } else {
            unsafe { self.translate_without_shard_check(addr) }
        }
    }

    #[inline]
    pub(crate) fn is_managed(&self, address: &AnonymousAddr) -> bool {
        if self.shard != address.shard {
            false
        } else {
            self.map.borrow().contains_key(address.local_index)
        }
    }

    pub(crate) fn unmanage(&self, address: &AnonymousAddr) {
        if self.shard != address.shard {
            return;
        }
        self.map.borrow_mut().remove(address.local_index);
        self.unmanage_bus.set_all(|| {});
    }

    pub unsafe fn translate_without_shard_check<A>(
        &self,
        addr: &Addr<A>,
    ) -> Result<LocalAddr<A>, ActorManagementError>
    where
        A: Actor,
    {
        match self.map.borrow().get(addr.anonymous.local_index) {
            None => Err(ActorManagementError::ActorNoLongerExists),
            Some(actor) => {
                let act_ref = unsafe {
                    let raw = actor.vtable.payload.cast::<RawHandle<A>>();
                    Rc::increment_strong_count(raw);
                    LocalAddr::from_raw(raw, addr.anonymous)
                };
                // let actref = unsafe { &*actor.vtable.payload.cast::<RawHandle<A>>() };
                Ok(act_ref)
            }
        }
    }

    pub unsafe fn signal_actor_unchecked(
        &self,
        addr: &AnonymousAddr,
        signal: ActorSignal,
    ) -> anyhow::Result<()> {
        let ke = self.map.borrow();
        let table = &ke
            .get(addr.local_index)
            .ok_or_else(|| anyhow!("No actor with that addresss."))?
            .vtable;
        (table.signal)(table.payload, signal)
    }

    pub unsafe fn supervsn_msg_unchecked(
        &self,
        addr: &AnonymousAddr,
        msg: SupervisorMessage,
    ) -> anyhow::Result<(), anyhow::Error> {
        let ke = self.map.borrow();
        let table = &ke.get(addr.local_index).ok_or_else(|| anyhow!("Good"))?;
        (table.vtable.supervision)(table.vtable.payload, msg).map_err(|_| anyhow!("send fail"))
    }

    #[inline]
    pub fn spawn_actor<A>(
        &'static self,
        executor: &'static Executor<'static>,
        arguments: A::Arguments,
    ) -> Addr<A>
    where
        A: Actor,
    {
        self.spawn_actor_internal(executor, arguments, None)
    }

    pub fn spawn_actor_with_parent<A>(
        &'static self,
        executor: &'static Executor<'static>,
        arguments: A::Arguments,
        parent: AnonymousAddr,
    ) -> Addr<A>
    where
        A: Actor,
    {
        self.spawn_actor_internal(executor, arguments, Some(parent))
    }

    fn spawn_actor_internal<A>(
        &'static self,
        executor: &'static Executor<'static>,
        arguments: A::Arguments,
        parent: Option<AnonymousAddr>,
    ) -> Addr<A>
    where
        A: Actor,
    {
        let address = self.map.borrow_mut().insert_with_key(|key| {
            let address = AnonymousAddr {
                local_index: key,
                shard: self.shard,
            };
            let (local, task) =
                super::base::spawn_actor_full::<A>(address, self, executor, arguments, parent);
            task.detach();

            ActorSupportingCtx {
                vtable: build_addr_vtable(local.to_raw_ptr()),
            }
        });

        Addr {
            anonymous: AnonymousAddr {
                shard: self.shard,
                local_index: address,
            },
            _marker: PhantomData,
        }
    }
}

new_key_type! { pub(crate) struct ActorIndex; }

#[cfg(test)]
mod tests {
    use std::{sync::LazyLock, time::Duration};

    use asyncnal::{Event, EventSetter};
    use smol::future::Race;

    use crate::{
        core::{
            actor::base::{Actor, ActorCall},
            executor::helper::{select2, Select2Result},
            shard::{
                shard::{access_shard_ctx_ref, signal_monorail},
                state::ShardId,
            },
            topology::{MonorailConfiguration, MonorailTopology},
        },
        monolib::{self, shard_id},
    };

    #[test]
    pub fn actor_unmanages() {
        struct BasicActor;

        static EVENT: LazyLock<Event> = LazyLock::new(|| Event::new());

        impl Actor for BasicActor {
            type Arguments = ();
            type Message = ();
            type State = ();

            async fn pre_start(arguments: Self::Arguments) -> Self::State {
                arguments
            }

            async fn handle(
                this: crate::core::actor::base::SelfAddr<'_, Self>,
                message: Self::Message,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                Ok(())
            }

            async fn post_stop(
                this: crate::core::actor::base::SelfAddr<'_, Self>,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                EVENT.set_one();
                Ok(())
            }
        }

        MonorailTopology::normal(async || {
            let actor = monolib::spawn_actor::<BasicActor>(());

            assert!(access_shard_ctx_ref()
                .actors
                .is_managed(&actor.clone().downgrade().downgrade()));

            actor.stop();

            EVENT.wait().await;

            assert!(
                !access_shard_ctx_ref()
                    .actors
                    .is_managed(&actor.clone().downgrade().downgrade()),
                "The state was not actually unmanaged."
            );

            signal_monorail(Ok(()));
        })
        .unwrap();
    }

    #[test]
    pub fn actor_wait_unmanage() {
        struct BasicActor;

        static EVENT: LazyLock<Event> = LazyLock::new(|| Event::new());

        impl Actor for BasicActor {
            type Arguments = ();
            type Message = ();
            type State = ();

            async fn pre_start(arguments: Self::Arguments) -> Self::State {
                arguments
            }

            async fn handle(
                this: crate::core::actor::base::SelfAddr<'_, Self>,
                message: Self::Message,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                Ok(())
            }

            async fn post_stop(
                this: crate::core::actor::base::SelfAddr<'_, Self>,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                monolib::sleep(Duration::from_millis(10)).await;
                EVENT.set_one();
                Ok(())
            }
        }

        MonorailTopology::normal(async || {
            let actor = monolib::spawn_actor::<BasicActor>(());

            assert!(access_shard_ctx_ref()
                .actors
                .is_managed(&actor.clone().downgrade().downgrade()));

            actor.stop();

            match select2(EVENT.wait(), async {
                actor.await_death().await.unwrap();
            })
            .await
            {
                Select2Result::Left(a) => panic!("Event was notified first."),
                Select2Result::Right(b) => {}
            }

            // smol::future::race(EVENT.wait(), async { actor.await_death().await.unwrap();  }).await;

            // EVENT.wait().await;

            assert!(
                !access_shard_ctx_ref()
                    .actors
                    .is_managed(&actor.clone().downgrade().downgrade()),
                "The state was not actually unmanaged."
            );

            signal_monorail(Ok(()));
        })
        .unwrap();
    }

    #[test]
    pub fn test_propagation() {
        struct BasicActor;

        impl Actor for BasicActor {
            type Arguments = ();
            type Message = ();
            type State = ();

            async fn pre_start(_arguments: Self::Arguments) -> Self::State {
                ()
            }

            async fn handle(
                _this: crate::core::actor::base::SelfAddr<'_, Self>,
                _message: Self::Message,
                _state: &mut Self::State,
            ) -> anyhow::Result<()> {
                Ok(())
            }
        }

        impl ActorCall<()> for BasicActor {
            type Output = ShardId;

            async fn call<'a>(
                _this: crate::core::actor::base::SelfAddr<'a, Self>,
                _msg: (),
                _state: &'a mut Self::State,
            ) -> Self::Output {
                shard_id()
            }
        }

        async fn check_functioning(shard: ShardId) {
            monolib::call_on(shard, async move || {
                println!("Running for {shard:?}");
                let resolved = monolib::actors::resolve::<BasicActor>("main.manager").unwrap();
                assert_eq!(resolved.call(()).await.unwrap(), ShardId::new(0));
            })
            .await
            .unwrap();
        }

        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(25)
                .build(),
            async || {
                let actor = monolib::spawn_actor::<BasicActor>(());

                monolib::actors::register("main.manager", actor)
                    .await
                    .unwrap();

                for i in 0..monolib::get_topology_info().cores {
                    check_functioning(ShardId::new(i)).await;
                }

                let result = monolib::call_on_all(async move || {
                    let actor = monolib::actors::resolve::<BasicActor>("main.manager")?;
                    let reso = actor.call(()).await?;

                    Ok::<_, anyhow::Error>(reso)
                }).await.into_iter().collect::<Result<Vec<_>, _>>().unwrap().into_iter().collect::<Result<Vec<_>, _>>().unwrap();

                assert!(result.iter().all(|f| f.as_usize() == 0));


                signal_monorail(Ok(()));
            },
        )
        .unwrap();
    }
}
