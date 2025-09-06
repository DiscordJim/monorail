use std::marker::PhantomData;

use anyhow::anyhow;

use crate::core::{
    actor::base::{Actor, ActorCall, Addr, FornAddr},
    shard::{shard::get_topology_info, state::ShardId},
};

pub struct Router<A>
where
    A: Actor,
{
    // routing_table: Vec<>
    _marker: PhantomData<A>,
}


pub trait CallRoutePolicy<M>: Actor

{
    fn route(message: &M, targets: usize, cursor: &mut usize) -> usize;
}

// pub trait SmartRoutingPolicy {
//     // fnb 
// }

struct LocalRouterCtx<A>
where
    A: Actor,
{
    addr: FornAddr<A>,
    local: usize,
}

pub struct RouterState<A>
where
    A: Actor,
    Router<A>: Actor
{
    targets: Vec<LocalRouterCtx<A>>,
    last_shot: usize,
    arguments: RouterArguments<A>,
    _marker: PhantomData<A>,
}

pub enum RoutingPolicy<A>
where 
    A: Actor
{
    First,
    RoundRobin,
    RoutingFn(Box<dyn Fn(&A::Message, usize, &mut usize) -> usize>)
}

pub enum RouterSpawnPolicy {
    PerCore,
}

pub struct RouterArguments<A>
where
    A: Actor,
    Router<A>: Actor
{
    pub arguments: A::Arguments,
    pub spawn_policy: RouterSpawnPolicy,
    pub routing_policy: RoutingPolicy<A>,
    pub transformer: fn(&<Router<A> as Actor>::Arguments, usize) -> A::Arguments
}

pub struct RoutedMessage<A>
where
    A: Actor,
{
    pub route: usize,
    pub message: A::Message,
}

#[derive(Debug)]
pub struct RouterResponse<M> {
    pub responder: usize,
    pub message: M,
}



impl<A, M> ActorCall<M> for Router<A>
where
    A: Actor,
    A::Arguments: Clone + Send + 'static,
    A::Message: Send + 'static,
    A: ActorCall<M>,
    <A as ActorCall<M>>::Output: Send + 'static,
    M: Send + 'static, // M
    A: CallRoutePolicy<M>
{
    type Output = Result<RouterResponse<<A as ActorCall<M>>::Output>, anyhow::Error>;
    async fn call<'a>(
        this: super::base::SelfAddr<'a, Self>,
        msg: M,
        state: &'a mut Self::State,
    ) -> Self::Output {
        if state.targets.is_empty() {
            return Err(anyhow!("No actors to route to."));
        }

     

        let policy = <A as CallRoutePolicy<M>>::route(&msg, state.targets.len(), &mut state.last_shot);
        if policy >= state.targets.len() {
            return Err(anyhow!("Specified policy routed the message out of bounds."));
        }
        // println!("Targeting -> {target}");
        let reso = state.targets[policy]
            .addr
            .call(msg)
            .await
            .map(|f| RouterResponse {
                message: f,
                responder: policy,
            });

        reso

        // state.targets[0].addr.call(msg).await.map
    }
}

impl<A> Actor for Router<A>
where
    A: Actor,
    A::Arguments: Clone + Send + 'static,
    A::Message: Send + 'static,
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

        let target = match &state.arguments.routing_policy {
            RoutingPolicy::First => 0,
            RoutingPolicy::RoundRobin => {
                let prev = state.last_shot;
                state.last_shot = (state.last_shot + 1) % state.targets.len();
                prev
            }
            RoutingPolicy::RoutingFn(functor) => {
                let decision = functor(&message, state.targets.len(), &mut state.last_shot);
                if decision >= state.targets.len() {
                    return Err(anyhow!("Routing function pointed to {decision}, which is out of bounds."));
                }
                decision
            }
        };

        if target >= state.targets.len() {
            return Err(anyhow!("Target specified by routing policy is out of range."));
        }

        state.targets[target].addr.send(message).await?;


        Ok(())
    }
    fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
        Ok(RouterState {
            arguments: arguments,
            targets: vec![],
            last_shot: 0,
            _marker: PhantomData,
        })
    }
    async fn post_start(
        mut this: super::base::SelfAddr<'_, Self>,
        state: &mut Self::State,
    ) -> anyhow::Result<()> {
        for i in 0..get_topology_info().cores {
            // println!("Starting actor on {i}");
            let args_copy = state.arguments.arguments.clone();
            let forn = this
                .spawn_linked_foreign::<A>(ShardId::new(i), (state.arguments.transformer)(&state.arguments, i))
                .await?;

                // println!("Forn: {:?}", forn.signal);

            state.targets.push(LocalRouterCtx {
                addr: forn,
                local: i,
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
    use futures::channel::oneshot;

    use crate::core::{
        actor::{
            base::{Actor, ActorCall},
            routing::{CallRoutePolicy, Router, RouterArguments, RouterSpawnPolicy, RoutingPolicy},
        },
        shard::{shard::{shard_id, signal_monorail, spawn_actor, spawn_async_task, submit_task_to}, state::ShardId},
        topology::{MonorailConfiguration, MonorailTopology},
    };

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

        impl ActorCall<usize> for BasicAdder {
            type Output = usize;
            async fn call<'a>(
                this: crate::core::actor::base::SelfAddr<'a, Self>,
                msg: usize,
                state: &'a mut Self::State,
            ) -> Self::Output {
                msg + 1
            }
        }

        impl CallRoutePolicy<usize> for BasicAdder {
            fn route(message: &usize, targets: usize, cursor: &mut usize) -> usize {
                let og = *cursor;
                *cursor += 1;
                og
            }
        }

        // impl Actor

        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(6)
                .build(),
            |init| {
                spawn_async_task(async move {
                    let (forn, local) = spawn_actor::<Router<BasicAdder>>(RouterArguments {
                        arguments: (),
                        spawn_policy: RouterSpawnPolicy::PerCore,
                        routing_policy: RoutingPolicy::RoundRobin,
                        transformer: |a, b| a.arguments
                    })
                    .unwrap();

                    let result = forn
                        .call(5)
                        .await
                        .unwrap()
                        .expect("Failed to process the routed message.");
                    assert_eq!(result.responder, 0);
                    assert_eq!(result.message, 6);


                    let result = forn
                        .call(5)
                        .await
                        .unwrap()
                        .expect("Failed to process the routed message.");
                    assert_eq!(result.responder, 1);
                    assert_eq!(result.message, 6);

                    signal_monorail(Ok(()));


                    // println!("Result: {:?}", result);
                })
                .detach();
            },
        )
        .unwrap();
    }


     #[test]
    pub fn test_router_with_routing_fn() {
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

        impl ActorCall<usize> for BasicAdder {
            type Output = usize;
            async fn call<'a>(
                this: crate::core::actor::base::SelfAddr<'a, Self>,
                msg: usize,
                state: &'a mut Self::State,
            ) -> Self::Output {
                msg + 1
            }
        }

        impl CallRoutePolicy<usize> for BasicAdder {
            fn route(message: &usize, targets: usize, cursor: &mut usize) -> usize {
                *message
            }
        }

        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(6)
                .build(),
            |init| {
                submit_task_to(ShardId::new(0), || async move {
                    let (forn, local) = spawn_actor::<Router<BasicAdder>>(RouterArguments {
                        arguments: (),
                        spawn_policy: RouterSpawnPolicy::PerCore,
                        routing_policy: RoutingPolicy::RoundRobin,
                        transformer: |a, _| a.arguments
                    })
                    .unwrap();

                    let result = forn
                        .call(5)
                        .await
                        .unwrap()
                        .expect("Failed to process the routed message.");
                    assert_eq!(result.responder, 5);
                    assert_eq!(result.message, 6);


                    let result = forn
                        .call(4)
                        .await
                        .unwrap()
                        .expect("Failed to process the routed message.");
                    assert_eq!(result.responder, 4);
                    assert_eq!(result.message, 5);

                    signal_monorail(Ok(()));


                    // println!("Result: {:?}", result);
                });
                // .detach();
            },
        )
        .unwrap();
    }
    

    #[test]
    pub fn test_message_routing_function() {
        pub struct BasicAdder;

        impl Actor for BasicAdder {
            type Arguments = ();
            type Message = (usize, oneshot::Sender<(ShardId, usize)>);
            type State = ();
            async fn handle(
                this: crate::core::actor::base::SelfAddr<'_, Self>,
                (src, returner): Self::Message,
                state: &mut Self::State,
            ) -> anyhow::Result<()> {
                
                let _ = returner.send((shard_id(), src + 1));

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
                submit_task_to(ShardId::new(0), || async move {
                    let (forn, local) = spawn_actor::<Router<BasicAdder>>(RouterArguments {
                        arguments: (),
                        spawn_policy: RouterSpawnPolicy::PerCore,
                        routing_policy: RoutingPolicy::RoutingFn(Box::new(|msg: &(usize, oneshot::Sender<(ShardId, usize)>), targets, _| {
                            msg.0
                        })),
                        transformer: |a, _| a.arguments
                    })
                    .unwrap();

                    let (tx, rx) = oneshot::channel();
                    local.send((3, tx)).await.unwrap();
                    let (shard, result) = rx.await.unwrap();
                    assert_eq!(shard.as_usize(), 3);
                    assert_eq!(result, 4);

                    let (tx, rx) = oneshot::channel();
                    local.send((4, tx)).await.unwrap();
                    let (shard, result) = rx.await.unwrap();
                    assert_eq!(shard.as_usize(), 4);
                    assert_eq!(result, 5);

                 
                    signal_monorail(Ok(()));


                    // println!("Result: {:?}", result);
                });
                // .detach();
            },
        )
        .unwrap();


    }
}
