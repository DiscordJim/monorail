use std::marker::PhantomData;

use anyhow::Result;
use futures::channel::oneshot::{self, Sender};
use slab::Slab;

use crate::core::{actor::{base::{Actor, FornAddr}, routing::{Router, RouterArguments, RouterSpawnPolicy, RoutingPolicy}}, shard::shard::spawn_actor};


pub struct ShardedSlab<V>
where 
    // K: Send + Eq + Hash + 'static,
    V: Send + Clone + 'static
{
    addr: FornAddr<Router<ShardedSlabActor<V>>>
}

impl<V> ShardedSlab<V>
where
    V: Send + Clone + 'static
{
    pub fn new() -> Result<Self> {
        let (foreign, _) = spawn_actor::<Router<ShardedSlabActor<V>>>(RouterArguments {
            arguments: 0,
            routing_policy: RoutingPolicy::RoutingFn(Box::new(|request, targets, cursor| {

                let target = match request {
                    HashMapRequest::Insert(_, _) => {
                        let prev = *cursor;
                        *cursor = (*cursor + 1) % targets;
                        prev
                    }
                    HashMapRequest::Get(shard, _) => shard.route,
                    HashMapRequest::Remove(shard, _) => shard.route
                };

                target


            })),
            spawn_policy: RouterSpawnPolicy::PerCore,
            transformer: |_, b| b
        })?;

        Ok(ShardedSlab {
            addr: foreign
        })
    }
    pub async fn get(&self, key: SlabIndex) -> Result<Option<V>> {
        let (tx, rx) = oneshot::channel();
        self.addr.send(HashMapRequest::Get(key, tx)).await?;
        Ok(rx.await?)
    }
    pub async fn insert(&self, value: V) -> Result<SlabIndex> {
        let (tx, rx) = oneshot::channel();
        self.addr.send(HashMapRequest::Insert(value, tx)).await?;
        Ok(rx.await?)
    }
    pub async fn remove(&self, key: SlabIndex) -> Result<Option<V>> {
        let (tx, rx) = oneshot::channel();
        self.addr.send(HashMapRequest::Remove(key, tx)).await?;
        Ok(rx.await?)
    }
}

pub struct ShardedSlabActor<V>(PhantomData<V>);


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlabIndex {
    local: usize,
    route: usize
}

pub enum HashMapRequest<V> {
    Get(SlabIndex, Sender<Option<V>>),
    Insert(V, Sender<SlabIndex>),
    Remove(SlabIndex, Sender<Option<V>>)
}

impl<V> Actor for ShardedSlabActor<V>
where 
    V: Clone + 'static
{
    type Arguments = usize;
    type Message = HashMapRequest<V>;
    type State = (usize, Slab<V>);
    fn name() -> &'static str {
        ""
    }
    fn pre_start(arguments: Self::Arguments) -> anyhow::Result<Self::State> {
        println!("Received init number: {arguments}");
        Ok((arguments, Slab::new()))
    }
    async fn handle(
            _: crate::core::actor::base::SelfAddr<'_, Self>,
            message: Self::Message,
            (route_num, map): &mut Self::State,
        ) -> anyhow::Result<()> {
        match message {
            HashMapRequest::Get(k, res) => {
                let _ = res.send(map.get(k.local).cloned());
            }
            HashMapRequest::Insert(k, tx) => {
                // state.insert(k, v);
                let local = map.insert(k);
                let _ = tx.send(SlabIndex {
                    local,
                    route: *route_num
                });
            }
            HashMapRequest::Remove(k, tx) => {
                // state.remove(&k);
                let result = map.try_remove(k.local);
                let _ = tx.send(result);
            }
        }
        Ok(())
    }
}




#[cfg(test)]
mod tests {
    use crate::core::{actor::lib::slab::ShardedSlab, shard::{shard::{signal_monorail, submit_task_to}, state::ShardId}, topology::{MonorailConfiguration, MonorailTopology}};


    #[test]
    pub fn test_distributed_slab() {

        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(6)
                .build(),
            |_| {
                submit_task_to(ShardId::new(0), async move || {
                    let map = ShardedSlab::<&'static str>::new().unwrap();

                    let what = map.insert("what").await.unwrap();
                    assert_eq!(map.get(what).await.unwrap(), Some("what"));

                    let what = map.insert("hello").await.unwrap();
                    assert_eq!(map.get(what).await.unwrap(), Some("hello"));

                    let what = map.insert("cool").await.unwrap();
                    assert_eq!(map.get(what).await.unwrap(), Some("cool"));

                    signal_monorail(Ok(()));
                });
            }
        ).unwrap();
    }
}