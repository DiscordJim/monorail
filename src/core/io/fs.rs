use std::{
    ffi::CString,
    os::fd::RawFd,
    path::Path,
};

use anyhow::Result;
use nix::libc::{O_APPEND, O_CLOEXEC, O_CREAT, O_NONBLOCK, O_RDONLY, O_RDWR, O_WRONLY};

use crate::core::io::{ring::{openat, read, write, IoRingDriver, OwnedBuffer, OwnedReadBuf}, FromRing};

// TODO: Finalize this.
pub struct OpenOptions<'a> {
    ring: &'a IoRingDriver,
    read: bool,
    write: bool,
    append: bool,
    create: bool,
    truncate: bool
}

impl<'a> FromRing<'a> for OpenOptions<'a> {
    fn from_ring(driver: &'a IoRingDriver) -> Self {
        Self {
            ring: driver,
            append: false,
            read: false,
            write: false,
            create: false,
            truncate: false

        }
    }
}

impl<'a> OpenOptions<'a> {
    pub fn read(mut self, value: bool) -> Self {
        self.read = value;
        self
    }
    pub fn write(mut self, value: bool) -> Self {
        self.write = value;
        self
    }
    pub fn append(mut self, value: bool) -> Self {
        self.append = value;
        self
    }
    pub fn create(mut self, value: bool) -> Self {
        self.create = value;
        self
    }
    pub fn truncate(mut self, value: bool) -> Self {
        self.truncate = value;
        self
    }
    pub async fn open<P>(self, path: P) -> Result<File<'a>>
    where 
        P: AsRef<Path>
    {
        let mut flags = O_NONBLOCK | O_CLOEXEC;
        if self.create {
            flags |= O_CREAT;
        } else if self.read && self.write {
            flags |= O_RDWR;
        } else if self.read {
            flags |= O_RDONLY;
        } else if self.write {
            flags |= O_WRONLY;
        } else if self.append {
            flags |= O_APPEND;
        }

        let fd = openat(
            self.ring,
            &CString::new(path.as_ref().as_os_str().as_encoded_bytes())?,
            flags,
            0,
        )
        .await?;

        Ok(File { ring: self.ring, fd })

    }
    // pub fn from_ring
}

pub struct File<'a> {
    ring: &'a IoRingDriver,
    fd: RawFd,
}

impl<'a> File<'a> {
    pub(crate) async fn open<P>(ring: &'a IoRingDriver, path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        OpenOptions::from_ring(ring)
            .open(path).await
    }
    pub async fn read<B>(&self, buffer: B) -> (std::io::Result<usize>, B)
    where 
        B: OwnedBuffer
    {
        read(self.ring, self.fd, buffer).await
    } 
    pub async fn write<B>(&self, buffer: B) -> (std::io::Result<usize>, B)
    where 
        B: OwnedReadBuf
    {
        write(self.ring, self.fd, buffer).await
    }
}

#[cfg(test)]
mod tests {
    use std::{ffi::CString, io::{Read, Write}};

    use asyncnal::{EventSetter, LocalEvent};
    use nix::libc::{O_NONBLOCK, O_RDONLY};
    use tempfile::{tempfile, NamedTempFile};

    use crate::core::{
        executor::scheduler::Executor,
        io::{
            fs::{File, OpenOptions},
            ring::{openat, IoRingDriver}, FromRing,
        },
    };

    #[test]
    pub fn test_file_ring_basic_read() -> anyhow::Result<()> {
        let mut tfile = NamedTempFile::new()?;
        tfile.write_all("hello".as_bytes())?;
        

        let executor = Executor::new();

        smol::future::block_on(executor.run(async {
            let file = File::open(&executor.io_uring(), tfile.path()).await.unwrap();
            let (r, b) = file.read(vec![0u8; 24]).await;
            assert_eq!(std::str::from_utf8(&b[..r?])?, "hello");
            Ok::<_, anyhow::Error>(())
        }))?;

        Ok(())

    }

    #[test]
    pub fn test_file_ring_basic_write() -> anyhow::Result<()> {
        let mut tfile = NamedTempFile::new()?;
        // tfile.write_all("hello".as_bytes())?;
        

        let executor = Executor::new();

        smol::future::block_on(executor.run(async {
            let file = executor.open_options()
                .write(true)
                .open(tfile.path())
                .await?;
            let (r, b) = file.write("hello").await;
            r?;

            // let (r, b) = file.read(vec![0u8; 24]).await;
            // assert_eq!(std::str::from_utf8(&b[..r?])?, "hello");
            Ok::<_, anyhow::Error>(())
        }))?;

        let mut remainer = vec![0u8; 1024];
        let b = tfile.read(&mut remainer)?;

        assert_eq!(std::str::from_utf8(&remainer[..b])?, "hello");

        Ok(())

    }
}
