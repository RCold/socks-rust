use crate::socks::error::Error;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Clone, Copy)]
#[repr(u8)]
enum Method {
    NoAuth = 0x00u8,
    _UserPass = 0x02u8,
    NoAcceptable = 0xFFu8,
}

struct Reply {
    method: Method,
}

impl Reply {
    fn new(method: Method) -> Self {
        Self { method }
    }

    async fn write_to<W: AsyncWrite + Unpin>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[5u8, self.method as u8]).await?;
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
        Reply::new(Method::NoAuth).write_to(stream).await?;
        Ok(())
    } else {
        Reply::new(Method::NoAcceptable)
            .write_to(stream)
            .await
            .unwrap_or_default();
        Err(Error::NoAcceptableAuthMethods)
    }
}
