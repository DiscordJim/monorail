use std::cell::RefCell;
use std::marker::PhantomData;


use crate::core::actor::manager::ThreadActorManager;
use crate::core::channels::bridge::{Bridge, BridgeConsumer, BridgeProducer, Rx, Tx};
use crate::core::channels::promise::SyncPromiseResolver;
use crate::core::executor::scheduler::Executor;
use crate::core::shard::shard::ShardSeedFn;
use crate::core::topology::TopologicalInformation;
use crate::core::{shard::error::ShardError};
use crate::core::channels::Sender;


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
    pub id: ShardId,
    pub table: ShardMapTable,
    pub top_info: TopologicalInformation,
    pub executor: Executor<'static>,
    pub actors: ThreadActorManager,
    _unsend: PhantomData<*const ()>
}

impl ShardCtx {
    pub(crate) fn new(core: ShardId, info: TopologicalInformation, table: ShardMapTable) -> Self {
        Self {
            id: core,
            table,
            top_info: info,
            executor: Executor::new(core),
            actors: ThreadActorManager::new(core).into(),
            _unsend: PhantomData
        }
    }
}

// pub struct ShardRuntime {
//     pub id: ShardId,
//     // office: ShardActorOffice
//     // pub executor: &'a LocalExecutor<'a>
// }

// impl ShardRuntime {
//     pub fn new(core: ShardId) -> Self {
//         Self {
//             id: core,
//             // office: ShardActorOffice::new()
//         }
//     }
// }

pub struct ShardMapTable {
    pub table: Box<[ShardRoute]>
}

pub(crate) enum ShardRoute {
    /// This shard points to a bridge.
    Bridge(Bridge),
    Loopback
}


impl ShardMapTable {
    pub(crate) fn initialize<F>(cores: usize, functor: F) -> Result<Self, ShardError>
    where 
        F: FnOnce(&mut [Option<ShardRoute>]) -> Result<(), ShardError>
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
        consumer: BridgeProducer<Rx>
    },
    StartConfiguration {
        /// The core that wishes to establish a bridge.
        requester: ShardId,

        

        /// The resolver which contains the consumer for
        /// messages from the shard receiving this message.
        /// 
        /// The first item of the tuple is the consumer for receiving
        /// and the second item is the producer for sending.
        queue: SyncPromiseResolver<BridgeProducer<Rx>>
    },
    Seed(ShardSeedFn),
    FinalizeConfiguration(SyncPromiseResolver<()>)
}