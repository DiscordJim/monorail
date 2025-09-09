use std::{
    // collections::HashMap,
    marker::PhantomData, rc::Rc,
};

use anyhow::{anyhow, Ok};
use slab::Slab;

use crate::core::{
    actor::base::{spawn_actor, Actor, ActorSignal, Addr, FornAddr, FornSignalHandle, RawHandle, SignalHandle},
    executor::scheduler::Executor,
    shard::state::ShardId,
};


struct AddrVTable {
    /// The actual data contained within the [Rc].
    payload: *const (),
    /// The function to decrement the reference count.
    /// We need this to actually properly perform drops
    /// on the table.
    _decref: fn(*const ()),
}

#[inline]
fn build_addr_vtable<A>(handle: &Addr<A>) -> AddrVTable
where 
    A: Actor
{
    AddrVTable {
        payload: handle.to_raw_ptr().cast(),
        _decref: |ptr| unsafe {
            Rc::from_raw(ptr.cast::<RawHandle<A>>());
        }
    }
}

impl Drop for AddrVTable {
    fn drop(&mut self) {
        (self._decref)(self.payload)
    }
}

struct ActorCtx {
    /// A type-erased pointer to an actor. This is actually
    /// a pointer to the [RawHandle] type which is parameterized
    /// with the actor type.
    address: AddrVTable,
    /// The signal handler for the address.
    signal_chart: SignalHandle
}

/// Issues mail addresses so it can
/// direct messages around
pub(crate) struct ShardActorOffice {
    map: Slab<ActorCtx>,
}

impl ShardActorOffice {
    pub fn new() -> Self {
        Self {
            map: Slab::new(),
            // id_count: 0,
        }
    }
    pub fn spawn_actor<A>(
        &mut self,
        core: ShardId,
        executor: &'static Executor<'static>,
        arguments: A::Arguments,
    ) -> anyhow::Result<(FornAddr<A>, Addr<A>)>
    where
        A: Actor + 'static
    {
        let (addr, task) = spawn_actor(executor, arguments)?;

        task.detach();

        let id = self.map.insert(ActorCtx {
            address: build_addr_vtable(&addr),
            signal_chart: addr.clone().get_signal()
        });

        Ok((
            FornAddr {
                signal: FornSignalHandle {
                    mail: MailId(id),
                    shard: core
                },
                _marker: PhantomData,
            },
            addr,
        ))
    }
    #[inline]
    fn lookup_entry(&self, mail_id: MailId) -> Option<&ActorCtx> {
        self.map.get(mail_id.0)
    }
    #[inline]
    pub fn lookup_address<A>(&self, mail_id: MailId) -> Option<Addr<A>>
    where
        A: Actor,
    {
        let address = &self.lookup_entry(mail_id)?.address;
        let raw_handle = address.payload.cast::<RawHandle<A>>();
        unsafe {
            // Bump the refcount.
            Rc::increment_strong_count(raw_handle);
            Some(Addr::from_raw(raw_handle))
        }
    }
    #[inline]
    pub fn signal_address(&self, mail_id: MailId, signal: ActorSignal) -> anyhow::Result<()> {

        let signalhandler = &self.lookup_entry(mail_id).ok_or_else(|| anyhow!("Failed to find entry."))?.signal_chart;
        match signal {
            ActorSignal::Kill => signalhandler.kill(),
            ActorSignal::Stop => signalhandler.stop()?
        }

        Ok(())
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MailId(usize);

#[cfg(test)]
mod tests {

    

    use futures::channel::oneshot;

    use crate::core::{
        actor::base::{Actor, ActorCall, SelfAddr}, shard::{shard::{signal_monorail, spawn_actor, submit_to}, state::ShardId}, topology::{MonorailConfiguration, MonorailTopology}
    };

    #[test]
    pub fn test_post_office_assignment() {
        struct BasicActor;

        impl Actor for BasicActor {
            type Arguments = ();
            type Message = oneshot::Sender<()>;
            type State = ();
            fn name() -> &'static str {
                ""
            }
            async fn handle(
                _: SelfAddr<'_, Self>,
                msg: Self::Message,
                _: &mut Self::State,
            ) -> anyhow::Result<()> {
                let _ = msg.send(());
                Ok(())
            }
            fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
                Ok(arguments)
            }
        }

        impl ActorCall<usize> for BasicActor {
            type Output = usize;
            async fn call<'a>(
                    _: SelfAddr<'a, Self>,
                    msg: usize,
                    _: &'a mut Self::State,
                ) -> Self::Output {
                msg + 1
            }
        }

        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(3)
                .build(),
            async || {
                let (foreign, _) = spawn_actor::<BasicActor>(()).expect("Failed to create basic actor.");
                submit_to(ShardId::new(1), async move || {
                    assert_eq!(foreign.call(1).await.expect("Failed to get result."), 2);
                    assert_eq!(foreign.call(2).await.expect("Failed to get result."), 3);

                    let (tx, rx) = oneshot::channel();
                    foreign.send(tx).await.expect("Failed to send.");
                    rx.await.expect("Failed to extract.");

                    signal_monorail(Ok(()));
                });
            },
        )
        .unwrap();
    }
}
