use crate::core::{actor::{base::{Actor, LocalAddr}, manager::{tam_resolve, Addr, ResolutionError}}, channels::promise::PromiseError, shard::shard::access_shard_ctx_ref};



pub async fn register<A>(name: &'static str, address: LocalAddr<A>) -> anyhow::Result<(), PromiseError>
where   
    A: Actor
{
    access_shard_ctx_ref().actors.register(name, address).await
}

pub fn resolve<A>(name: &'static str) -> Result<Addr<A>, ResolutionError>
where 
    A: Actor
{
    tam_resolve(name)
}