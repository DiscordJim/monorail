use std::{any::Any, cell::UnsafeCell, future::Future};

use nix::{
    sched::{sched_setaffinity, CpuSet},
    unistd::{gettid, Pid},
};

use crate::core::{actor::base::{Actor, ActorSignal, Addr, FornAddr}, channels::{bridge::{Bridge, Rx, Tx}, promise::{PromiseError, SyncPromiseResolver}}, executor::mail::MailId, shard::{
    error::ShardError,
    state::{ShardConfigMsg, ShardCtx, ShardId, ShardMapTable},
}, topology::TopologicalInformation};
use crate::core::
    channels::{Receiver, Sender}
;

fn bind_core<F>(core: usize, functor: F) -> nix::Result<()>
where
    F: FnOnce(Pid) + Send + 'static,
{

    std::thread::spawn(move || {
        let thread_id = gettid();

        let mut cpu_set = CpuSet::new();
        cpu_set.set(core)?;
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

    // println!("Binding {core:?} -> {}", core.as_usize() % num_cpus::get());
    bind_core(core.as_usize() % num_cpus::get(), move |_| {
        // println!("Peforming a core bind {:?}", core);
        if let Err(e) = perform_core_bind(core, total_cores, config_rx) {
            eprintln!("Shard Failure: {e:?}");
        }
    })?;

    Ok(config_tx)
}

thread_local! {
    // static SHARD_CTX: UnsafeCell<Option<&'static ShardCtx>> = const  { UnsafeCell::new(None) };
    static ROUTING_TABLE: UnsafeCell<Option<&'static ShardCtx>> = const { UnsafeCell::new(None) };
    pub static MONITOR: UnsafeCell<Option<SyncPromiseResolver<Result<(), Box<dyn Any + Send + 'static>>>>> = const { UnsafeCell::new(None) };
}

pub fn signal_monorail(result: Result<(), Box<dyn Any + Send + 'static>>) {
    let ctx = access_shard_ctx_ref();
    if ctx.id == ShardId::new(0) {
        println!("Hello");
        unsafe {
            MONITOR.with(|f| {
                if let Some(val) = (&mut *f.get()).take() {
                    val.resolve(result);
            } else {
                println!("bad bad bad!");
            }
            });
            
        }
    } else {
        submit_to(ShardId::new(0), async || signal_monorail(result));
    }
}

pub(crate) fn access_shard_ctx_ref() -> &'static ShardCtx {
    ROUTING_TABLE.with(|f| unsafe { (&*f.get()).unwrap() })
}

pub(crate) fn spawn_async_task<F, T>(future: F) -> smol::Task<T>
where 
    F: Future<Output = T> + 'static,
    T: 'static
{
    let r = access_shard_ctx_ref();
    r.executor.spawn(future)
}

pub(crate) fn get_actor_addr<A>(addr: FornAddr<A>) -> Option<Addr<A>>
where 
    A: Actor
{
    let local = access_shard_ctx_ref();
    local.executor.lookup_actor(addr)
}

// pub(crate) fn with_signal_handler<F>(addr: &FornAddr<A>) -> anyhow::Result<()>
// where 
//     F: FnOnce(&SignalHandle)
// {
//     let local = access_shard_ctx_ref();
//     local.executor.

// }

#[inline]
pub(crate) fn signal_actor_mailbox(addr: MailId, signal: ActorSignal) -> anyhow::Result<()> {

    access_shard_ctx_ref().executor.signal_mailbox(addr, signal)?;

    Ok(())
}

pub fn spawn_actor<A>(args: A::Arguments) -> anyhow::Result<(FornAddr<A>, Addr<A>)>
where 
    A: Actor + 'static
{

    let r = access_shard_ctx_ref();
    let (faddr, addr) = r.executor.spawn_actor(args)?;
    // r.

    Ok((faddr, addr))


}

// pub(crate) async fn spawn_local_task<'a>() {

// }


// pub(crate) fn monorail_unwind_guard<F>(function: F)
// where 
//     F: FnOnce() -> () 
// {
//     match std::panic::catch_unwind(AssertUnwindSafe(|| function())) {
//         Ok(v) => {
//             // println!("HELLO...2");

//         }
//         Err(e) => {
//             signal_monorail(Err(e));
//         }
//     }

// }

// // pub

// pub fn submit_task_to<F, FUT>(core: ShardId, task: F)
// where 
//     F: FnOnce() -> FUT + Send + 'static,
//     FUT: Future<Output = ()> + 'static
// {
//     submit_to(core, move |runtime| {
//         let task = task();

//         // spawn_async_task(async move {
//         //     use smol::future::FutureExt;


//         // //    task.catch_unwind().await; 

//         // });

//         spawn_async_task(async move {
//             match AssertUnwindSafe(task).catch_unwind().await {
//                 Ok(_) => {},
//                 Err(e) => signal_monorail(Err(e)),
//             }
//         }).detach();
//     });
// }

pub(crate) fn get_remote_brige(origin: ShardId) -> &'static Bridge {
    let ctx = access_shard_ctx_ref();
    &ctx.table.table[origin.as_usize()]
}

pub async fn call_on<F, FUT, O>(core: ShardId, task: F) -> Result<O, PromiseError>
where 
    F: FnOnce() -> FUT + Send + 'static,
    FUT: Future<Output = O> + 'static,
    O: Send + 'static
{
    // let job: AsyncTaskWithResult = Box::new(|| Box::pin(async move {
    //     let r = task().await;
    //     Box::new(r) as Box<dyn Any + Send + 'static>
    // }));

    let shot = access_shard_ctx_ref().table.table[core.as_usize()].fire_with_ticket(task).await?;

    // let shot: Box<O> = shot.downcast().expect("Wrong return type?");

    Ok(shot)

    

    // let bridged = Bridge
}

pub fn submit_to<F, FUT>(core: ShardId, task: F)
where
    F: FnOnce() -> FUT + Send + 'static,
    FUT: Future<Output = ()> + 'static
{


    // let job = box_job(task);

    ROUTING_TABLE
        .with(|f: &UnsafeCell<Option<&'static ShardCtx>>| unsafe { &*f.get() }.unwrap())
        .table
        .table[core.as_usize()]
        
        .fire_and_forget(task);
    println!("fired!");
}

pub fn get_topology_info() -> &'static TopologicalInformation {
    &access_shard_ctx_ref().top_info
}

pub fn shard_id() -> ShardId {
    access_shard_ctx_ref().id
}

fn perform_core_bind(
    core: ShardId,
    core_count: usize,
    config_rx: Receiver<ShardConfigMsg>,
) -> Result<(), ShardError> {
    let (table, notifier) = configure_shard(core, core_count, config_rx)?;

    // println!("Configuration done for core: {core:?}");

    let context = Box::leak(Box::new(ShardCtx::new(core, TopologicalInformation { cores: core_count }, table)));

    // SHARD_CTX.with(|f| unsafe { *f.get() =  Some(&context) } );
    ROUTING_TABLE.with(|f| unsafe { *f.get() = Some(context) });

    configure_shard_executor(context, notifier);

    Ok(())
}

// struct ShardRunFut<'a> {
//     receiver: Receiver<Task>,
//     runtime: &'a mut ShardRuntime
// }

// impl<'a> Future for ShardRunFut<'a> {
//     type Output = ();
//     fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        
//         Poll::Pending
//     }
// }

fn configure_shard_executor(
    ctx: &'static ShardCtx,
    notifier: SyncPromiseResolver<()>,
) {
    // let mut runtime = Rc::new(UnsafeCell::new(ShardRuntime::new(core)));
    // let executor = LocalExecutor::new().leak();

    // println!("Entering...");

    smol::future::block_on(ctx.executor.run(async move {


        for bridge in &ctx.table.table {
            ctx.executor.spawn(async move {
                // println!("spawning bridge. ({:?})", shard_id());
                bridge.run_bridge().await;
            }).detach();
        }

        // for recvr in rcvers {
        //     let rcvr = Box::leak(Box::new(recvr));
        //     let runtime2 = runtime.clone();
        //     ctx.executor.spawn(async move {
        //         let runtime2 = unsafe { &mut *runtime2.get() };
        //         loop {
        //             // println!("WAITING>..");
        //             let task = rcvr.recv_async().await.unwrap();
        //             // println!("GOT A TASK...");
        //             // task(runtime2);
        //             monorail_unwind_guard(|| task(runtime2));
        //         }
        //     }).detach();

        // }


        let _ = notifier.resolve(());
        smol::future::pending::<()>().await;
    }));

    
}

fn configure_shard(
    core: ShardId,
    total_cores: usize,
    config_rx: Receiver<ShardConfigMsg>,
) -> Result<(ShardMapTable, SyncPromiseResolver<()>), ShardError> {


    let mut producers = vec![];
    for _ in 0..total_cores {
        producers.push(None);
    }
    let mut consumers = vec![];
    for _ in 0..total_cores {
        consumers.push(None);
    }

    //   let mut producers_rx = vec![];
    // for i in 0..total_cores {
    //     producers_rx.push(None);
    // }
    // let mut consumers_rx = vec![];
    // for i in 0..total_cores {
    //     consumers_rx.push(None);
    // }


    // let shard_recievers = Vec::with_capacity(total_cores);
    let mut notifier = None;
    let table = ShardMapTable::initialize(total_cores, |rt| {
        loop {
            match config_rx.recv() {
                Ok(msg) => match msg {
                    ShardConfigMsg::ConfigureExternalShard { target_core, consumer } => {
                        // producers[target_core.as_usize()] = Some(from_receiver);

                        producers[target_core.as_usize()] = Some(consumer);



                        // producers_rx[]
                        // consumers_tx[target_core.as_usize()] = Some(rx_consumer);
                        // producers_rx[target_core.as_usize()] = Some(tx_producer);
                    }
                    ShardConfigMsg::StartConfiguration {
                        requester,
                        queue,
                    } => {

                        let (tx_producer, tx_consumer) = Bridge::create_queues::<Rx, Tx>();
                        // let (rx_producer, rx_consumer) = Bridge::create_queues();

                        // producers[requester.as_usize()] = Some(tx_producer);
                        // consumers_rx[requester.as_usize()] = Some(rx_consumer);

                        consumers[requester.as_usize()] = Some(tx_consumer);

                        queue.resolve(tx_producer);

                        // producers[requester.as_usize()] = Some(producer);
                        // queue.resolve(consumer);
                        // consumers[requester.as_usize()] = Some(consumer);
                        // queue.

                        // let (tx, rx) = flume::bounded(50);
                        // shard_recievers.push(rx);
                        // queue.resolve(tx);
                    }
                    ShardConfigMsg::FinalizeConfiguration(tx) => {

                        // let tx = producers_rx.into_iter().map(Option::unwrap).zip(consumers.into_iter().map(Option::unwrap)).collect::<Vec<_>>();
                        // let rx = producers.into_iter().map(Option::unwrap).zip(consumers_rx.into_iter().map(Option::unwrap)).collect::<Vec<_>>();


                        let mut channels = producers.into_iter().map(Option::unwrap).zip(consumers.into_iter().map(Option::unwrap)).map(|(producer, consumer)| {
                            Bridge::create(producer, consumer)
                        }).collect::<Vec<_>>();
                        
                        // for i in 0..channels.len() {
                        //     rt[i] = Some(channels[i]);
                        // }

                        for i in 0..channels.len() {
                            rt[i] = Some(channels.remove(0));
                        }

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

    Ok((table, notifier.unwrap()))
}

