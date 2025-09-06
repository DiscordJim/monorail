use std::{
    cell::RefCell, ffi::CStr, future::Future, io::{Error, ErrorKind}, mem::zeroed, net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6}, os::fd::RawFd, ptr::NonNull, sync::atomic::{AtomicU8, Ordering}, task::{Poll, Waker}, time::Duration
};

use io_uring::{
    cqueue,
    opcode::{
        self, Accept, Bind, Close, Connect, Fsync, Listen, OpenAt, Read, Shutdown, Socket, Timeout, Write
    },
    squeue::{self, PushError},
    types::{self, Fd, Timespec},
    IoUring,
};
use nix::{
    libc::{
        in6_addr, in_addr, sockaddr, sockaddr_in, sockaddr_in6, socklen_t, AF_INET, AF_INET6, AT_FDCWD, EINVAL
    }, poll::PollFlags, sys::socket::SockaddrIn6
};
use slab::Slab;

use crate::core::io::ring::sealed::IoSeal;


#[derive(Debug)]
struct RingEntry {
    waker: RingDirective,
    result: Option<i32>,
}

pub struct IoRingDriver {
    ring: RefCell<IoUring<squeue::Entry, cqueue::Entry>>,
    slab: RefCell<Slab<RingEntry>>,
    support: IoRingSupport,
}
#[derive(Debug)]
enum RingDirective {
    Waker(Waker),
    Claim
}

struct IoRingSupport {
    has_bind: bool,
    // has_checked_bind: bool,
    has_listen: bool,
}

#[inline]
fn perform_compatibility_checks(ring: &mut IoUring) -> std::io::Result<()> {
    if IORING_BIND_SUPPORT.load(Ordering::Relaxed) == 0 {
        
        let addr = sockaddr_in {
            sin_addr: in_addr { s_addr: 0 },
            sin_family: 0,
            sin_port: 0,
            sin_zero: [0u8; 8]
        };

        // submit an invalid 
        let bind = Bind::new(types::Fd(-1), &addr as *const _ as *const sockaddr, size_of::<sockaddr_in>() as u32)
            .build();
       
        unsafe {
            ring.submission().push(&bind).map_err(|_| Error::other("Failed to submit to submission queue."))?;
        }

        ring.submit_and_wait(1)?;

        let event = ring.completion().next().ok_or_else(|| Error::other("Failed to poll the bind event from the queue during compat check."))?;

        if event.result() == -EINVAL {
            IORING_BIND_SUPPORT.store(1, Ordering::Relaxed);
        } else {
            IORING_BIND_SUPPORT.store(2, Ordering::Relaxed);
        }
    }
    if IORING_LISTEN_SUPPORT.load(Ordering::Relaxed) == 0 {
        let listen = Listen::new(types::Fd(-1), 1000).build();
         unsafe {
            ring.submission().push(&listen).map_err(|_| Error::other("Failed to submit to submission queue."))?;
        }

        ring.submit_and_wait(1)?;

        let event = ring.completion().next().ok_or_else(|| Error::other("Failed to poll the bind event from the queue during compat check."))?;

        if event.result() == -EINVAL {
            IORING_LISTEN_SUPPORT.store(1, Ordering::Relaxed);
        } else {
            IORING_LISTEN_SUPPORT.store(2, Ordering::Relaxed);
        }
    }

    Ok(())
}

impl IoRingDriver {
    pub fn new(entries: u32) -> std::io::Result<Self> {
        // static TEST_ADDR: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
        
        
        let mut ring = IoUring::builder()
            // .setup_sqpoll(2000)
            .build(entries)?;

        // First we will check compatibility.
        // println!("Running compat check.");
        perform_compatibility_checks(&mut ring)?;
 // ring.submission().push(&)
        
        let object = Self {
            ring: RefCell::new(ring),
            slab: Slab::with_capacity(entries as usize).into(),
            support: IoRingSupport {
                has_bind: IORING_BIND_SUPPORT.load(Ordering::Relaxed) == 2,
                // has_checked_bind: false,
                has_listen: IORING_LISTEN_SUPPORT.load(Ordering::Relaxed) == 2,
            },
        };

        // let ring= 

        // object.support.has_bind = check_bind(&object).await?;
        // match bind(&object, -1, TEST_ADDR).await {
        //     Ok(_) => {
        //         return Err(Error::other("Bound to file descriptor -1, which is almost certainly an error."));
        //     },
        //     Err(e) => {
        //         if e.kind() != ErrorKind::InvalidInput {

        //         }
        //     }
        // }

        Ok(object)
    }
    pub fn supports_bind(&self) -> bool {
        self.support.has_bind
    }
    pub fn supports_ioring_listen(&self) -> bool {
        self.support.has_listen
    }
    pub fn register_nowait(&self, entry: squeue::Entry) -> Result<usize, PushError> {
        self.insert_claim(entry, RingDirective::Claim)
    }
    pub fn register(&self, entry: squeue::Entry) -> IoPromise<'_> {
        IoPromise {
            ring: self,
            state: Some(IoPromiseState::Init { entry }),
        }
    }

    pub fn submit(&self) {
        self.ring.borrow_mut().submit().unwrap();
    }
    fn insert_claim(
        &self,
        entry: squeue::Entry,
        directive: RingDirective
    ) -> Result<usize, io_uring::squeue::PushError> {
        let mut slab = self.slab.borrow_mut();
        let dr = slab.vacant_entry();
        let slot_id = dr.key();

        dr.insert(RingEntry {
            waker: directive,
            result: None,
        });

        unsafe {
            self.ring
                .borrow_mut()
                .submission()
                .push(&entry.user_data(slot_id as u64))?
        }
        // println!("Key: {:?} {}", slab, self.ring.borrow_mut().submission().len());

        // le

        Ok(slot_id)
    }

    fn push(
        &self,
        entry: squeue::Entry,
        waker: Waker,
    ) -> Result<usize, io_uring::squeue::PushError> {
        self.insert_claim(entry, RingDirective::Waker(waker))
    }
    fn check_result(&self, index: usize) -> Option<i32> {
        self.slab.borrow()[index].result
    }
    fn remove_entry(&self, index: usize) {
        self.slab.borrow_mut().remove(index);
    }
    pub fn len(&self)->usize{
        self.ring.borrow_mut().submission().len()
    }
    /// This drives the driver forward, waking up any pending tasks.
    pub fn drive(&self) {
        let mut ring = self.ring.borrow_mut();
        let mut slab = self.slab.borrow_mut();
        let _ = ring.submitter().submit();

        let mut cqe = ring.completion();
        while let Some(comp) = cqe.next() {
            let result = comp.result();
            let index = comp.user_data();
            // if index == UD_EIN {
            //     continue;

            // println!("WAKING.... {:?}", comp);
            // }
            let entry = slab.get_mut(index as usize).unwrap();
            entry.result = Some(result);
            match &entry.waker {
                RingDirective::Waker(wkr) => wkr.wake_by_ref(),
                RingDirective::Claim => { /* Do nothing */ }
            }
            // entry.waker.wake_by_ref();
        }
    }
    pub fn sub_and_wait(
        &self
    ) -> std::io::Result<usize>{

        self.ring.borrow().submit_and_wait(1)
        // let ringed = self.ring.borrow_mut();
        // // ringed.submit_and_wait(want)
        // let a = unsafe { self.ring.borrow().submit_and_wait(1) };
        // println!("Waited: {a:?}");
    }
}

mod sealed {
    pub(crate) trait IoSeal {}
}

pub trait OwnedBuffer: IoSeal {
    fn as_mut_ptr(&mut self) -> *mut u8;
    fn len(&self) -> usize;
}

pub trait OwnedReadBuf: IoSeal {
    fn as_ptr(&self) -> *const u8;
    fn len(&self) -> usize;
}

impl IoSeal for &'static str {}

impl OwnedReadBuf for &'static str {
    fn as_ptr(&self) -> *const u8 {
        self.as_bytes().as_ptr()
    }
    fn len(&self) -> usize {
        self.as_bytes().len()
    }
}

impl IoSeal for Vec<u8> {}

impl OwnedBuffer for Vec<u8> {
    fn as_mut_ptr(&mut self) -> *mut u8 {
        Vec::as_mut_ptr(self)
    }
    fn len(&self) -> usize {
        Vec::len(self)
    }
}

impl IoSeal for Box<[u8]> {}

impl OwnedBuffer for Box<[u8]> {
    fn as_mut_ptr(&mut self) -> *mut u8 {
        <[_]>::as_mut_ptr(self)
    }
    fn len(&self) -> usize {
        <[_]>::len(self)
    }
}

pub(crate) async fn socket(
    ring: &IoRingDriver,
    domain: i32,
    socket_type: i32,
    protocol: i32,
    // flags: i32
) -> std::io::Result<RawFd> {
    let socket = Socket::new(domain, socket_type, protocol);
    let submission = ring.register(socket.build()).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }

    Ok(submission)
}

async fn connect_ipv4(
    ring: &IoRingDriver,
    socket: RawFd,
    addr: SocketAddrV4,
) -> std::io::Result<()> {
    let address = Box::new(ipv4_to_libc(addr));

    let connect = Connect::new(
        types::Fd(socket),
        address.as_ref() as *const sockaddr_in as *const sockaddr,
        size_of::<sockaddr_in>() as u32,
    );

    let submission = ring.register(connect.build()).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }

    Ok(())
}

// #[inline]
// fn with_addr_conversion<F>(addr: SocketAddr, functor: F)
// where
//     F: FnOnce(*const sockaddr, u32) -> Box<>

// fn has_ioring_bind() -> bool {
//     static
// }

#[inline]
pub fn ipv4_to_libc(ipv4: SocketAddrV4) -> sockaddr_in {
    sockaddr_in {
        sin_family: AF_INET as u16,
        sin_addr: in_addr {
            s_addr: ipv4.ip().to_bits().to_be(),
        },
        sin_port: ipv4.port().to_be(),
        sin_zero: [0; 8],
    }
}

#[inline]
fn ipv6_to_libc(ipv6: SocketAddrV6) -> sockaddr_in6 {
    sockaddr_in6 {
        sin6_addr: in6_addr {
            s6_addr: ipv6.ip().octets(),
        },
        sin6_family: AF_INET6 as u16,
        sin6_port: ipv6.port().to_be(),
        sin6_flowinfo: ipv6.flowinfo(),
        sin6_scope_id: ipv6.scope_id(),
    }
}

pub static IORING_BIND_SUPPORT: AtomicU8 = AtomicU8::new(0);
pub static IORING_LISTEN_SUPPORT: AtomicU8 = AtomicU8::new(0);

#[inline]
async fn check_bind(ring: &IoRingDriver) -> std::io::Result<bool> {
    // static DUMMY_IP: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));

    let value = IORING_BIND_SUPPORT.load(std::sync::atomic::Ordering::Relaxed);

    if value == 0 {
        match uring_bind_ipv4(
            ring,
            -1,
            &sockaddr_in {
                sin_family: AF_INET as u16,
                sin_port: 0,
                sin_addr: in_addr { s_addr: 0 },
                sin_zero: [0u8; 8],
            },
        )
        .await
        {
            Ok(_) => {
                return Err(Error::other(
                    "We were able to bind to the file descriptor -1. This is weird.",
                ));
            }
            Err(e) => {
                // println!("error: {e:?}");
                let result = if e.kind() == ErrorKind::InvalidInput {
                    1
                } else {
                    2
                };

                IORING_BIND_SUPPORT.store(result, Ordering::Relaxed);

                Ok(result == 2)
            }
        }
    } else {
        Ok(value == 2)
    }
}

async fn connect_ipv6(
    ring: &IoRingDriver,
    socket: RawFd,
    ipv6: SocketAddrV6,
) -> std::io::Result<()> {
    let address = Box::new(ipv6_to_libc(ipv6));

    let connect = Connect::new(
        types::Fd(socket),
        address.as_ref() as *const sockaddr_in6 as *const sockaddr,
        size_of::<sockaddr_in6>() as u32,
    );

    let submission = ring.register(connect.build()).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }

    Ok(())
}

pub(crate) async fn connect(
    ring: &IoRingDriver,
    socket: RawFd,
    addr: SocketAddr,
) -> std::io::Result<()> {
    match addr {
        SocketAddr::V4(v4) => connect_ipv4(ring, socket, v4).await,
        SocketAddr::V6(v6) => connect_ipv6(ring, socket, v6).await,
    }
}

pub(crate) async fn openat(
    ring: &IoRingDriver,
    path: &CStr,
    flags: i32,
    mode: u32,
) -> std::io::Result<RawFd> {
    // let bytes = Box::new(x)
    let entry = OpenAt::new(types::Fd(AT_FDCWD), path.as_ptr())
        .flags(flags)
        .mode(mode);

    // println!("Submitting entry...");

    let submission = ring.register(entry.build()).await;
    if submission <= 0 {
        return Err(Error::from_raw_os_error(-submission));
    }

    Ok(submission)
}

pub(crate) async fn fsync(ring: &IoRingDriver, fd: RawFd) -> std::io::Result<()> {
    let entry = Fsync::new(types::Fd(fd));
    let submission = ring.register(entry.build()).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    } else {
        Ok(())
    }
}

pub(crate) fn install_polladd_multi(
    ring: &IoRingDriver,
    fd: RawFd
) -> Result<usize, PushError> {

    let entry = opcode::PollAdd::new(types::Fd(fd), PollFlags::POLLIN.bits() as u32)
        .multi(true)
        .build();

    let claim = ring.register_nowait(entry);

    claim

}

pub(crate) async fn polladd(
    ring: &IoRingDriver,
    fd: RawFd
) -> std::io::Result<()> {
    let entry = opcode::PollAdd::new(types::Fd(fd), 0)
        .multi(true)
        .build();
    let submission = ring.register(entry).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }
    Ok(())
}

pub(crate) async fn close(ring: &IoRingDriver, fd: RawFd) -> std::io::Result<()> {
    let entry = Close::new(types::Fd(fd));
    let submission = ring.register(entry.build()).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }
    Ok(())
}

async fn uring_bind_ipv6(
    ring: &IoRingDriver,
    socket: RawFd,
    address: &Box<sockaddr_in6>
) -> std::io::Result<()> {

    
    let entry = Bind::new(
        types::Fd(socket),
        address.as_ref() as *const _ as *const sockaddr,
        size_of::<sockaddr_in6>() as socklen_t,
    )
    .build();
    println!("Built da thing");
    let submission = ring.register(entry).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }
    Ok(())
}

async fn bind_ipv6(
    ring: &IoRingDriver,
    socket: RawFd,
    socket_addr: SocketAddrV6,
) -> std::io::Result<()> {
    let address = Box::new(ipv6_to_libc(socket_addr));

    if ring.supports_bind() {
        uring_bind_ipv6(ring, socket, &address).await?;
    } else {
        nix::sys::socket::bind(socket, &SockaddrIn6::from(*address))?;
    }
    
    Ok(())
}

async fn bind_ipv4(
    ring: &IoRingDriver,
    socket: RawFd,
    socket_addr: SocketAddrV4,
) -> std::io::Result<()> {
    let address = Box::new(ipv4_to_libc(socket_addr));

    if ring.supports_bind() {
        // println!("binding...");
        uring_bind_ipv4(ring, socket, &*address).await?;
    } else {
        // getsock
        println!("HELLO");
        unsafe {
            nix::libc::bind(
                socket,
                address.as_ref() as *const _ as *const sockaddr,
                size_of::<sockaddr_in>() as u32,
            );
        }
    }

    println!("ADDY: {:?}", address);

    Ok(())
}

#[inline]
async fn uring_bind_ipv4(
    ring: &IoRingDriver,
    socket: RawFd,
    addr: &sockaddr_in,
) -> std::io::Result<()> {
    let entry = Bind::new(
        types::Fd(socket),
        addr as *const _ as *const sockaddr,
        size_of::<sockaddr_in>() as u32,
    )
    .build();
    let submission = ring.register(entry).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }
    Ok(())
}


pub(crate) async fn bind(
    ring: &IoRingDriver,
    socket: RawFd,
    socket_addr: SocketAddr,
) -> std::io::Result<()> {
    // if !ring.support.has_checked_bind {
    //     ring.support.has_bind = check_bind(ring).await?;
    //     ring.support.has_checked_bind = true;
    // }
    match socket_addr {
        SocketAddr::V4(ipv4) => bind_ipv4(ring, socket, ipv4).await,
        SocketAddr::V6(ipv6) => bind_ipv6(ring, socket, ipv6).await,
    }
}



pub(crate) async fn shutdown(ring: &IoRingDriver, socket: RawFd, how: i32) -> std::io::Result<()> {
    let shutdown = Shutdown::new(types::Fd(socket), how).build();
    let submision = ring.register(shutdown).await;
    if submision < 0 {
        return Err(Error::from_raw_os_error(-submision));
    }
    Ok(())
}

pub struct Claim<'a, T> {
    ring: &'a IoRingDriver,
    idx: usize,
    spec: NonNull<T>
}

impl<'a, T> Drop for Claim<'a, T> {
    fn drop(&mut self) {
        unsafe {
            let _ = Box::from_raw(self.spec.as_ptr());
        }
    }
}

// pub enum ClaimResult {}

#[inline]
fn convert_error(descriptor: i32) -> std::io::Result<i32> {
    if descriptor < 0 {
        Err(std::io::Error::from_raw_os_error(-descriptor))
    } else {
        Ok(descriptor)
    }
}

impl<'a, T> Claim<'a, T> {
    // fn new()
    pub fn check(self) -> Result<std::io::Result<i32>, Self> {
        let mut slab_ref = self.ring.slab.borrow_mut();
        // let slot = slab_ref[]
        if let Some(result) = slab_ref[self.idx].result.take() {
            slab_ref.remove(self.idx);
            return Ok(convert_error(result));
        } else {
            return Err(self);
        }
        // match self.ring.slab.bor
    }
}

/// Puts a timeout in the ring without returning a future.
pub(crate) fn install_timeout(
    ring: &IoRingDriver,
    duration: Duration
) -> Result<Claim<'_, Timespec>, PushError> {
    let spec = Box::into_raw(Box::new(Timespec::new()
        .sec(duration.as_secs())
        .nsec(duration.subsec_nanos())));
    let timeout = Timeout::new(spec as *mut _ as *const _).build();
    // ring.register_nowait(timeout)
    Ok(Claim {
        ring: ring,
        idx: ring.register_nowait(timeout)?,
        spec: unsafe { NonNull::new_unchecked(spec) }
    })
    // Ok(())
}

pub(crate) async fn timeout(ring: &IoRingDriver, duration: Duration) -> std::io::Result<()> {
    let spec = Timespec::new()
        .sec(duration.as_secs())
        .nsec(duration.subsec_nanos());
    let timeout = Timeout::new(&spec as *const _).build();
    let submission = ring.register(timeout).await;
    Ok(())
}

pub(crate) async fn listen(
    ring: &IoRingDriver,
    socket: RawFd,
    backlog: i32, // socket_addr: SocketAddr
) -> std::io::Result<()> {
    if ring.supports_ioring_listen() {
         let listen = Listen::new(types::Fd(socket), backlog);
    let submission = ring.register(listen.build()).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }
    } else {

        // nix::sys::socket::listen(socket, backlog)
        let status = unsafe {
            nix::libc::listen(socket, backlog)
        };
        if status < 0 {
            return Err(Error::from_raw_os_error(-status));
        }
        
    }
   
    Ok(())
}

pub(crate) async fn accept(
    ring: &IoRingDriver,
    socket: RawFd,
    flags: i32,
) -> std::io::Result<(RawFd, SocketAddr)> {
    let mut storage: Box<sockaddr> = unsafe { Box::new(zeroed()) };
    let mut len: Box<socklen_t> = unsafe { Box::new(zeroed()) };

    let accept = Accept::new(
        types::Fd(socket),
        storage.as_mut() as *mut _,
        len.as_mut() as *mut _,
    )
    .flags(flags);
    let submission = ring.register(accept.build()).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }

    if *len as usize == size_of::<sockaddr_in>() {
        let ipv4_storage = unsafe { &*(storage.as_ref() as *const _ as *const sockaddr_in) };

        let ipv4_address = SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::from_bits(ipv4_storage.sin_addr.s_addr.to_be()),
            ipv4_storage.sin_port.to_be(),
        ));

        Ok((submission, ipv4_address))

        // Ok((submission, SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from_bits(u32::from_be_bytes(bytes)), port))))
    } else if *len as usize == size_of::<sockaddr_in6>() {
        let ipv6_storage = unsafe { &*(storage.as_ref() as *const _ as *const sockaddr_in6) };
        let ipv6_address = SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from(ipv6_storage.sin6_addr.s6_addr), u16::from_be(ipv6_storage.sin6_port), u32::from_be(ipv6_storage.sin6_flowinfo), u32::from_be(ipv6_storage.sin6_scope_id)));


        return Ok((submission, ipv6_address));
    } else {
        println!("protocol family: {}", storage.sa_family);
        return Err(Error::other("Unsupported protocol family."));
    }

    // Ok(submission)
}

pub(crate) async fn read<B>(
    ring: &IoRingDriver,
    fd: RawFd,
    mut buffer: B,
) -> (std::io::Result<usize>, B)
where
    B: OwnedBuffer,
{
    let entry = Read::new(Fd(fd), buffer.as_mut_ptr(), buffer.len() as _);
    let submission = ring.register(entry.build()).await;
    if submission <= 0 {
        return (Err(Error::from_raw_os_error(-submission)), buffer);
    }

    (Ok(submission as usize), buffer)
}

pub(crate) async fn write<B>(
    ring: &IoRingDriver,
    fd: RawFd,
    buffer: B,
) -> (std::io::Result<usize>, B)
where
    B: OwnedReadBuf,
{
    let entry = Write::new(Fd(fd), buffer.as_ptr(), buffer.len() as _);
    let submission = ring.register(entry.build()).await;
    if submission <= 0 {
        return (Err(Error::from_raw_os_error(-submission)), buffer);
    }

    (Ok(submission as usize), buffer)
}

// pub enum

// impl IoRingDriver {
//     fn read_raw(&self,)
// }


enum IoPromiseState {
    /// The promise has not yet submitted
    /// to the SQE.
    Init { entry: squeue::Entry },
    /// The promise is waiting to be woken up
    /// by the CQE.
    Waiting { index: usize },
}

pub struct IoPromise<'a> {
    ring: &'a IoRingDriver,
    state: Option<IoPromiseState>,
}

impl<'a> Future for IoPromise<'a> {
    type Output = i32;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match self.state.take().unwrap() {
            IoPromiseState::Init { entry } => {
                // First, we submit the task.
                let index = self.ring.push(entry, cx.waker().to_owned()).unwrap();

                self.state = Some(IoPromiseState::Waiting { index });

                Poll::Pending
            }
            IoPromiseState::Waiting { index } => {
                if let Some(result) = self.ring.check_result(index) {
                    self.ring.remove_entry(index);
                    self.state = Some(IoPromiseState::Waiting { index });
                    Poll::Ready(result)
                } else {
                    self.state = Some(IoPromiseState::Waiting { index });
                    Poll::Pending
                }
            }
        }
    }
}

// pub struct FileReadFut<'a> {
//     ring: &'a IoRingDriver,
//     request
// }
