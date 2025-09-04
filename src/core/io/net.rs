use std::{
    io::ErrorKind,
    net::{SocketAddr, SocketAddrV4, SocketAddrV6, ToSocketAddrs},
    os::fd::RawFd,
};

use nix::{
    libc::{
        AF_INET, AF_INET6,
        IPPROTO_IP, SOCK_CLOEXEC, SOCK_NONBLOCK, SOCK_STREAM,
    },
    sys::socket::{AddressFamily, SockaddrLike, SockaddrStorage},
};

use crate::core::io::ring::{
    accept, bind, connect, listen, read, socket, write, IoRingDriver, OwnedBuffer, OwnedReadBuf,
};

pub struct TcpStream<'a> {
    ring: &'a IoRingDriver,
    socket: RawFd,
}

impl<'a> TcpStream<'a> {
    pub async fn connect(
        ring: &'a IoRingDriver,
        address: impl ToSocketAddrs,
    ) -> std::io::Result<Self> {
        match address
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| std::io::Error::other("Failed to convert socket adresses."))?
        {
            SocketAddr::V4(v4addr) => {
                let sock = create_socket_ipv4(ring).await?;
                connect(ring, sock, SocketAddr::V4(v4addr)).await?;
                Ok(TcpStream { ring, socket: sock })
            }
            SocketAddr::V6(v6addr) => {
                let sock = create_socket_ipv6(ring).await?;
                connect(ring, sock, SocketAddr::V6(v6addr)).await?;
                Ok(TcpStream { ring, socket: sock })
            }
        }
    }
    #[inline]
    pub async fn write<B>(&mut self, buf: B) -> (std::io::Result<usize>, B)
    where
        B: OwnedReadBuf,
    {
        write(self.ring, self.socket, buf).await
    }
    pub async fn write_all<B>(&mut self, mut buf: B) -> (std::io::Result<usize>, B)
    where
        B: OwnedReadBuf,
    {
        let mut written = 0;
        let to_write = buf.len();
        while written < to_write {
            let (sub_written, sbuf) = self.write(buf).await;
            buf = sbuf;
            match sub_written {
                Ok(val) => written += val,
                Err(e) => {
                    return (Err(e), buf);
                }
            }
        }
        (Ok(written), buf)
    }
    #[inline]
    pub async fn read<B>(&mut self, buf: B) -> (std::io::Result<usize>, B)
    where
        B: OwnedBuffer,
    {
        read(self.ring, self.socket, buf).await
    }
}

pub struct TcpListener<'a> {
    ring: &'a IoRingDriver,
    fd: RawFd,
}

#[inline]
async fn create_socket_ipv4(ring: &IoRingDriver) -> std::io::Result<RawFd> {
    socket(
        ring,
        AF_INET,
        SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC,
        IPPROTO_IP,
    )
    .await
}

#[inline]
async fn create_socket_ipv6(ring: &IoRingDriver) -> std::io::Result<RawFd> {
    socket(
        ring,
        AF_INET6,
        SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC,
        IPPROTO_IP,
    )
    .await
}

impl<'a> TcpListener<'a> {
    pub async fn bind(ring: &'a IoRingDriver, addr: impl ToSocketAddrs) -> std::io::Result<Self> {
        // let socket = socket(ring, domain, socket_type, protocol)
        let addy = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| std::io::Error::other("Failed to get address.."))?;
        // println!("Requesing: {:?}", addy);
        match addy {
            SocketAddr::V4(v4) => {
                let socket_fd = create_socket_ipv4(ring).await?;
                bind(ring, socket_fd, SocketAddr::V4(v4)).await?;
                listen(ring, socket_fd, 1024).await?;
                Ok(TcpListener {
                    ring,
                    fd: socket_fd,
                })
            }
            SocketAddr::V6(v6) => {
                let socket_fd = create_socket_ipv6(ring).await?;
                bind(ring, socket_fd, SocketAddr::V6(v6)).await?;
                listen(ring, socket_fd, 1024).await?;
                Ok(TcpListener {
                    ring,
                    fd: socket_fd,
                })
            }
        }
    }
    pub async fn accept(&self) -> std::io::Result<(TcpStream<'_>, SocketAddr)> {
        let s = accept(self.ring, self.fd, SOCK_CLOEXEC | SOCK_NONBLOCK).await?;

        Ok((
            TcpStream {
                ring: self.ring,
                socket: s.0,
            },
            s.1,
        ))
    }
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        let address: SockaddrStorage = nix::sys::socket::getsockname(self.fd).unwrap();
        match address
            .family()
            .ok_or_else(|| std::io::Error::other("Could not get family."))?
        {
            AddressFamily::Inet => {
                let address = address.as_sockaddr_in().cloned().unwrap();
                return Ok(SocketAddr::V4(SocketAddrV4::new(
                    address.ip(),
                    address.port(),
                )));
            }
            AddressFamily::Inet6 => {
                let address = address.as_sockaddr_in6().unwrap();
                return Ok(SocketAddr::V6(SocketAddrV6::new(
                    address.ip(),
                    address.port(),
                    address.flowinfo(),
                    address.scope_id(),
                )));
            }
            x => Err(std::io::Error::new(
                ErrorKind::Unsupported,
                format!("Unknown socket family: {x:?}"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{Ipv6Addr, SocketAddrV6, TcpListener},
    };

    use anyhow::anyhow;
    use flume::unbounded;
    

    use crate::core::{executor::scheduler::Executor, io::net::TcpStream};

    enum IpTestPacket {
        Addr(std::net::SocketAddr),
    }

    // fn raw_net_setup<F>(
    //     sync_portion
    // )

    // fn setup_tcp_connector<F>(

    // )

    #[test]
    pub fn test_tcp_connector() {
        let (tx, rx) = unbounded::<IpTestPacket>();

        let handle = std::thread::spawn({
            move || {
                let tcp = TcpListener::bind("0.0.0.0:0")?;

                tx.send(IpTestPacket::Addr(tcp.local_addr()?))?;

                let (mut conn, _) = tcp.accept()?;
                let mut data = vec![0u8; 256];
                let us = conn.read(&mut data)?;
                assert_eq!(std::str::from_utf8(&data[..us])?, "Hello World");

                conn.write_all("Bye World".as_bytes())?;
                Ok::<_, anyhow::Error>(())
            }
        });

        let execuor = Executor::new();
        smol::future::block_on(execuor.run(async {
            //         let to_connect = rx.await.unwrap();
            // println!("tto_connect: {:?}", to_connect);
            let Ok(IpTestPacket::Addr(addr)) = rx.recv_async().await else {
                return Err(anyhow!("Failed to lookup addresss"));
            };

            let mut tcp = TcpStream::connect(execuor.io_uring(), addr).await?;
            let (r, _) = tcp.write_all("Hello World").await;
            r?;

            let buffer = vec![0u8; 512];
            let (r, buffer) = tcp.read(buffer).await;
            let us = r?;
            assert_eq!(std::str::from_utf8(&buffer[..us])?, "Bye World");

            Ok::<_, anyhow::Error>(())
        }))
        .unwrap();

        handle.join().unwrap().unwrap();
    }

    #[test]
    pub fn test_tcp_ipv6_connector() {
        let (tx, rx) = unbounded::<IpTestPacket>();

        let handle = std::thread::spawn({
            move || {
                let tcp = TcpListener::bind(std::net::SocketAddr::V6(SocketAddrV6::new(
                    Ipv6Addr::LOCALHOST,
                    0,
                    0,
                    0,
                )))?;

                println!("TCP: {:?}", tcp.local_addr()?);
                tx.send(IpTestPacket::Addr(tcp.local_addr()?))?;

                let (mut conn, _) = tcp.accept()?;
                let mut data = vec![0u8; 256];
                let us = conn.read(&mut data)?;
                assert_eq!(std::str::from_utf8(&data[..us])?, "Hello World");

                conn.write_all("Bye World".as_bytes())?;
                Ok::<_, anyhow::Error>(())
            }
        });

        let execuor = Executor::new();
        smol::future::block_on(execuor.run(async {
            //         let to_connect = rx.await.unwrap();
            // println!("tto_connect: {:?}", to_connect);
            let Ok(IpTestPacket::Addr(addr)) = rx.recv_async().await else {
                return Err(anyhow!("Failed to lookup addresss"));
            };

            let mut tcp = TcpStream::connect(execuor.io_uring(), addr).await?;
            let (r, _) = tcp.write_all("Hello World").await;
            r?;

            let buffer = vec![0u8; 512];
            let (r, buffer) = tcp.read(buffer).await;
            let us = r?;
            assert_eq!(std::str::from_utf8(&buffer[..us])?, "Bye World");

            Ok::<_, anyhow::Error>(())
        }))
        .unwrap();

        handle.join().unwrap().unwrap();
    }

    #[test]
    pub fn test_tcp_listener() {
        let (tx, rx) = unbounded::<IpTestPacket>();

        let handle = std::thread::spawn({
            move || {
                let Ok(IpTestPacket::Addr(ady)) = rx.recv() else {
                    return Err(anyhow!("Failed to receive IP."));
                };

                // println!("ADY: {:?}", ady);

                let mut socket = std::net::TcpStream::connect(ady)?;
                // println!("Connected..");
                socket.write_all("hello".as_bytes())?;

                let mut buffer = vec![0u8; 512];
                let us = socket.read(&mut buffer)?;
                assert_eq!(std::str::from_utf8(&buffer[..us])?, "bye");

                // let tcp = TcpListener::bind("0.0.0.0:0")?;

                // tx.send(IpTestPacket::Addr(tcp.local_addr()?))?;

                // let (mut conn, _) = tcp.accept()?;
                // let mut data = vec![0u8; 256];
                // let us =  conn.read(&mut data)?;
                // // println!("Data: {:?}", data);?
                // assert_eq!(std::str::from_utf8(&data[..us])?, "Hello World");

                // conn.write_all("Bye World".as_bytes())?;
                // // tx.send(tcp.local_addr().unwrap()).unwrap();

                Ok::<_, anyhow::Error>(())
            }
        });

        let execuor = Executor::new();
        smol::future::block_on(execuor.run(async {
            //         let to_connect = rx.await.unwrap();
            // println!("tto_connect: {:?}", to_connect);
            let listener =
                crate::core::io::net::TcpListener::bind(execuor.io_uring(), "0.0.0.0:3917").await?;

            tx.send_async(IpTestPacket::Addr(listener.local_addr()?))
                .await?;

            let (mut stream, _) = listener.accept().await?;

            // println!("herro");

            let buffer = vec![0u8; 512];
            let (r, buffer) = stream.read(buffer).await;
            let us = r?;
            assert_eq!(std::str::from_utf8(&buffer[..us])?, "hello");

            let (r, _) = stream.write_all("bye").await;
            r?;

            // println!("BE: {:?}", buffer);

            Ok::<_, anyhow::Error>(())
        }))
        .unwrap();

        handle.join().unwrap().unwrap();
    }

    #[test]
    pub fn test_tcp_ipv6_listener() {
        let (tx, rx) = unbounded::<IpTestPacket>();

        let handle = std::thread::spawn({
            move || {
                let Ok(IpTestPacket::Addr(ady)) = rx.recv() else {
                    return Err(anyhow!("Failed to receive IP."));
                };

                // println!("ADY: {:?}", ady);

                let mut socket = std::net::TcpStream::connect(ady)?;
                // println!("Connected..");
                socket.write_all("hello".as_bytes())?;

                let mut buffer = vec![0u8; 512];
                let us = socket.read(&mut buffer)?;
                assert_eq!(std::str::from_utf8(&buffer[..us])?, "bye");

                // let tcp = TcpListener::bind("0.0.0.0:0")?;

                // tx.send(IpTestPacket::Addr(tcp.local_addr()?))?;

                // let (mut conn, _) = tcp.accept()?;
                // let mut data = vec![0u8; 256];
                // let us =  conn.read(&mut data)?;
                // // println!("Data: {:?}", data);?
                // assert_eq!(std::str::from_utf8(&data[..us])?, "Hello World");

                // conn.write_all("Bye World".as_bytes())?;
                // // tx.send(tcp.local_addr().unwrap()).unwrap();

                Ok::<_, anyhow::Error>(())
            }
        });

        let execuor = Executor::new();
        smol::future::block_on(execuor.run(async {
            //         let to_connect = rx.await.unwrap();
            // println!("tto_connect: {:?}", to_connect);
            let listener = crate::core::io::net::TcpListener::bind(
                execuor.io_uring(),
                std::net::SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 0, 0, 0)),
            )
            .await?;

            tx.send_async(IpTestPacket::Addr(listener.local_addr()?))
                .await?;

            // println!("Bind...");

            let (mut stream, _) = listener.accept().await?;

            // println!("herro");

            let buffer = vec![0u8; 512];
            let (r, buffer) = stream.read(buffer).await;
            let us = r?;
            assert_eq!(std::str::from_utf8(&buffer[..us])?, "hello");

            let (r, _) = stream.write_all("bye").await;
            r?;

            // println!("BE: {:?}", buffer);

            Ok::<_, anyhow::Error>(())
        }))
        .unwrap();

        handle.join().unwrap().unwrap();
    }
}
