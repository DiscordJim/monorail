use std::{any::Any, panic::AssertUnwindSafe};

use flume::Sender;

use crate::core::{
    channels::promise::SyncPromise,
    shard::{
        error::ShardError,
        shard::{setup_shard, signal_monorail, MONITOR},
        state::{ShardConfigMsg, ShardId, ShardRuntime},
    },
    task::Task,
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
    pub fn setup<F>(config: MonorailConfiguration, init: F) -> Result<Self, TopologyError>
    where
        F: FnOnce(&mut ShardRuntime) + Send + 'static,
    {
        launch_topology(config.limit.unwrap_or(num_cpus::get()), init)?;

        Ok(Self {})
    }
}

fn setup_basic_topology(core_count: usize) -> Result<Sender<Task>, TopologyError> {
    let mut seed_queue = None;
    let configurations = (0..core_count)
        .map(|i| setup_shard(ShardId::new(i), core_count))
        .collect::<Result<Vec<_>, ShardError>>()?;
    for x in 0..core_count {
        for y in 0..core_count {
            let (tx, rx) = SyncPromise::new();
            configurations[y]
                .send(ShardConfigMsg::RequestEntry {
                    // requester: ShardId::new(x),
                    queue: rx,
                })
                .map_err(|_| TopologyError::ShardClosedPrematurely)?;
            let queue = tx
                .wait()
                .map_err(|_| TopologyError::ShardClosedPrematurely)?;
            if x == 0 && y == 0 {
                seed_queue = Some(queue.clone());
            }

            configurations[x]
                .send(ShardConfigMsg::ConfigureExternalShard {
                    target_core: ShardId::new(y),
                    queue,
                })
                .map_err(|_| TopologyError::ShardClosedPrematurely)?;
        }
    }

    for x in 0..core_count {
        let (tx, rx) = SyncPromise::new();
        configurations[x]
            .send(ShardConfigMsg::FinalizeConfiguration(rx))
            .map_err(|_| TopologyError::ShardClosedPrematurely)?;

        tx.wait()
            .map_err(|_| TopologyError::ShardClosedPrematurely)?;
    }

    Ok(seed_queue.unwrap())
}

fn launch_topology<F>(core_count: usize, seeder_function: F) -> Result<(), TopologyError>
where
    F: FnOnce(&mut ShardRuntime) + Send + 'static,
{
    let (promise, resolver) = SyncPromise::<Result<(), Box<dyn Any + Send + 'static>>>::new();
    // std::mem::forget(resolver);
    // println!("Resolover ready...");
    let seeder = setup_basic_topology(core_count)?;
    // println!("Set up...");
    seeder
        .send(Box::new(move |runtime| {
            // println!("hello...");
            match std::panic::catch_unwind(AssertUnwindSafe(|| {

                 unsafe { 
                    // println!("HELLO");
                        MONITOR.with(|f| {
                            (*f.get()) = Some(resolver);
                        });
                        // println!("SET RUNTIME...");
                     }

                seeder_function(runtime)
            })) {
                Ok(_) => {},
                Err(e) => {
                    signal_monorail(Err(e));
                    // resolver.resolve(Err(e));
                }
            }
        }))
        .map_err(|_| TopologyError::SeedFailure)?;

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

    use crate::core::{
        channels::promise::{SyncPromise, SyncPromiseResolver},
        shard::{
            shard::{signal_monorail, submit_to},
            state::{ShardId, ShardRuntime},
        },
        topology::{MonorailConfiguration, MonorailTopology},
    };

    #[test]
    pub fn test_launch_single_core_topology() {
        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(1)
                .build(),
            |d| {
                assert_eq!(d.id, ShardId::new(0));
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

        fn jmp(runtime: &mut ShardRuntime, resolver: SyncPromiseResolver<()>) {
            MERRY_GO_ROUND.lock().unwrap().push(runtime.id.as_usize());
            if runtime.id.as_usize() < 5 {
                submit_to(ShardId::new(runtime.id.as_usize() + 1), move |runtime| {
                    jmp(runtime, resolver)
                });
            } else {
                resolver.resolve(());
            }
        }

        MonorailTopology::setup(
            MonorailConfiguration::builder()
                .with_core_override(6)
                .build(),
            move |d| {
                let (rx, tx) = SyncPromise::new();
                jmp(d, tx);
                rx.wait().unwrap();
                signal_monorail(Ok(()));
                // ControlFlow::Break(())
            },
        )
        .unwrap();

        assert_eq!(&*MERRY_GO_ROUND.lock().unwrap(), &[0, 1, 2, 3, 4, 5]);
    }
}
