use std::marker::PhantomData;

use crate::core::{actor::base::{Actor, ActorCall, Addr, FornAddr}, shard::{shard::get_topology_info, state::ShardId}};


pub struct Router<A>
where 
    A: Actor
{
    // routing_table: Vec<>

    _marker: PhantomData<A>
}

struct LocalRouterCtx<A>
where 
    A: Actor
{
    addr: FornAddr<A>,
    local: usize
}

pub struct RouterState<A>
where 
    A: Actor
{
    targets: Vec<LocalRouterCtx<A>>,
    arguments: RouterArguments<A>,
    _marker: PhantomData<A>

}

pub enum RouterSpawnPolicy {
    PerCore
}

pub struct RouterArguments<A>
where 
    A: Actor
{
    arguments: A::Arguments,
    spawn_policy: RouterSpawnPolicy

}

pub struct RoutedMessage<A>
where 
    A: Actor
{
    pub route: usize,
    pub message: A::Message
}

impl<A, M> ActorCall<M> for Router<A>
where 
    A: Actor,
    A::Arguments: Clone + Send + 'static,
    A::Message: Send + 'static,
    A: ActorCall<M>,
    <A as ActorCall<M>>::Output: Send + 'static,
    M: Send + 'static
    // M
{
    type Output = Result<<A as ActorCall<M>>::Output, anyhow::Error>;
    async fn call<'a>(
            this: super::base::SelfAddr<'a, Self>,
            msg: M,
            state: &'a mut Self::State,
        ) -> Self::Output {
        state.targets[0].addr.call(msg).await
    }

}

impl<A> Actor for Router<A>
where 
    A: Actor,
    A::Arguments: Clone + Send + 'static,
    A::Message: Send + 'static
{
    type Arguments = RouterArguments<A>;
    type Message = A::Message;
    type State = RouterState<A>;
    fn name() -> &'static str {
        "Router"
    }
    async fn handle(
            this: super::base::SelfAddr<'_, Self>,
            message: Self::Message,
            state: &mut Self::State,
        ) -> anyhow::Result<()> {
        Ok(())
    }
    fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
        Ok(RouterState {
            arguments: arguments,
            targets: vec![],
            _marker: PhantomData
        })
    }
    async fn post_start(
            mut this: super::base::SelfAddr<'_, Self>,
            state: &mut Self::State,
        ) -> anyhow::Result<()> {


        for i in 0..get_topology_info().cores {
            let args_copy = state.arguments.arguments.clone();
            let forn = this.spawn_linked_foreign::<A>(ShardId::new(i), args_copy).await?;
            state.targets.push(LocalRouterCtx {
                addr: forn,
                local: i
            });
        }


        Ok(())
    }
}

// struct RoutingContext<A>
// where 
//     A: 
// {

// }




// fn routing_function


#[cfg(test)]
mod tests {
    use crate::core::{actor::{base::Actor, routing::{Router, RouterArguments, RouterSpawnPolicy}}, shard::shard::{spawn_actor, spawn_async_task}, topology::{MonorailConfiguration, MonorailTopology}};



    #[test]
    pub fn test_router() {
        pub struct BasicAdder;

        impl Actor for BasicAdder {
            type Arguments = ();
            type Message = ();
            type State = ();
            async fn handle(
                    this: crate::core::actor::base::SelfAddr<'_, Self>,
                    message: Self::Message,
                    state: &mut Self::State,
                ) -> anyhow::Result<()> {
                Ok(())
            }
            fn name() -> &'static str {
                ""
            }
            fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
                Ok(arguments)
            }
        }


        MonorailTopology::setup(

            MonorailConfiguration::builder()
                .with_core_override(6)
                .build(),
                |init| {
                    
                    spawn_async_task(async move {
                        let (forn, local) = spawn_actor::<Router<BasicAdder>>(RouterArguments {
                            arguments: (),
                            spawn_policy: RouterSpawnPolicy::PerCore
                        }).unwrap();
                    }).detach();


                }

        ).unwrap();
    }
}

