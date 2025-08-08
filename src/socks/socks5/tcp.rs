use crate::socks::error::{Error, ErrorKind};
use crate::socks::socks5::address::Address;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[repr(u8)]
pub enum Command {
    Connect = 0x01u8,
    Bind = 0x02u8,
    UdpAssociate = 0x03u8,
}

#[repr(u8)]
enum Method {
    NoAuth = 0x00u8,
    _UserPass = 0x02u8,
    NoAcceptable = 0xFFu8,
}

#[repr(u8)]
pub enum Reply {
    Succeeded = 0x00u8,
    GeneralFailure = 0x01u8,
    CommandNotSupported = 0x07u8,
    AddrTypeNotSupported = 0x08u8,
}

impl TryInto<Command> for u8 {
    type Error = Error;

    fn try_into(self) -> Result<Command, Error> {
        if self == Command::Connect as Self {
            Ok(Command::Connect)
        } else if self == Command::Bind as Self {
            Ok(Command::Bind)
        } else if self == Command::UdpAssociate as Self {
            Ok(Command::UdpAssociate)
        } else {
            Err(Error::new(ErrorKind::CommandNotSupported))
        }
    }
}

pub struct TcpReply {
    rep: u8,
    bind_addr: Address,
}

impl TcpReply {
    pub fn new(rep: u8, bind_addr: Option<Address>) -> Self {
        Self {
            rep,
            bind_addr: bind_addr.unwrap_or(Address::SocketAddress("0.0.0.0:0".parse().unwrap())),
        }
    }

    pub async fn write_to<W>(&self, writer: &mut W) -> Result<(), Error>
    where
        W: AsyncWrite + Unpin,
    {
        writer.write_all(&[5u8, self.rep, 0u8]).await?;
        self.bind_addr.write_to(writer).await?;
        writer.flush().await?;
        Ok(())
    }
}

pub struct TcpRequest {
    pub cmd: Command,
    pub addr: Address,
}

impl TcpRequest {
    pub async fn read_from<R>(stream: &mut R) -> Result<Self, Error>
    where
        R: AsyncRead + AsyncWrite + Unpin,
    {
        let len = stream.read_u8().await? as usize;
        let mut methods = vec![0u8; len];
        stream.read_exact(&mut methods).await?;
        if !methods.contains(&(Method::NoAuth as u8)) {
            stream.write_all(&[5u8, Method::NoAcceptable as u8]).await?;
            stream.flush().await?;
            return Err(Error::new(ErrorKind::NoAcceptableMethod));
        }
        stream.write_all(&[5u8, Method::NoAuth as u8]).await?;
        stream.flush().await?;

        let version = stream.read_u8().await?;
        if version != 5u8 {
            return Err(Error::new(ErrorKind::VersionMismatch));
        }

        let cmd: Command = match stream.read_u8().await?.try_into() {
            Ok(v) => v,
            Err(err) => {
                TcpReply::new(Reply::CommandNotSupported as u8, None)
                    .write_to(stream)
                    .await?;
                return Err(err);
            }
        };
        let _rsv = stream.read_u8().await?;
        let addr = match Address::read_from(stream).await {
            Ok(v) => v,
            Err(err) => {
                TcpReply::new(Reply::AddrTypeNotSupported as u8, None)
                    .write_to(stream)
                    .await?;
                return Err(err);
            }
        };
        Ok(Self { cmd, addr })
    }
}
