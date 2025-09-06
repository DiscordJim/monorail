use std::{collections::HashMap, hash::{DefaultHasher, Hash, Hasher}, marker::PhantomData};

use anyhow::Result;
use futures::channel::oneshot::{self, Sender};

use crate::core::{actor::{base::{Actor, FornAddr}, routing::{Router, RouterArguments, RouterSpawnPolicy, RoutingPolicy}}, shard::shard::spawn_actor};


pub struct ShardedHashMap<K, V>
where 
    K: Send + Eq + Hash + 'static,
    V: Send + Clone + 'static
{
    addr: FornAddr<Router<HashMapShardActor<K, V>>>
}

impl<K, V> ShardedHashMap<K, V>
where 
    K: Send + Eq + Hash + 'static,
    V: Send + Clone + 'static
{
    pub fn new() -> Result<Self> {
        let (foreign, _) = spawn_actor::<Router<HashMapShardActor<K, V>>>(RouterArguments {
            arguments: (),
            routing_policy: RoutingPolicy::RoutingFn(Box::new(|message, targets, _| {


                let key: &K = match &message {
                    HashMapRequest::Get(k, _) => k,
                    HashMapRequest::Insert(k, _, _) => k,
                    HashMapRequest::Remove(k, _) => k

                };

                let mut hasher = DefaultHasher::new();
                key.hash(&mut hasher);
                let reso = hasher.finish();

                (reso % targets as u64) as usize
            })),
            spawn_policy: RouterSpawnPolicy::PerCore,
            transformer: |a, _| a.arguments
        })?;

        Ok(ShardedHashMap {
            addr: foreign
        })
    }
    pub async fn get(&self, key: K) -> Result<Option<V>> {
        let (tx, rx) = oneshot::channel();
        self.addr.send(HashMapRequest::Get(key, tx)).await?;
        Ok(rx.await?)
    }
    pub async fn insert(&self, key: K, value: V) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.addr.send(HashMapRequest::Insert(key, value, tx)).await?;
        Ok(rx.await?)
    }
    pub async fn remove(&self, key: K) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.addr.send(HashMapRequest::Remove(key, tx)).await?;
        Ok(rx.await?)
    }
}

pub struct HashMapShardActor<K, V>(PhantomData<HashMap<K, V>>);


pub enum HashMapRequest<K, V> {
    Get(K, Sender<Option<V>>),
    Insert(K, V, Sender<()>),
    Remove(K, Sender<()>)
}

impl<K, V> Actor for HashMapShardActor<K, V>
where 
    K: Eq + Hash + 'static,
    V: Clone + 'static
{
    type Arguments = ();
    type Message = HashMapRequest<K, V>;
    type State = HashMap<K, V>;
    fn name() -> &'static str {
        ""
    }
    fn pre_start(_: Self::Arguments) -> anyhow::Result<Self::State> {
        Ok(HashMap::new())
    }
    async fn handle(
            _: crate::core::actor::base::SelfAddr<'_, Self>,
            message: Self::Message,
            state: &mut Self::State,
        ) -> anyhow::Result<()> {
        match message {
            HashMapRequest::Get(k, res) => {
                let _ = res.send(state.get(&k).cloned());
            }
            HashMapRequest::Insert(k, v, tx) => {
                state.insert(k, v);
                let _ = tx.send(());
            }
            HashMapRequest::Remove(k, tx) => {
                state.remove(&k);
                let _ = tx.send(());
            }
        }
        Ok(())
    }
}




#[cfg(test)]
mod tests {
    use crate::core::{actor::lib::hashmap::ShardedHashMap, shard::{shard::{signal_monorail, submit_to}, state::ShardId}, topology::{MonorailConfiguration, MonorailTopology}};


    #[test]
    pub fn test_router_distributed_hashmap() {

        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(6)
                .build(),
            |_| {
                submit_to(ShardId::new(0), async move || {
                    let map = ShardedHashMap::<String, usize>::new().unwrap();

                    // Timer::after(Duration::from_millis(250)).await;

                    map.insert("hello".to_string(), 4).await.unwrap();
                    map.insert("bye".to_string(), 3).await.unwrap();
                    assert_eq!(map.get("hello".to_string()).await.unwrap(), Some(4));
                    assert_eq!(map.get("bye".to_string()).await.unwrap(), Some(3));
                    assert_eq!(map.get("carl".to_string()).await.unwrap(), None);
                    signal_monorail(Ok(()));
                });
            }
        ).unwrap();
    }
}