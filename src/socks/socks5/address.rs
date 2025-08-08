use crate::socks::error::{Error, ErrorKind};
use std::fmt::Display;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::{fmt, io};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[repr(u8)]
enum AddrType {
    IPv4 = 0x01u8,
    DomainName = 0x03u8,
    IPv6 = 0x04u8,
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub enum Address {
    SocketAddress(SocketAddr),
    DomainAddress(String, u16),
}

impl Address {
    pub async fn read_from<R>(reader: &mut R) -> Result<Self, Error>
    where
        R: AsyncRead + Unpin,
    {
        let addr_type = reader.read_u8().await?;

        if addr_type == AddrType::IPv4 as u8 {
            let mut ip = [0u8; 4];
            reader.read_exact(&mut ip).await?;
            let port = reader.read_u16().await?;
            Ok(Self::SocketAddress(SocketAddr::from((
                Ipv4Addr::from(ip),
                port,
            ))))
        } else if addr_type == AddrType::DomainName as u8 {
            let len = reader.read_u8().await? as usize;
            let mut domain = vec![0u8; len];
            reader.read_exact(&mut domain).await?;
            let port = reader.read_u16().await?;
            let domain =
                String::from_utf8(domain).map_err(|_| Error::new(ErrorKind::InvalidDomainName))?;
            Ok(Self::DomainAddress(domain, port))
        } else if addr_type == AddrType::IPv6 as u8 {
            let mut ip = [0u8; 16];
            reader.read_exact(&mut ip).await?;
            let port = reader.read_u16().await?;
            Ok(Self::SocketAddress(SocketAddr::from((
                Ipv6Addr::from(ip),
                port,
            ))))
        } else {
            Err(Error::new(ErrorKind::AddressTypeNotSupported))
        }
    }

    pub async fn write_to<W>(&self, writer: &mut W) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        match self {
            Self::SocketAddress(SocketAddr::V4(addr)) => {
                writer.write_u8(AddrType::IPv4 as u8).await?;
                writer.write_all(&addr.ip().octets()).await?;
                writer.write_u16(addr.port()).await?;
            }
            Self::DomainAddress(addr, port) => {
                writer.write_u8(AddrType::DomainName as u8).await?;
                writer.write_u8(addr.len() as u8).await?;
                writer.write_all(addr.as_bytes()).await?;
                writer.write_u16(*port).await?;
            }
            Self::SocketAddress(SocketAddr::V6(addr)) => {
                writer.write_u8(AddrType::IPv6 as u8).await?;
                writer.write_all(&addr.ip().octets()).await?;
                writer.write_u16(addr.port()).await?;
            }
        }
        Ok(())
    }

    pub fn _serialized_len(&self) -> usize {
        1 + match self {
            Self::SocketAddress(SocketAddr::V4(_)) => 6,
            Self::SocketAddress(SocketAddr::V6(_)) => 18,
            Self::DomainAddress(addr, _) => 1 + addr.len() + 2,
        }
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SocketAddress(SocketAddr::V4(addr)) => {
                write!(f, "{}:{}", addr.ip(), addr.port())
            }
            Self::DomainAddress(domain, port) => {
                if let Ok(addr) = Ipv6Addr::from_str(domain) {
                    write!(f, "[{}]:{}", addr, port)
                } else {
                    write!(f, "{}:{}", domain, port)
                }
            }
            Self::SocketAddress(SocketAddr::V6(addr)) => {
                write!(f, "[{}]:{}", addr.ip(), addr.port())
            }
        }
    }
}
