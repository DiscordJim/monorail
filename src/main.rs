use std::{ffi::CString, io::stdout, net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs}, os::fd::AsRawFd, time::{Duration, SystemTime}};

use io_uring::{cqueue, opcode::{self, Accept, Bind, Connect, Read, Socket}, squeue, types, IoUring, Probe};
use nix::libc::{self, sockaddr, sockaddr_in, AF_INET, EBADF, EFAULT, EINVAL, IPPROTO_TCP, O_CLOEXEC, O_CREAT, O_NONBLOCK, O_RDONLY, SOCK_CLOEXEC, SOCK_NONBLOCK, SOCK_STREAM};
use smol::{future, LocalExecutor, Timer};

use crate::core::{io::ring::{accept, bind, close, connect, ipv4_to_libc, listen, openat, read, socket, timeout, write, IoRingDriver}, topology::MonorailTopology};

pub mod core;

// pub struct TaskPtr

// pub struct ShardRuntime<'a> {
//     executor: &'static StaticLocalExecutor,
//     count: usize,
//     ctx: &'a ShardCtx,
// }

// struct RuntimePtr<T>(NonNull<T>);

// impl<T> RuntimePtr<T> {
//     pub fn allocate(value: T) -> Self {
//         Self(unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(value))) })
//     }
//     pub fn deallocate(self) {
//         unsafe {
//             let _ = Box::from_raw(self.0.as_ptr());
//         }
//     }
// }

// impl<T> Deref for RuntimePtr<T> {
//     type Target = T;
//     fn deref(&self) -> &Self::Target {
//         unsafe { self.0.as_ref() }
//     }
// }

// pub struct Alloc<T: 'static>(&'static str, RuntimePtr<T>);

// impl<T: 'static> Alloc<T> {
//     pub fn new(name: &'static str, value: T) -> Alloc<T> {
//         Alloc(name, RuntimePtr::allocate(value))
//     }
//     pub fn make_foreign(self) -> ForeignPtr<T> {
//         ForeignPtr(sched_getcpu().unwrap(), Some(self))
//     }
// }

// impl<T: 'static> Drop for Alloc<T> {
//     fn drop(&mut self) {
//         println!("Dropping [{}] on core {}", self.0, sched_getcpu().unwrap());
//     }
// }

// impl<T> Deref for Alloc<T> {
//     type Target = T;
//     fn deref(&self) -> &Self::Target {
//         &*self.1
//     }
// }

// pub struct AllocMover<T: 'static>(Alloc<T>);

// unsafe impl<T: 'static> Send for AllocMover<T> {}

// pub struct ForeignPtr<T: 'static>(usize, Option<Alloc<T>>);

// unsafe impl<T: 'static> Send for ForeignPtr<T> {}

// impl<T: 'static> Drop for ForeignPtr<T> {
//     fn drop(&mut self) {

//         let allocation = self.1.take().unwrap();
//         println!(
//             "Dropping foregin [{}] on core {}",
//             allocation.0,
//             sched_getcpu().unwrap()
//         );

//         let allocation = AllocMover(allocation);

//         submit_to(self.0, move |_| {
//             drop(allocation);
//         });
//     }
// }

// impl<T: 'static> Deref for ForeignPtr<T> {
//     type Target = T;
//     fn deref(&self) -> &Self::Target {
//         &*self.1.as_ref().unwrap()
//     }
// }

// thread_local! {
//     static ROUTING_TABLE: UnsafeCell<Option<&'static ShardMapTable>> = const { UnsafeCell::new(None) };
// }

// // static mut ROUTING_TABLE: MaybeUninit<HashMap<usize, UnsafeCell<ShardMapTable>>> =

// // impl<T> !Send for Alloc<T> {}

// // impl<'a> ShardRuntime<'a> {
// //     pub fn submit_to<F>(&self, core: usize, task: F)
// //     where
// //         F: FnOnce(&mut ShardRuntime) + Send + 'static,
// //     {
// //         self.ctx.table.table[core]
// //             .queue
// //             .send(Box::new(task))
// //             .expect("Failed to send task.");
// //     }
// //     pub fn make_foreign<T>(&self, allocation: Alloc<T>) -> ForeignPtr<T> {
// //         ForeignPtr(allocation)
// //     }
// // }

// fn bind_core<F>(core: usize, functor: F) -> nix::Result<()>
// where
//     F: FnOnce(Pid) + Send + 'static,
// {
//     std::thread::spawn(move || {
//         let thread_id = gettid();

//         let mut cpu_set = CpuSet::new();
//         cpu_set.set(core)?;
//         sched_setaffinity(thread_id, &cpu_set)?;

//         functor(thread_id);

//         Ok::<_, nix::Error>(())
//     });
//     Ok(())
// }

// // struct

// // fn get_runtime(&self) -

// fn setup_shard(
//     core: usize,
//     total_count: usize,
// ) -> anyhow::Result<Sender<CoreConfigurationMesssage>> {
//     let (tx, rx) = flume::bounded::<CoreConfigurationMesssage>(50);

//     // let mut table = ShardMapTable {
//     //     table:
//     // };

//     // let mut table =

//     // let tx

//     bind_core(core, move |_| {
//         // let executor = LocalExecutor::new().leak();

//         // let mut table = ShardMapTable {
//         //     table: vec![None; total_count].into_boxed_slice()
//         // };

//         let mut table = vec![None; total_count];
//         // let (stx, srx) = flume::bounded(50);

//         let mut shard_receivers = vec![];

//         loop {
//             match rx.recv() {
//                 Ok(a) => match a {
//                     CoreConfigurationMesssage::ConfigureCore { core, queue } => {
//                         table[core] = Some(queue);
//                     }
//                     CoreConfigurationMesssage::RequestEntry { core, queue } => {
//                         let (tx, rx) = flume::bounded(50);
//                         shard_receivers.push(rx);
//                         let _ = queue.send(tx);
//                     }
//                     CoreConfigurationMesssage::Finalize => {
//                         break;
//                     }
//                 },
//                 Err(e) => panic!("Failed to terminate core setup."),
//             }
//         }

//         // println!("Shard {core} configured.");

//         let raw_table = table
//             .into_iter()
//             .map(|f| ShardMapEntry { queue: f.unwrap() })
//             .collect::<Box<[_]>>();

//         let table = Box::leak(Box::new(ShardMapTable { table: raw_table }));

//         let context = ShardCtx {
//             id: core,
//             table: table,
//         };

//         ROUTING_TABLE.with(|f| {
//             unsafe { *f.get() = Some(table) };
//         });

//         let executor = LocalExecutor::new().leak();

//         let mut runtime = Box::leak(Box::new(ShardRuntime {
//             executor: executor,
//             count: 0,
//             ctx: &context,
//         }));

//         future::block_on(executor.run(async {
//             for receiver in shard_receivers {
//                 unsafe {
//                     executor
//                         .spawn_scoped(async {
//                             let rcv = Box::leak(Box::new(receiver));
//                             loop {
//                                 let Ok(message) = rcv.recv_async().await else {
//                                     panic!("Cross core queue has passed away.");
//                                 };

//                                 message(&mut runtime);
//                             }
//                         })
//                         .detach();
//                 }
//                 // The receiver needs to be forgot here or soemthing.
//             }

//             loop {
//                 Timer::after(Duration::from_secs(4)).await;
//             }
//         }));
//     })?;

//     Ok(tx)
// }

// pub enum CoreConfiguration {}

// pub enum CoreConfigurationMesssage {
//     ConfigureCore {
//         core: usize,
//         queue: Sender<Task>,
//     },
//     RequestEntry {
//         core: usize,
//         queue: Sender<Sender<Task>>,
//     },
//     Finalize,
// }

// struct ShardMapTable {
//     table: Box<[ShardMapEntry]>,
// }

// struct ShardMapEntry {
//     queue: Sender<Task>,
// }

// fn start_topology<F>(cores: usize, functor: F) -> Result<()>
// where
//     F: FnOnce(&mut ShardRuntime) + Send + 'static,
// {
//     let seeder = setup_topology(cores)?;

//     seeder
//         .send(Box::new(functor))
//         .map_err(|_| anyhow!("Failed to seed the runtime topology."))?;

//     park();

//     Ok(())
// }

// fn jump_task(runtime: &mut ShardRuntime<'_>) {
//     let current = sched_getcpu().unwrap();
//     println!("Good morning! I am Core {current}.");
//     if current == 15 {
//     } else {
//         println!("Sending to {}.", current + 1);
//         submit_to(current + 1, jump_task);
//     }
// }

fn main() {
    // unsafe {
    //     sched_setaffinity(pid, cpusetsize, cpuset)
    // }

   
    let executor = LocalExecutor::new().leak();

    future::block_on(executor.run(async {
       
         let driver = Box::leak(Box::new(IoRingDriver::new(8).unwrap()));

        executor.spawn({
            // let dref = &driver;
            async {
                loop {
                    driver.drive();
                    // println!("hello...");

                    Timer::after(Duration::from_micros(50)).await;
                }
            }
        }).detach();


        // EBADF

        println!("advancing...");

        println!("Supports IORING BIND: {}", driver.supports_bind());

        // EFAULT
        // EINVAL

        // read(&driver, -1, vec![]).await.0.unwrap();

        // bind(&driver, -1, "0.0.0.0:69".to_socket_addrs().unwrap().next().unwrap()).await.unwrap();
        // let socket = socket(&driver, A, socket_type, protocol)


        // let stdout = stdout();
        // let fs = stdout.as_raw_fd();

        // write(&driver, fs, "get hecked!\n".as_bytes().to_vec()).await.0.unwrap();


        // let time = SystemTime::now();

        // timeout(&driver, Duration::from_secs(5)).await.unwrap();
        // let elapsed = time.elapsed().unwrap();
        // println!("Elapsed: {:?}", elapsed);
        let socket = socket(&driver, AF_INET, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, IPPROTO_TCP).await.unwrap();
        println!("Socket: {socket}");
        // // EINVAL

        
        // println!("Support: {}", Probe::new().is_supported(Socket::CODE));


        // let ie = ipv4_to_libc(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 6942));


        // let bruh = unsafe { libc::bind(socket, &ie as *const _ as *const sockaddr, size_of::<sockaddr_in>() as u32 ) };
        // println!("bruh: {:?}", bruh);
        bind(&driver, socket, SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 6942))).await.unwrap();
        // let listen = unsafe { libc::listen(socket, 1000) };
        // println!("listen: {listen}");
        listen(&driver, socket, 1000).await.unwrap();

        let accept = accept(&driver, socket, SOCK_CLOEXEC | SOCK_NONBLOCK).await.unwrap();

        println!("Accepted: {:?}", accept);

        // let mut buffer = vec![0; 128];

        // let (r, o) = read(&driver, accept.0, buffer).await;
        
        // println!("{o:?}");

        // let addy = "0.0.0.0:6969".to_socket_addrs().unwrap().next().unwrap();
        // println!("Addy: {:?}", addy);

        // let connect = connect(&driver, socket, "127.0.0.1:6969".to_socket_addrs().unwrap().next().unwrap()).await.unwrap();

        // write(&driver, socket, "hello bruh".as_bytes().to_vec()).await.0.unwrap();

        // let mut returnbuf = vec![0; 32];
        // let (r, bs) = read(&driver, socket, returnbuf).await;

        // let length = r.unwrap();
        // println!("Bs: {:?}", std::str::from_utf8(&bs[..length]));

        // write

        // let mut ring = IoUring::<squeue::Entry, cqueue::Entry>::builder()
        //     // .setup_sqpoll(idle)
        //     .build(8).unwrap();

        // let fd = openat(&driver, CString::new("README.md").unwrap().as_c_str(), O_RDONLY, 0).await.unwrap();

        // // close(&driver, fd).await.unwrap();

        // let bytes = read(&driver, fd, vec![0; 24]).await.unwrap();

        // println!("FD: {bytes:?}");

        // let fd = std::fs::File::open("README.md").unwrap();

        // let mut buf = vec![0;1024];

        // let buffer = read(&driver, fd.as_raw_fd(), buf).await.unwrap();

        // // let write = opcode::Read::new(types::Fd(fd.as_raw_fd()), buf.as_mut_ptr(), buf.len() as _).build();


        // // // EBADF
        // // let entity = driver.register(write).await;
        // println!("Entity: {buffer:?}");
        // println!("{:?}")
        // let mut buf = vec![0; 1024];

        // let read_e =
        //     opcode::Read::new(types::Fd(fd.as_raw_fd()), buf.as_mut_ptr(), buf.len() as _).build();

        // let entity = driver.register(read_e).await;
        // println!("Read {entity} bytes.");

        // driver.push(read_e).unwrap();
    }));

    // println!("{:?}", )

    // // Note that the developer needs to ensure
    // // that the entry pushed into submission queue is valid (e.g. fd, buffer).
    // unsafe {
    //     ring.submission()
    //         .push(&read_e)
    //         .expect("submission queue is full");
    // }

    // ring.submit_and_wait(1).unwrap();

    // let cqe = ring.completion().next().expect("completion queue is empty");

    // assert_eq!(cqe.user_data(), 0x42);
    // assert!(cqe.result() >= 0, "read error: {}", cqe.result());

    // println!("READ E: {:?} {}", buf, cqe.result());

    // MonorailTopology::setup(|runtime| {
    //     println!("I'm speaking to you from the Monorail Runtime.");

    // }).unwrap();

    // let mut handles = vec![];

    // for (pos, identifier) in ["A", "B"].iter().enumerate() {
    //     setup_shard(pos).unwrap();
    // }

    // let cores = num_cpus::get();
    // println!("Detected {cores} cores...");

    // start_topology(cores, |runtime| {
    //     println!("Hello, I'm on the monorail runtime!");
    //     println!("I'm running on Core {}.", sched_getcpu().unwrap());

    //     let str = Alloc::new("hello", 12).make_foreign();
    //     // let str = runtime.make_foreign(str);

    //     submit_to(1, move |rt| {
    //         println!("Hello, I have received this number: {}", *str);
    //     });

    //     jump_task(runtime);

    //     // runtime.submit_to(1, |rt| {
    //     //     println!("Good morning from core {}", sched_getcpu().unwrap());
    //     // });
    //     // jump_task(runtime);
    // })
    // .unwrap();
    // std::thread::sleep(Duration::from_secs(60000));

    // let mut cpu_set = CpuSet::new();
    // cpu_set.set(0).unwrap();
    // sched_setaffinity(pid, &cpu_set).unwrap();

    // for i in 0..5 {
    //     println!("(A) Hello {i} -> Running on {cpu}");
    //     std::thread::sleep(Duration::from_secs(1));
    //     sched_yield().unwrap();
    // }

    // println!("Hello, world! {pid} {cpu}");
}
