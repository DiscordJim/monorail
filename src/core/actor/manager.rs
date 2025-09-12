use std::{cell::RefCell, future::Future, marker::PhantomData, mem::ManuallyDrop, ops::Add, rc::Rc};

use anyhow::anyhow;
use slotmap::{new_key_type, SlotMap};

use crate::{
    core::{
        actor::base::{Actor, ActorCall, ActorSignal, LocalAddr, RawHandle},
        channels::promise::PromiseError,
        executor::scheduler::Executor,
        shard::{shard::access_shard_ctx_ref, state::ShardId},
    },
    monolib::{self},
};

struct ActorAddrVTable {
    payload: *const (),
    signal: fn(*const (), ActorSignal) -> anyhow::Result<()>,
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
            let m = ManuallyDrop::new(LocalAddr::from_raw(payload.cast::<RawHandle<A>>()));

            match signal {
                ActorSignal::Kill => m.kill(),
                ActorSignal::Stop => m.stop()
            }

            Ok(())

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

// impl<T> 

#[derive(Clone, Copy)]
pub struct AnonymousAddr {
    shard: ShardId,
    local_index: ActorIndex
}



impl<T: Actor> Copy for Addr<T> {}
impl<T: Actor> Clone for Addr<T> {
    fn clone(&self) -> Self {
        Self {
            anonymous: self.anonymous.clone(),
            _marker: PhantomData
            // local_index: self.local_index,
        }
    }
}

pub struct ThreadActorManager {
    shard: ShardId,
    map: RefCell<SlotMap<ActorIndex, ActorSupportingCtx>>
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
        }).await.map_err(|_| anyhow!("promise fail"))??)
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
        let e: Result<Result<R, _>, PromiseError> = monolib::call_on(addr.anonymous.shard, async move || {
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
            .map_err(|_| anyhow!("Failed tos end."))?.map_err(|_| anyhow!("Faield to send"))?;
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
        use_actor_address(self, async move |local| local.call(param).await).await.map_err(|_| anyhow!("Send failure."))?
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
        }
    }
    pub fn translate_local<A>(
        &self,
        addr: &Addr<A>,
    ) -> Result<LocalAddr<A>, ActorManagementError>
    where
        A: Actor,
    {
        if self.shard != addr.anonymous.shard {
            return Err(ActorManagementError::NotLocal);
        } else {
            unsafe { self.translate_without_shard_check(addr) }
        }
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
                    LocalAddr::from_raw(raw)
                };
                // let actref = unsafe { &*actor.vtable.payload.cast::<RawHandle<A>>() };
                Ok(act_ref)
            }
        }
    }

    pub unsafe fn signal_actor_unchecked(&self, addr: &AnonymousAddr, signal: ActorSignal) -> anyhow::Result<()> {
        let ke = self.map.borrow();
        let table = &ke.get(addr.local_index).ok_or_else(|| anyhow!("No actor with that addresss."))?.vtable;
        (table.signal)(table.payload, signal)
    }

    pub fn spawn_actor<A>(
        &self,
        executor: &'static Executor<'static>,
        arguments: A::Arguments,
    ) -> anyhow::Result<Addr<A>>
    where
        A: Actor,
    {
        let (local, task) = super::base::spawn_actor::<A>(executor, arguments)?;
        task.detach();

        let local = local.to_raw_ptr();
        let index = self.map.borrow_mut().insert(ActorSupportingCtx {
            vtable: build_addr_vtable(local),
        });

        Ok(Addr {
            anonymous: AnonymousAddr { shard: self.shard, local_index: index },
            _marker: PhantomData,
        })
    }
}

new_key_type! { pub(crate) struct ActorIndex; }
