use crate::core::{actor::{base::{Actor, LocalAddr}, manager::{Addr, ResolutionError, tam_resolve}}, channels::promise::PromiseError, shard::state::ShardCtx};



pub async fn register<A>(name: &'static str, address: LocalAddr<A>) -> anyhow::Result<(), PromiseError>
where   
    A: Actor
{
    ShardCtx::access_ref().actors.register(name, address).await
}

pub fn resolve<A>(name: &'static str) -> Result<Addr<A>, ResolutionError>
where 
    A: Actor
{
    tam_resolve(name)
}