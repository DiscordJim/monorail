use std::{any::Any, future::Future, panic::AssertUnwindSafe};

use flume::Sender;

use crate::core::{
    channels::promise::SyncPromise,
    shard::{
        error::ShardError,
        shard::{access_shard_ctx_ref, setup_shard, signal_monorail, ShardSeedFn, MONITOR},
        state::{ShardConfigMsg, ShardId},
    },
    task::{self, Task, TaskControlBlock, TaskControlHeader},
};

pub struct MonorailTopology {}

#[derive(Clone)]
pub struct TopologicalInformation {
    pub cores: usize
}

pub struct MonorailConfigurationBuilder {
    limit: Option<usize>,
}

pub struct MonorailConfiguration {
    limit: Option<usize>,
}

impl MonorailConfigurationBuilder {
    /// The core override configures how many shards will be configured.
    /// If this is left unset, this is set to the number of logical cores.
    /// 
    /// Note that this may cause shard-core-wrapping which is when the shards
    /// exceeds the cores, causing multiple to be spawned. Please avoid this!
    ///
    /// **RECOMMENDATION:** If you are not running a test, it is highly recommended
    /// to just leave this to the default and keep it unset.
    pub fn with_core_override(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
    /// Builds the [MonorailConfiguration] from the current builder
    /// context and state.
    pub fn build(self) -> MonorailConfiguration {
        MonorailConfiguration { limit: self.limit }
    }
}

impl MonorailConfiguration {
    pub fn builder() -> MonorailConfigurationBuilder {
        MonorailConfigurationBuilder { limit: None }
    }
}

impl MonorailTopology {
    pub fn setup<T, F>(config: MonorailConfiguration, init: T) -> Result<Self, TopologyError>
    where
        T: FnOnce() -> F + Send + 'static,
        F: Future<Output = ()> + 'static
    {
        println!("Launching topology...");
        launch_topology(config.limit.unwrap_or(num_cpus::get()), init)?;

        Ok(Self {})
    }
}

fn setup_basic_topology(core_count: usize, seeder: Option<ShardSeedFn>) -> Result<(), TopologyError> {
    // let mut seed_queue = None;
    let configurations = (0..core_count)
        .map(|i| setup_shard(ShardId::new(i), core_count))
        .collect::<Result<Vec<_>, ShardError>>()?;
    println!("We are configuring {core_count} shards with only {} cores.", num_cpus::get());
    
    if let Some(seed_fn) = seeder {
        configurations[0].send(ShardConfigMsg::Seed(seed_fn)).unwrap();
    }
    
    for x in 0..core_count {
        for y in 0..core_count {
            if x == y {
                continue;
            }
            let (tx, rx) = SyncPromise::new();
            configurations[y]
                .send(ShardConfigMsg::StartConfiguration {
                    requester: ShardId::new(x),
                    queue: rx,

                })
                .map_err(|_| TopologyError::ShardClosedPrematurely)?;
            let queue = tx
                .wait()
                .map_err(|_| TopologyError::ShardClosedPrematurely)?;
            // if x == 0 && y == 0 {
            //     let (seed_tx, seed_rx) = flume::unbounded();

            //     println!("bringing da queue...");
            //     let initial = TaskControlBlock::create(TaskControlHeader::FireAndForget, async move || {
            //         println!("Erm... {:?}", seed_rx.len());
            //         let task: Box<dyn FnOnce() + Send + 'static>  = seed_rx.recv_async().await.unwrap();
            //         task();
            //     });
            //     let _ = queue.send(initial);
            //     println!("Sent a seed task.");

            //     // let _ = queue.send(BridgedTask::FireAndForget(box_job(async move || {
            //     //     let task: Box<dyn FnOnce() + Send + 'static> = seed_rx.recv_async().await.unwrap();
                    
            //     //     task();
            //     // })));
            //     i
            //     seed_queue = Some(seed_tx);
            // }

            configurations[x]
                .send(ShardConfigMsg::ConfigureExternalShard {
                    target_core: ShardId::new(y),
                    consumer: queue
                    // queue,
                })
                .map_err(|_| TopologyError::ShardClosedPrematurely)?;
        }
    }

    for x in (0..core_count).rev() {
        let (tx, rx) = SyncPromise::new();
        configurations[x]
            .send(ShardConfigMsg::FinalizeConfiguration(rx))
            .map_err(|_| TopologyError::ShardClosedPrematurely)?;

        tx.wait()
            .map_err(|_| TopologyError::ShardClosedPrematurely)?;
    }

    Ok(())
}

fn launch_topology<T, F>(core_count: usize, seeder_function: T) -> Result<(), TopologyError>
where
    T: FnOnce() -> F + Send + 'static,
    F: Future<Output = ()> + 'static
{
    let (promise, resolver) = SyncPromise::<Result<(), Box<dyn Any + Send + 'static>>>::new();
    // std::mem::forget(resolver);
    // println!("Resolover ready...");
    setup_basic_topology(core_count, Some(Box::new(|| {
        MONITOR.with(|f| {
                unsafe { *(&mut *f.get()) = Some(resolver); }
            });

        Box::pin(seeder_function())
    })))?;

    println!("Done setup.");
    // // println!("Set up...");
    // seeder
    //     .send(Box::new(|| {
    //         // println!("hello");

    //         MONITOR.with(|f| {
    //             unsafe { *(&mut *f.get()) = Some(resolver); }
    //         });

    //         access_shard_ctx_ref().executor.spawn(seeder_function()).detach();
    //     }))
    //     .map_err(|_| TopologyError::SeedFailure)?;

    promise.wait().unwrap().unwrap();

    Ok(())
}

#[derive(thiserror::Error, Debug)]
pub enum TopologyError {
    #[error("Failed to initialize shard with error: {0:?}")]
    ShardInitError(#[from] ShardError),
    #[error("An external shard closed during the setup procedure.")]
    ShardClosedPrematurely,
    #[error("Failed to send the seeder task onto the runtime.")]
    SeedFailure
}

#[cfg(test)]
mod tests {
    use std::
        sync::{LazyLock, Mutex}
    ;

    use crate::{core::{
        channels::promise::{SyncPromise, SyncPromiseResolver},
        shard::{
            shard::{shard_id, signal_monorail, submit_to},
            state::ShardId,
        },
        topology::{MonorailConfiguration, MonorailTopology},
    }, monolib};

    #[test]
    pub fn test_launch_single_core_topology() {
        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(1)
                .build(),
            async || {
                assert_eq!(shard_id(), ShardId::new(0));
                signal_monorail(Ok(()));
                // ControlFlow::Break(())
            },
        )
        .unwrap();
    }

    #[test]
    pub fn test_six_core_go_around() {
        static MERRY_GO_ROUND: LazyLock<Mutex<Vec<usize>>> =
            LazyLock::new(|| Mutex::new(Vec::new()));

        fn jmp(resolver: SyncPromiseResolver<()>) {
            MERRY_GO_ROUND.lock().unwrap().push(shard_id().as_usize());
            println!("Broadcasting from {:?}", shard_id());
            if shard_id().as_usize() < 5 {
                submit_to(ShardId::new(shard_id().as_usize() + 1), async || {
                    jmp(resolver)
                });
            } else {
                resolver.resolve(());
            }
        }

        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(6)
                .build(),
            async || {
                // println!("hello... (from async)");
                let (rx, tx) = SyncPromise::new();
                jmp(tx);
                rx.wait().unwrap();
                signal_monorail(Ok(()));
                // ControlFlow::Break(())
            },
        )
        .unwrap();

        assert_eq!(&*MERRY_GO_ROUND.lock().unwrap(), &[0, 1, 2, 3, 4, 5]);
    }

    #[test]
    pub fn test_choked_cores() {
        // TEST: Specifying a core ovveride greater than the CPU count.
        static MERRY_GO_ROUND: LazyLock<Mutex<Vec<usize>>> =
            LazyLock::new(|| Mutex::new(Vec::new()));

        fn jmp(resolver: SyncPromiseResolver<()>) {
            MERRY_GO_ROUND.lock().unwrap().push(shard_id().as_usize());
            // println!("Broadcasting from {:?} {:?}", shard_id(), top);
            if shard_id().as_usize() < monolib::get_topology_info().cores - 1 {
                submit_to(ShardId::new(shard_id().as_usize() + 1), async || {
                    jmp(resolver)
                });
            } else {
                println!("Resolving...");
                resolver.resolve(());
            }
        }

        // println!("Spawning with: {:?}", num_cpus::get() + 1);

        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(num_cpus::get() + 1)
                .build(),
            async || {
                // println!("hello... (from async)");
                let (rx, tx) = SyncPromise::new();
                jmp(tx);
                rx.wait().unwrap();
                signal_monorail(Ok(()));
                // ControlFlow::Break(())
            },
        )
        .unwrap();


        let mut ar = vec![];
        for i in 0..num_cpus::get() + 1 {
            ar.push(i);
        }

        assert_eq!(&*MERRY_GO_ROUND.lock().unwrap(), &ar);
    }

    #[test]
    pub fn test_self_signal_msg() {
        MonorailTopology::setup(MonorailConfiguration::builder().build(), async || {
  

            monolib::submit_to(shard_id(), async || {
                signal_monorail(Ok(()));
            });

        }).unwrap();
    }

    #[test]
    pub fn test_self_signal() {
        MonorailTopology::setup(MonorailConfiguration::builder().build(), async || {

            println!("hello!");

            

            let wow = monolib::call_on(shard_id(), async || {
                3
            }).await.unwrap();
            assert_eq!(wow, 3);

            signal_monorail(Ok(()));

        }).unwrap();
    }
}
