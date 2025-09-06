use std::{
    any::Any,
    collections::HashMap,
    marker::PhantomData,
};

use anyhow::{anyhow, Ok};

use crate::core::{
    actor::base::{spawn_actor, Actor, ActorSignal, Addr, FornAddr, FornSignalHandle, SignalHandle},
    executor::scheduler::Executor,
    shard::state::ShardId,
};

/// Issues mail addresses so it can
/// direct messages around
pub(crate) struct ShardActorOffice {
    reverse: HashMap<MailId, Box<dyn Any>>,
    signal_chart: HashMap<MailId, SignalHandle>,
    id_count: usize,
    // free_list: Vec<usize>
}

impl ShardActorOffice {
    pub fn new() -> Self {
        Self {
            reverse: HashMap::new(),
            signal_chart: HashMap::new(),
            id_count: 0,
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
        let id = self.id_count;
        self.id_count += 1;

        self.reverse.insert(MailId(id), Box::new(addr.clone()));
        self.signal_chart.insert(MailId(id), addr.clone().downgrade());

        // self.reverse.insert(TypeId::<, v)

        // println!("CoRE: {:?}", core);

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
    pub fn lookup_address<A>(&self, mail_id: MailId) -> Option<&Addr<A>>
    where
        A: Actor,
    {
        self.reverse
            .get(&mail_id)
            .map(|f| f.downcast_ref())
            .flatten()
    }
    #[inline]
    pub fn signal_address(&self, mail_id: MailId, signal: ActorSignal) -> anyhow::Result<()> {

        let signalhandler = self.signal_chart.get(&mail_id).ok_or_else(|| anyhow!("Failed to find address."))?;
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
        actor::base::{Actor, ActorCall, SelfAddr}, shard::{shard::{signal_monorail, spawn_actor, submit_task_to}, state::ShardId}, topology::{MonorailConfiguration, MonorailTopology}
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
            |_| {
                let (foreign, _) = spawn_actor::<BasicActor>(()).expect("Failed to create basic actor.");
                submit_task_to(ShardId::new(1), async move || {
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
