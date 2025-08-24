use std::{
    cell::RefCell, ffi::CStr, future::Future, io::{Error, ErrorKind}, mem::zeroed, net::{Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6}, os::fd::RawFd, task::{Poll, Waker}
};

use io_uring::{
    cqueue,
    opcode::{Accept, Bind, Close, Connect, Fsync, Listen, OpenAt, Read, Socket, Write},
    squeue,
    types::{self, Fd},
    IoUring,
};
use nix::libc::{
    in6_addr, in_addr, sockaddr, sockaddr_in, sockaddr_in6, sockaddr_storage, socklen_t, AF_INET, AF_INET6, AT_FDCWD, E2BIG, EAGAIN, EBADF, EFAULT, EINTR, EINVAL, EIO, EISDIR, EWOULDBLOCK
};
use slab::Slab;

use crate::core::io::ring::sealed::IoSeal;

struct RingEntry {
    waker: Waker,
    result: Option<i32>,
}

pub struct IoRingDriver {
    ring: RefCell<IoUring<squeue::Entry, cqueue::Entry>>,
    slab: RefCell<Slab<RingEntry>>,
}

impl IoRingDriver {
    pub fn new(entries: u32) -> std::io::Result<Self> {
        Ok(Self {
            ring: RefCell::new(IoUring::builder().build(entries)?),
            slab: Slab::with_capacity(entries as usize).into(),
        })
    }
    pub fn register(&self, entry: squeue::Entry) -> IoPromise<'_> {
        IoPromise {
            ring: self,
            state: Some(IoPromiseState::Init { entry }),
        }
    }
    fn push(
        &self,
        entry: squeue::Entry,
        waker: Waker,
    ) -> Result<usize, io_uring::squeue::PushError> {
        // unsafe {
        //     self.ring.submission().push(&entry).unwrap();
        // }
        let mut slab = self.slab.borrow_mut();
        let dr = slab.vacant_entry();

        let slot_id = dr.key();

        // let entry = entry.user_data(dr.key() as u64);

        dr.insert(RingEntry {
            waker,
            result: None,
        });

        unsafe {
            self.ring
                .borrow_mut()
                .submission()
                .push(&entry.user_data(slot_id as u64))?
        }
        // println!("Key: {:?}", dr.key());

        // le

        Ok(slot_id)
        // self.slab.vacant_entry()
    }
    fn check_result(&self, index: usize) -> Option<i32> {
        self.slab.borrow()[index].result
    }
    fn remove_entry(&self, index: usize) {
        self.slab.borrow_mut().remove(index);
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
            let entry = slab.get_mut(index as usize).unwrap();
            entry.result = Some(result);
            entry.waker.wake_by_ref();
        }
    }
}

mod sealed {
    pub(crate) trait IoSeal {}
}

pub trait OwnedBuffer: IoSeal {
    fn as_mut_ptr(&mut self) -> *mut u8;
    fn len(&self) -> usize;
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

#[inline]
fn ipv4_to_libc(ipv4: SocketAddrV4) -> sockaddr_in {
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
    let entry = OpenAt::new(types::Fd(AT_FDCWD), path.as_ptr())
        .flags(flags)
        .mode(mode);

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

pub(crate) async fn close(ring: &IoRingDriver, fd: RawFd) -> std::io::Result<()> {
    let entry = Close::new(types::Fd(fd));
    let submission = ring.register(entry.build()).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }
    Ok(())
}

async fn bind_ipv4(
    ring: &IoRingDriver,
    socket: RawFd,
    socket_addr: SocketAddrV4
) -> std::io::Result<()> {
    let address = Box::new(ipv4_to_libc(socket_addr));
    let entry = Bind::new(types::Fd(socket), address.as_ref() as *const _ as *const sockaddr, size_of::<sockaddr_in>() as u32).build();
    let submission = ring.register(entry).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }
    Ok(())
}

async fn bind_ipv6(
    ring: &IoRingDriver,
    socket: RawFd,
    socket_addr: SocketAddrV6
) -> std::io::Result<()> {
    let address = Box::new(ipv6_to_libc(socket_addr));
    let entry = Bind::new(types::Fd(socket), address.as_ref() as *const _ as *const sockaddr, size_of::<sockaddr_in6>() as u32).build();
    let submission = ring.register(entry).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }
    Ok(())
}
pub(crate) async fn bind(
    ring: &IoRingDriver,
    socket: RawFd,
    socket_addr: SocketAddr
) -> std::io::Result<()>{
    match socket_addr {
        SocketAddr::V4(ipv4) => bind_ipv4(ring, socket, ipv4).await,
        SocketAddr::V6(ipv6) => bind_ipv6(ring, socket, ipv6).await
    }
}

pub(crate) async fn listen(
    ring: &IoRingDriver,
    socket: RawFd,
    backlog: i32
    // socket_addr: SocketAddr
) -> std::io::Result<()> {

    let listen = Listen::new(types::Fd(socket), backlog);
    let submission = ring.register(listen.build()).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }
    Ok(())
}

pub(crate) async fn accept(
    ring: &IoRingDriver,
    socket: RawFd,
    flags: i32
) -> std::io::Result<(RawFd, SocketAddr)> {

    let mut storage: Box<sockaddr> = unsafe { Box::new(zeroed()) };
    let mut len: Box<socklen_t> = unsafe { Box::new(zeroed()) };

    let accept = Accept::new(types::Fd(socket), storage.as_mut() as *mut _, len.as_mut() as *mut _)
        .flags(flags);
    let submission = ring.register(accept.build()).await;
    if submission < 0 {
        return Err(Error::from_raw_os_error(-submission));
    }

    if storage.sa_family == AF_INET as u16 {

        let ipv4_storage = unsafe { &*(storage.as_ref() as *const _ as *const sockaddr_in) };

        let ipv4_address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from_bits(ipv4_storage.sin_addr.s_addr.to_be()), ipv4_storage.sin_port.to_be()));

        Ok((submission, ipv4_address))


        // Ok((submission, SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from_bits(u32::from_be_bytes(bytes)), port))))
    } else if storage.sa_family == AF_INET6 as u16 {
        return Err(Error::other("Unsupported protocol family."));
    } else {
        return Err(Error::other("Unsupported protocol family."))
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
    mut buffer: B,
) -> (std::io::Result<usize>, B)
where
    B: OwnedBuffer,
{
    let entry = Write::new(Fd(fd), buffer.as_mut_ptr(), buffer.len() as _);
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

const INIT: u8 = 0;
const READY: u8 = 1;

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
