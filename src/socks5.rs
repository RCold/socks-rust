// SPDX-License-Identifier: Apache-2.0
// Copyright (C) 2025 Yeuham Wang <rcold@rcold.name>

mod auth;
mod tcp;
mod udp;

use crate::error::Error;
use crate::socks5::tcp::{Reply, ReplyCode, Request};
use crate::socks5::udp::{UdpHeader, UdpListener, UdpSession};
use log::{debug, error, info};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;
use std::fmt::Display;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use tokio::io;
use tokio::io::{
    AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufStream, BufWriter,
};
use tokio::net;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc;

#[repr(u8)]
enum Command {
    Connect = 0x01u8,
    Bind = 0x02u8,
    UdpAssociate = 0x03u8,
}

impl TryInto<Command> for u8 {
    type Error = Error;

    fn try_into(self) -> Result<Command, Self::Error> {
        if self == Command::Connect as Self {
            Ok(Command::Connect)
        } else if self == Command::Bind as Self {
            Ok(Command::Bind)
        } else if self == Command::UdpAssociate as Self {
            Ok(Command::UdpAssociate)
        } else {
            Err(Error::InvalidCommand)
        }
    }
}

#[repr(u8)]
enum AddrType {
    IPv4 = 0x01u8,
    DomainName = 0x03u8,
    IPv6 = 0x04u8,
}

impl TryInto<AddrType> for u8 {
    type Error = Error;

    fn try_into(self) -> Result<AddrType, Self::Error> {
        if self == AddrType::IPv4 as Self {
            Ok(AddrType::IPv4)
        } else if self == AddrType::DomainName as Self {
            Ok(AddrType::DomainName)
        } else if self == AddrType::IPv6 as Self {
            Ok(AddrType::IPv6)
        } else {
            Err(Error::InvalidAddressType)
        }
    }
}

enum Address {
    Socket(SocketAddr),
    Domain(String, u16),
}

impl Address {
    pub async fn read_from<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Self, Error> {
        let addr_type = reader.read_u8().await?.try_into()?;
        match addr_type {
            AddrType::IPv4 => {
                let mut ip = [0u8; 4];
                reader.read_exact(&mut ip).await?;
                let port = reader.read_u16().await?;
                Ok(Self::Socket(SocketAddr::new(ip.into(), port)))
            }
            AddrType::DomainName => {
                let len = reader.read_u8().await? as usize;
                if len < 1 {
                    return Err(Error::InvalidDomainName);
                }
                let mut domain = vec![0u8; len];
                reader.read_exact(&mut domain).await?;
                let domain = String::from_utf8(domain).map_err(|_| Error::InvalidDomainName)?;
                let port = reader.read_u16().await?;
                Ok(Self::Domain(domain, port))
            }
            AddrType::IPv6 => {
                let mut ip = [0u8; 16];
                reader.read_exact(&mut ip).await?;
                let port = reader.read_u16().await?;
                Ok(Self::Socket(SocketAddr::new(ip.into(), port)))
            }
        }
    }

    pub async fn write_to<W: AsyncWrite + Unpin>(&self, writer: &mut W) -> Result<(), Error> {
        match self {
            Self::Socket(SocketAddr::V4(addr)) => {
                writer.write_u8(AddrType::IPv4 as u8).await?;
                writer.write_all(&addr.ip().octets()).await?;
                writer.write_u16(addr.port()).await?;
            }
            Self::Domain(addr, port) => {
                writer.write_u8(AddrType::DomainName as u8).await?;
                if addr.is_empty() || addr.len() > 255 {
                    return Err(Error::InvalidDomainName);
                }
                writer.write_u8(addr.len() as u8).await?;
                writer.write_all(addr.as_bytes()).await?;
                writer.write_u16(*port).await?;
            }
            Self::Socket(SocketAddr::V6(addr)) => {
                writer.write_u8(AddrType::IPv6 as u8).await?;
                writer.write_all(&addr.ip().octets()).await?;
                writer.write_u16(addr.port()).await?;
            }
        }
        Ok(())
    }

    pub fn _serialized_len(&self) -> usize {
        1 + match self {
            Self::Socket(SocketAddr::V4(_)) => 4,
            Self::Socket(SocketAddr::V6(_)) => 16,
            Self::Domain(addr, _) => 1 + addr.len(),
        } + 2
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Socket(addr) => addr.fmt(f),
            Self::Domain(domain, port) => {
                if let Ok(addr) = Ipv6Addr::from_str(domain) {
                    write!(f, "[{addr}]:{port}")
                } else {
                    write!(f, "{domain}:{port}")
                }
            }
        }
    }
}

impl Default for Address {
    fn default() -> Self {
        Self::Socket(SocketAddr::new([0u8, 0u8, 0u8, 0u8].into(), 0u16))
    }
}

async fn handle_connect(
    mut stream: BufStream<TcpStream>,
    remote_addr: String,
) -> Result<(), Error> {
    match TcpStream::connect(&remote_addr).await {
        Ok(mut remote) => {
            remote.set_nodelay(true).unwrap_or_default();
            debug!("tcp://{remote_addr} connected");
            Reply::new(ReplyCode::Succeeded, None)
                .write_to(&mut stream)
                .await?;
            io::copy_bidirectional(&mut stream, &mut remote).await?;
            debug!("tcp://{remote_addr} disconnected");
            Ok(())
        }
        Err(err) => {
            Reply::new(ReplyCode::GeneralFailure, None)
                .write_to(&mut stream)
                .await
                .unwrap_or_default();
            Err(err.into())
        }
    }
}

async fn handle_remote_udp(
    session: &UdpSession,
    data: &[u8],
    remote_addr: SocketAddr,
) -> Result<(), Error> {
    let mut writer = BufWriter::new(Vec::new());
    UdpHeader::new(Address::Socket(remote_addr))
        .write_to(&mut writer)
        .await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    session.send(writer.into_inner());
    Ok(())
}

async fn handle_udp(mut session: UdpSession) -> Result<(), Error> {
    let (client_tx, mut client_rx) = mpsc::channel(32);
    let (remote_tx, mut remote_rx) = mpsc::channel::<(Vec<u8>, _)>(32);
    let remote_socket_v4 = UdpSocket::bind("0.0.0.0:0").await?;
    let remote_socket_v6 = UdpSocket::bind("[::]:0").await?;
    tokio::spawn(async move {
        let mut buf_v4 = [0u8; 65536];
        let mut buf_v6 = [0u8; 65536];
        loop {
            tokio::select! {
                v = session.recv() => {
                    match v {
                        Some(data) => client_tx.try_send(data).unwrap_or_default(),
                        None => break,
                    }
                }
                Some((data, remote_addr)) = remote_rx.recv() => {
                    match remote_addr {
                        SocketAddr::V4(_) => &remote_socket_v4,
                        SocketAddr::V6(_) => &remote_socket_v6,
                    }.try_send_to(data.as_slice(), remote_addr).unwrap_or_default();
                }
                Ok((len, remote_addr)) = remote_socket_v4.recv_from(&mut buf_v4) => {
                    handle_remote_udp(&session, &buf_v4[..len], remote_addr).await.unwrap_or_default();
                }
                Ok((len, remote_addr)) = remote_socket_v6.recv_from(&mut buf_v6) => {
                    handle_remote_udp(&session, &buf_v6[..len], remote_addr).await.unwrap_or_default();
                }
            }
        }
    });
    let mut resolve_cache = HashMap::new();
    while let Some(data) = client_rx.recv().await {
        let mut reader = BufReader::new(data.as_slice());
        let header = UdpHeader::read_from(&mut reader).await?;
        let remote_addr = match header.addr() {
            Address::Socket(v) => *v,
            Address::Domain(domain, port) => {
                let ip = *match resolve_cache.entry(String::from(domain)) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        let v = net::lookup_host((domain.as_str(), 0u16))
                            .await?
                            .next()
                            .ok_or(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "no addresses to send data to",
                            ))?;
                        debug!("domain name {domain} resolved to {}", v.ip());
                        entry.insert(v.ip())
                    }
                };
                SocketAddr::new(ip, *port)
            }
        };
        let mut data = Vec::new();
        reader.read_to_end(&mut data).await?;
        remote_tx.try_send((data, remote_addr)).unwrap_or_default();
    }
    Ok(())
}

async fn handle_udp_associate(
    mut stream: BufStream<TcpStream>,
    client_ip: IpAddr,
) -> Result<(), Error> {
    let mut bind = stream.get_ref().local_addr()?;
    bind.set_port(0u16);
    let mut udp_listener = match UdpListener::bind(&bind).await {
        Ok(v) => v,
        Err(err) => {
            Reply::new(ReplyCode::GeneralFailure, None)
                .write_to(&mut stream)
                .await
                .unwrap_or_default();
            return Err(err.into());
        }
    };
    Reply::new(
        ReplyCode::Succeeded,
        Some(Address::Socket(udp_listener.local_addr())),
    )
    .write_to(&mut stream)
    .await?;
    let mut buf = [0u8; 65536];
    loop {
        tokio::select! {
            v = stream.read(&mut buf) => if v? == 0 { break },
            v = udp_listener.accept() => {
                match v {
                    Ok((session, client_addr)) => {
                        if client_addr.ip() != client_ip {
                            info!("udp packets from client {client_addr} dropped: client ip address not allowed");
                            continue;
                        }
                        tokio::spawn(async move {
                            debug!("udp session for client {client_addr} opened");
                            if let Err(err) = handle_udp(session).await {
                                error!("failed to handle udp packet from client {client_addr}: {err}");
                            }
                            debug!("udp session for client {client_addr} closed");
                        });
                    }
                    Err(err) => {
                        error!("failed to receive udp packet from a client: {err}");
                    }
                }
            }
        }
    }
    Ok(())
}

pub async fn handle_tcp(stream: TcpStream, client_addr: SocketAddr) -> Result<(), Error> {
    let mut stream = BufStream::new(stream);
    if !auth::authenticate(&mut stream).await? {
        info!("socks5 request from {client_addr} rejected: authentication failed");
        return Ok(());
    }
    let request = Request::read_from(&mut stream).await?;
    let remote_addr = request.addr().to_string();
    match request.cmd() {
        Command::Connect => {
            info!(
                "socks5 connect request from client {client_addr} to tcp://{remote_addr} accepted"
            );
            handle_connect(stream, remote_addr).await?;
        }
        Command::Bind => {
            info!("socks5 bind request from client {client_addr} rejected: not implemented");
            Reply::new(ReplyCode::CommandNotSupported, None)
                .write_to(&mut stream)
                .await?;
        }
        Command::UdpAssociate => {
            info!(
                "socks5 udp associate request from client {client_addr} to udp://{remote_addr} accepted"
            );
            handle_udp_associate(stream, client_addr.ip()).await?;
        }
    }
    Ok(())
}
