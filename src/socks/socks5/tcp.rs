use crate::socks::error::{Error, ErrorKind};
use crate::socks::socks5::address::Address;
use crate::socks::socks5::command::Command;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[repr(u8)]
pub enum ReplyCode {
    Succeeded = 0x00u8,
    GeneralFailure = 0x01u8,
    CommandNotSupported = 0x07u8,
    AddrTypeNotSupported = 0x08u8,
}

pub struct Reply {
    rep: u8,
    bind_addr: Address,
}

impl Reply {
    pub fn new(rep: u8, bind_addr: Option<Address>) -> Self {
        Self {
            rep,
            bind_addr: bind_addr.unwrap_or(Address::SocketAddress("0.0.0.0:0".parse().unwrap())),
        }
    }

    pub async fn write_to<W: AsyncWrite + Unpin>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[5u8, self.rep, 0u8]).await?;
        self.bind_addr.write_to(writer).await?;
        writer.flush().await
    }
}

pub struct Request {
    pub cmd: Command,
    pub addr: Address,
}

impl Request {
    pub async fn read_from<RW>(stream: &mut RW) -> Result<Self, Error>
    where
        RW: AsyncRead + AsyncWrite + Unpin,
    {
        let version = stream.read_u8().await?;
        if version != 5u8 {
            return Err(Error::new(ErrorKind::VersionMismatch));
        }
        let cmd: Command = match stream.read_u8().await?.try_into() {
            Ok(v) => v,
            Err(err) => {
                Reply::new(ReplyCode::CommandNotSupported as u8, None)
                    .write_to(stream)
                    .await
                    .unwrap_or_default();
                return Err(err);
            }
        };
        let _rsv = stream.read_u8().await?;
        let addr = match Address::read_from(stream).await {
            Ok(v) => v,
            Err(err) => {
                Reply::new(ReplyCode::AddrTypeNotSupported as u8, None)
                    .write_to(stream)
                    .await
                    .unwrap_or_default();
                return Err(err);
            }
        };
        Ok(Self { cmd, addr })
    }
}
