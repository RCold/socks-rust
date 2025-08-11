use crate::socks::error::{Error, ErrorKind};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[repr(u8)]
enum Method {
    NoAuth = 0x00u8,
    _UserPass = 0x02u8,
    NoAcceptable = 0xFFu8,
}

struct Reply {
    method: u8,
}

impl Reply {
    fn new(method: u8) -> Self {
        Self { method }
    }

    async fn write_to<W>(&self, writer: &mut W) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        writer.write_all(&[5u8, self.method]).await?;
        writer.flush().await
    }
}

pub async fn authenticate<RW>(stream: &mut RW) -> Result<(), Error>
where
    RW: AsyncRead + AsyncWrite + Unpin,
{
    let len = stream.read_u8().await? as usize;
    let mut methods = vec![0u8; len];
    stream.read_exact(&mut methods).await?;
    if methods.contains(&(Method::NoAuth as u8)) {
        Reply::new(Method::NoAuth as u8).write_to(stream).await?;
        Ok(())
    } else {
        Reply::new(Method::NoAcceptable as u8)
            .write_to(stream)
            .await
            .unwrap_or(());
        Err(Error::new(ErrorKind::NoAcceptableAuthMethods))
    }
}
