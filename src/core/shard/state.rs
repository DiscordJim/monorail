use std::marker::PhantomData;
use std::mem::MaybeUninit;

use async_executor::StaticLocalExecutor;
use smol::LocalExecutor;

use crate::core::channels::promise::SyncPromiseResolver;
use crate::core::{shard::error::ShardError, task::Task};
use crate::core::channels::{Sender, Receiver};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ShardId(usize);

impl ShardId {
    pub fn new(id: usize) -> Self {
        Self(id)
    }
    pub fn as_usize(&self) -> usize {
        self.0
    }
}

pub struct ShardCtx {
    id: ShardId,
    pub table: ShardMapTable,
    _unsend: PhantomData<*const ()>
}

impl ShardCtx {
    pub(crate) fn new(core: ShardId, table: ShardMapTable) -> Self {
        Self {
            id: core,
            table,
            _unsend: PhantomData
        }
    }
}

pub struct ShardRuntime {
    // pub executor: &'a LocalExecutor<'a>
}

pub struct ShardMapTable {
    pub table: Box<[ShardMapEntry]>
}


pub struct ShardMapEntry {
    pub queue: crate::core::channels::Sender<Task>
}



impl ShardMapTable {
    pub(crate) fn initialize<F>(cores: usize, functor: F) -> Result<Self, ShardError>
    where 
        F: FnOnce(&mut [Option<ShardMapEntry>]) -> Result<(), ShardError>
    {

        let mut array = Vec::with_capacity(cores);
        for _ in 0..cores {
            array.push(None);
        }
        functor(&mut array)?;
        // println!("Termianted..");
        let boxed = array.into_iter().map(Option::unwrap).collect::<Box<[_]>>();

        Ok(ShardMapTable {
            table: boxed
        })
        
    }
}

pub(crate) enum ShardConfigMsg {
    // WaitReady(flume::Sender<()>),
    ConfigureExternalShard {
        target_core: ShardId,
        queue: Sender<Task>
    },
    RequestEntry {
        requester: ShardId,
        queue: SyncPromiseResolver<Sender<Task>>
    },
    FinalizeConfiguration(SyncPromiseResolver<()>)
}