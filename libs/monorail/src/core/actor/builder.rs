use anyhow::anyhow;

use crate::{core::{actor::{base::{Actor, LocalAddr}, manager::Addr}, shard::state::ShardId}, monolib::{self, actors}};



pub struct ActorBuilder<A: Actor> {
    args: Option<A::Arguments>,
    registration_name: Option<&'static str>,
}

impl<A: Actor> ActorBuilder<A> {
    pub fn new() -> Self {
        Self {
            args: None,
            registration_name: None,
        }
    }
    
    pub fn with_args(mut self, args: A::Arguments) -> Self {
        self.args = Some(args);
        self
    }
    
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.registration_name = Some(name);
        self
    }
    
    // TODO: make Arguments Send?
    pub async fn spawn_on(self, shard: ShardId) -> anyhow::Result<Addr<A>> 
        where A::Arguments: Send
    {
        // Validate required fields
        let args = self.args.ok_or_else(|| anyhow::anyhow!("Arguments are required to spawn an Actor"))?;

        let handle = monolib::call_on(shard, move || async move {
            let actor = monolib::spawn_actor::<A>(args);
            if let Some(name) = self.registration_name {
                actors::register(name, actor.clone()).await.expect(&format!("Failed to register the Actor on shard {:?}", shard));
            }
            actor.downgrade()
        }).await;
        handle.map_err(|e| anyhow::anyhow!("Failed to spawn Actor on shard {:?}: {:?}", shard, e))
    }

    pub async fn spawn(self) -> anyhow::Result<LocalAddr<A>>
        where A::Arguments: Send
    {
        let args = self.args.ok_or_else(|| anyhow::anyhow!("Arguments are required to spawn an Actor"))?;

        let actor = monolib::spawn_actor::<A>(args);
        if let Some(name) = self.registration_name {
            actors::register(name, actor.clone()).await.expect(&format!("Failed to register the Actor on shard {:?}", monolib::shard_id()));
        }
        Ok(actor)
    }

}

#[cfg(test)]
mod tests {
    use std::future::Future;
use crate::core::topology::MonorailTopology;
use crate::core::topology::MonorailConfiguration;
use crate::core::shard::shard::signal_monorail;
    use crate::core::{actor::{base::Actor, builder::ActorBuilder}, shard::state::ShardId};

    struct MyActor;

    impl Actor for MyActor {
        type Arguments = i32;
    
        type Message = i32;
    
        type State = i32;
    
        fn pre_start(arguments: Self::Arguments) -> impl Future<Output = Self::State> {
            async move {
                arguments * 2 // Just an example of initializing state based on arguments
            }
        }
    }

    #[crate::test]
    async fn test() {
        let builder = ActorBuilder::<MyActor>::new()
            .with_args(42)
            .with_name("my_actor")
            .spawn_on(ShardId::new(2))
            .await.expect("");
    }

}