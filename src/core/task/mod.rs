use crate::core::shard::state::ShardRuntime;

pub type Task = Box<dyn FnOnce(&mut ShardRuntime) + Send + 'static>;
