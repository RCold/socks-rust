use crate::error::Error;
use crate::socks5::Address;
use crate::socks5::Command;
use tokio::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum ReplyCode {
    Succeeded = 0x00u8,
    GeneralFailure = 0x01u8,
    CommandNotSupported = 0x07u8,
    AddressTypeNotSupported = 0x08u8,
}

pub struct Reply {
    rep: ReplyCode,
    bind: Address,
}

impl Reply {
    pub fn new(rep: ReplyCode, bind: Option<Address>) -> Self {
        Self {
            rep,
            bind: bind.unwrap_or_default(),
        }
    }

    pub async fn write_to<W: AsyncWrite + Unpin>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[5u8, self.rep as u8, 0u8]).await?;
        self.bind.write_to(writer).await?;
        writer.flush().await
    }
}

pub struct Request {
    cmd: Command,
    addr: Address,
}

impl Request {
    pub async fn read_from<RW>(stream: &mut RW) -> Result<Self, Error>
    where
        RW: AsyncRead + AsyncWrite + Unpin,
    {
        let version = stream.read_u8().await?;
        if version != 5u8 {
            return Err(Error::VersionMismatch);
        }
        let cmd = match stream.read_u8().await?.try_into() {
            Ok(v) => v,
            Err(Error::CommandNotSupported) => {
                Reply::new(ReplyCode::CommandNotSupported, None)
                    .write_to(stream)
                    .await
                    .unwrap_or_default();
                return Err(Error::CommandNotSupported);
            }
            Err(err) => {
                return Err(err);
            }
        };
        let _rsv = stream.read_u8().await?;
        let addr = match Address::read_from(stream).await {
            Ok(v) => v,
            Err(Error::AddressTypeNotSupported) => {
                Reply::new(ReplyCode::AddressTypeNotSupported, None)
                    .write_to(stream)
                    .await
                    .unwrap_or_default();
                return Err(Error::AddressTypeNotSupported);
            }
            Err(err) => {
                return Err(err);
            }
        };
        Ok(Self { cmd, addr })
    }

    pub fn cmd(&self) -> &Command {
        &self.cmd
    }

    pub fn addr(&self) -> &Address {
        &self.addr
    }
}
