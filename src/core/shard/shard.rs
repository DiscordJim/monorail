use std::{any::Any, cell::UnsafeCell, future::{poll_fn, Future}, panic::AssertUnwindSafe, pin::Pin, task::Poll};

use nix::{
    sched::{sched_setaffinity, CpuSet},
    unistd::{gettid, Pid},
};

use crate::core::{actor::{base::{Actor, ActorSignal, LocalAddr}, manager::Addr}, channels::{bridge::{Bridge, Rx, Tx}, promise::{Promise, PromiseError, PromiseResolver, SyncPromiseResolver}}, shard::{
    error::ShardError,
    state::{ShardConfigMsg, ShardCtx, ShardId, ShardMapTable, ShardRoute},
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


// pub(crate) fn with_signal_handler<F>(addr: &FornAddr<A>) -> anyhow::Result<()>
// where 
//     F: FnOnce(&SignalHandle)
// {
//     let local = access_shard_ctx_ref();
//     local.executor.

// }


// pub fn signal_actor_address



pub fn spawn_actor<A>(args: A::Arguments) -> anyhow::Result<(Addr<A>, LocalAddr<A>)>
where 
    A: Actor + 'static
{

    let r = access_shard_ctx_ref();
    let address = r.actors.spawn_actor::<A>(&r.executor, args)?;
    let local = address.clone().upgrade().map_err(|_| ()).expect("This must upgrade!");

    Ok((address, local))


}


// pub(crate) fn spawn_actor_full<A>(args: A::Arguments) -> anyhow::Result<(Addr<A>, LocalAddr<A>)>
// where 
//     A: Actor + 'static
// {

//     let r = access_shard_ctx_ref();
//     let address = r.actors.spawn_actor::<A>(&r.executor, args)?;
//     let local = address.clone().upgrade().map_err(|_| ()).expect("This must upgrade!");

//     Ok((address, local))


// }

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

// pub(crate) fn get_remote_brige(origin: ShardId) -> &'static Bridge {
//     let ctx = access_shard_ctx_ref();
//     &ctx.table.table[origin.as_usize()]
// }

pub fn get_shard_route(core: ShardId) -> Option<&'static ShardRoute> {
    access_shard_ctx_ref().table.table.get(core.as_usize())
}

pub async fn call_on<F, FUT, O>(core: ShardId, task: F) -> Result<O, PromiseError>
where 
    F: FnOnce() -> FUT + Send + 'static,
    FUT: Future<Output = O> + 'static,
    O: Send + 'static
{
    match &access_shard_ctx_ref().table.table[core.as_usize()] {
        ShardRoute::Bridge(bridge) => {

            let shot = bridge.fire_with_ticket(task).await?;
            Ok(shot)
        }
        ShardRoute::Loopback => {
            call_on_local(task).await
        }
    }
}

use futures::FutureExt;


#[pin_project::pin_project]
struct LocalPromise<F, O> {
    #[pin]
    future: F,
    resolver: Option<PromiseResolver<O>>
}

impl<F, O> Future for LocalPromise<F, O>
where 
    F: Future<Output = O>
{
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match std::panic::catch_unwind(AssertUnwindSafe(|| this.future.poll(cx))) {
            Ok(poll) => match poll {
                Poll::Pending => Poll::Pending,
                Poll::Ready(o) => {
                    
                this.resolver.take().unwrap().resolve(o);
                Poll::Ready(())
            // }
                }
            }
            Err(e) => {
                this.resolver.take().unwrap().reject_panic(e);
                Poll::Ready(())
            }
        }
    }
}

#[inline]
pub async fn call_on_local<F, FUT, O>(task: F) -> Result<O, PromiseError>
where 
    F: FnOnce() -> FUT + Send + 'static,
    FUT: Future<Output = O> + 'static,
    O: Send + 'static
{
    // let (rx, tx) = Promise::<O>::new();
    match std::panic::catch_unwind(AssertUnwindSafe(|| task())) {
        Ok(fut) => {
            let (rx, tx) = Promise::<O>::new();

            access_shard_ctx_ref()
                .executor
                .spawn(LocalPromise {
                    future: fut,
                    resolver: Some(tx)
                }).detach();

            rx.await
            
        }
        Err(e) => Err(PromiseError::Paniced(e))
    }
}


pub fn submit_to<F, FUT>(core: ShardId, task: F)
where
    F: FnOnce() -> FUT + Send + 'static,
    FUT: Future<Output = ()> + 'static
{


    // let job = box_job(task);



    match &access_shard_ctx_ref().table.table[core.as_usize()] {
        ShardRoute::Bridge(bridge) => {

            bridge.fire_and_forget(task);
            // Ok(shot)
        }
        ShardRoute::Loopback => {
//
            // let fut= task();
            access_shard_ctx_ref()
                .executor
                .spawn(task()).detach();
            // Err(PromiseError::PromiseClosed)
        }
    }
    // println!("fired!");
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
    let ShardInitCtx { table, resolver, seed } = configure_shard(core, core_count, config_rx)?;

    // println!("Configuration done for core: {core:?}");

    let context = Box::leak(Box::new(ShardCtx::new(core, TopologicalInformation { cores: core_count }, table)));

    // SHARD_CTX.with(|f| unsafe { *f.get() =  Some(&context) } );
    ROUTING_TABLE.with(|f| unsafe { *f.get() = Some(context) });

    configure_shard_executor(context,  resolver, seed);

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
    seeder: Option<ShardSeedFn>
) {
    // let mut runtime = Rc::new(UnsafeCell::new(ShardRuntime::new(core)));
    // let executor = LocalExecutor::new().leak();

    // println!("Entering...");

    smol::future::block_on(ctx.executor.run(async move {

        if let Some(functor) = seeder {
            println!("Has a seeder.");
            ctx.executor.spawn(functor()).detach();
        }

        for bridge in &ctx.table.table {
            if let ShardRoute::Bridge(bridge) = bridge {
                 ctx.executor.spawn(async move {
                // println!("spawning bridge. ({:?})", shard_id());
                bridge.run_bridge().await;
            }).detach();
            }
           
        }

        let _ = notifier.resolve(());
        smol::future::pending::<()>().await;
    }));

    
}

pub type ShardSeedFn = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + 'static>> + Send + 'static>;

struct ShardInitCtx {
    table: ShardMapTable,
    resolver: SyncPromiseResolver<()>,
    seed: Option<ShardSeedFn>
}

fn configure_shard(
    core: ShardId,
    total_cores: usize,
    config_rx: Receiver<ShardConfigMsg>,
) -> Result<ShardInitCtx, ShardError> {


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


    let mut seeder = None;
    // let shard_recievers = Vec::with_capacity(total_cores);
    let mut notifier = None;
    let table = ShardMapTable::initialize(total_cores, |rt| {
        loop {
            match config_rx.recv() {
                Ok(msg) => match msg {
                    ShardConfigMsg::ConfigureExternalShard { target_core, consumer } => {
                        producers[target_core.as_usize()] = Some(consumer);
                    }
                    ShardConfigMsg::StartConfiguration {
                        requester,
                        queue,
                    } => {

                        let (tx_producer, tx_consumer) = Bridge::create_queues::<Rx, Tx>();
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
                    ShardConfigMsg::Seed(seedr) => {
                        seeder = Some(seedr);
                    }
                    ShardConfigMsg::FinalizeConfiguration(tx) => {

                        // let tx = producers_rx.into_iter().map(Option::unwrap).zip(consumers.into_iter().map(Option::unwrap)).collect::<Vec<_>>();
                        // let rx = producers.into_iter().map(Option::unwrap).zip(consumers_rx.into_iter().map(Option::unwrap)).collect::<Vec<_>>();

                        // producers[core.as_usize()] = 

                        let mut channels: Vec<ShardRoute> = vec![];
                        for i in 0..producers.len() {
                            if i == core.as_usize() {
                                // println!("That's me!");
                                channels.push(ShardRoute::Loopback);
                            } else {
                                channels.push(ShardRoute::Bridge(Bridge::create(producers[i].take().unwrap(), consumers[i].take().unwrap())));
                            }
                            println!("Loading {i}...");
                        }
                        
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

    Ok(ShardInitCtx {
        resolver: notifier.ok_or_else(|| ShardError::ConfigFailure(core))?,
        seed: seeder,
        table: table
    })
}

