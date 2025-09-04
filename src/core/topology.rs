use std::thread::park;

use flume::Sender;

use crate::core::{
    channels::promise::SyncPromise,
    shard::{
        error::ShardError,
        shard::setup_shard,
        state::{ShardConfigMsg, ShardId, ShardRuntime},
    },
    task::Task,
};

pub struct MonorailTopology {}

impl MonorailTopology {
    pub fn setup<F>(init: F) -> Result<Self, TopologyError>
    where
        F: FnOnce(&mut ShardRuntime) + Send + 'static,
    {
        launch_topology(num_cpus::get(), init)?;

        Ok(Self {})
    }
}

fn setup_basic_topology(core_count: usize) -> Result<Sender<Task>, TopologyError> {
    let mut seed_queue = None;
    let configurations = (0..core_count)
        .map(|i| setup_shard(ShardId::new(i), core_count))
        .collect::<Result<Vec<_>, ShardError>>()?;

    // std::thread::sleep(Duration::from_secs(3));
    for x in 0..core_count {
        // println!("{x:?}");
        for y in 0..core_count {
            let (tx, rx) = SyncPromise::new();
            configurations[y]
                .send(ShardConfigMsg::RequestEntry {
                    requester: ShardId::new(x),
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
            // println!("{x} -> {y}");
        }
    }

    for x in 0..core_count {
        // println!("Finalizing core {x}");
        // let (tx, rx) = flume::bounded(1);
        let (tx, rx) = SyncPromise::new();
        configurations[x]
            .send(ShardConfigMsg::FinalizeConfiguration(rx))
            .map_err(|_| TopologyError::ShardClosedPrematurely)?;

        tx.wait()
            // .recv()
            .map_err(|_| TopologyError::ShardClosedPrematurely)?;

        // println!("Shard {x} is ready.");
    }

    Ok(seed_queue.unwrap())
}

fn launch_topology<F>(core_count: usize, seeder_function: F) -> Result<(), TopologyError>
where
    F: FnOnce(&mut ShardRuntime) + Send + 'static,
{
    let seeder = setup_basic_topology(core_count)?;
    seeder
        .send(Box::new(seeder_function))
        .map_err(|_| TopologyError::SeedFailure)?;

    park();

    Ok(())
}

#[derive(thiserror::Error, Debug)]
pub enum TopologyError {
    #[error("Failed to initialize shard with error: {0:?}")]
    ShardInitError(#[from] ShardError),
    #[error("An external shard closed during the setup procedure.")]
    ShardClosedPrematurely,
    #[error("Failed to send the seeder task onto the runtime.")]
    SeedFailure, // #[from(ShardError)]
                 // ShardInitError(ShardError)
}
