use std::{cell::UnsafeCell, future::Future, pin::Pin, task::{Context, Poll}};

use nix::{
    sched::{sched_setaffinity, CpuSet},
    unistd::{gettid, Pid},
};
use smol::{future, LocalExecutor};

use crate::core::{channels::promise::SyncPromiseResolver, shard::{
    error::ShardError,
    state::{ShardConfigMsg, ShardCtx, ShardId, ShardMapEntry, ShardMapTable, ShardRuntime},
}};
use crate::core::{
    channels::{Receiver, Sender},
    task::Task,
};

fn bind_core<F>(core: ShardId, functor: F) -> nix::Result<()>
where
    F: FnOnce(Pid) + Send + 'static,
{
    std::thread::spawn(move || {
        let thread_id = gettid();

        let mut cpu_set = CpuSet::new();
        cpu_set.set(core.as_usize())?;
        sched_setaffinity(thread_id, &cpu_set)?;

        functor(thread_id);

        Ok::<_, nix::Error>(())
    });
    Ok(())
}

pub(crate) fn setup_shard(
    core: ShardId,
    total_cores: usize,
) -> Result<Sender<ShardConfigMsg>, ShardError> {
    let (config_tx, config_rx) = crate::core::channels::make_bounded(4);

    bind_core(core, move |_| {
        // println!("Peforming a core bind {:?}", core);
        if let Err(e) = perform_core_bind(core, total_cores, config_rx) {
            eprintln!("Shard Failure: {e:?}");
        }
    })?;

    Ok(config_tx)
}

thread_local! {
    static ROUTING_TABLE: UnsafeCell<Option<&'static ShardCtx>> = const { UnsafeCell::new(None) };
}

pub fn submit_to<F>(core: ShardId, task: F)
where
    F: FnOnce(&mut ShardRuntime) + Send + 'static,
{
    ROUTING_TABLE
        .with(|f| unsafe { &*f.get() }.unwrap())
        .table
        .table[core.as_usize()]
    .queue
    .send(Box::new(task))
    .expect("Failed for cross core communicatin..");
}

fn perform_core_bind(
    core: ShardId,
    core_count: usize,
    config_rx: Receiver<ShardConfigMsg>,
) -> Result<(), ShardError> {
    let (table, rcvers, notifier) = configure_shard(core, core_count, config_rx)?;

    // println!("Configuration done for core: {core:?}");

    let context = Box::leak(Box::new(ShardCtx::new(core, table)));

    ROUTING_TABLE.with(|f| unsafe { *f.get() = Some(context) });

    configure_shard_executor(core, core_count, rcvers, notifier);

    Ok(())
}

struct ShardRunFut<'a> {
    receiver: Receiver<Task>,
    runtime: &'a mut ShardRuntime
}

impl<'a> Future for ShardRunFut<'a> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        
        Poll::Pending
    }
}

fn configure_shard_executor(
    core: ShardId,
    core_count: usize,
    rcvers: Vec<Receiver<Task>>,
    notifier: SyncPromiseResolver<()>,
) {
    let mut runtime = ShardRuntime {
        // executor: &executor
    };

    let executor = LocalExecutor::new().leak();

    // println!("Entering...");

    future::block_on(executor.run(async {
        for rcvr in rcvers {
            let rcvr = Box::leak(Box::new(rcvr));
            unsafe { 
                executor
                .spawn_scoped(async {
                    loop {
                        let task = rcvr.recv_async().await.unwrap();
                        task(&mut runtime);
                    }
                })
                .detach();

            }
            
            // std::mem::forget(rcvr);
        }

        // println!("Starting core: {core:?}");

        let _ = notifier.resolve(());
        smol::future::pending::<()>().await;
    }));
}

fn configure_shard(
    core: ShardId,
    total_cores: usize,
    config_rx: Receiver<ShardConfigMsg>,
) -> Result<(ShardMapTable, Vec<Receiver<Task>>, SyncPromiseResolver<()>), ShardError> {
    let mut shard_recievers = Vec::with_capacity(total_cores);
    let mut notifier = None;
    let table = ShardMapTable::initialize(total_cores, |rt| {
        loop {
            match config_rx.recv() {
                Ok(msg) => match msg {
                    ShardConfigMsg::ConfigureExternalShard { target_core, queue } => {
                        rt[target_core.as_usize()] = Some(ShardMapEntry { queue });
                    }
                    ShardConfigMsg::RequestEntry {
                        requester: _,
                        queue,
                    } => {
                        let (tx, rx) = flume::bounded(50);
                        shard_recievers.push(rx);
                        queue.resolve(tx);
                    }
                    ShardConfigMsg::FinalizeConfiguration(tx) => {
                        notifier = Some(tx);
                        // println!("Shard {core:?} was told to finalize.");
                        break;
                    }
                    _ => {}
                },
                Err(_) => {
                    return Err(ShardError::ConfigFailure(core));
                }
            }
        }
        // println!("Broke out of core.");
        Ok(())
    })?;

    // println!("Shard {core:?} is terminating...");

    Ok((table, shard_recievers, notifier.unwrap()))
}
