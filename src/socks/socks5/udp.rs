use crate::socks::error::{Error, ErrorKind};
use crate::socks::socks5::address::Address;
use log::debug;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};
use tokio::net;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

struct UdpHeader {
    _frag: u8,
    addr: Address,
}

impl UdpHeader {
    fn new(addr: Address) -> Self {
        Self { _frag: 0, addr }
    }

    async fn read_from<R>(reader: &mut R) -> Result<Self, Error>
    where
        R: AsyncRead + Unpin,
    {
        let _rsv = reader.read_u16().await?;
        let frag = reader.read_u8().await?;
        if frag != 0u8 {
            return Err(Error::new(ErrorKind::FragmentationNotSupported));
        }
        let addr = Address::read_from(reader).await?;
        Ok(Self { _frag: frag, addr })
    }

    async fn write_to<W>(&self, writer: &mut W) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        writer.write_all(&[0u8, 0u8, self._frag]).await?;
        self.addr.write_to(writer).await
    }

    fn _serialized_len(&self) -> usize {
        2 + 1 + self.addr._serialized_len()
    }
}

pub struct UdpSession {
    remote_socket_v4: Arc<UdpSocket>,
    remote_socket_v6: Arc<UdpSocket>,
    remote_addr_map: HashMap<Address, SocketAddr>,
    client_addr_map: HashMap<SocketAddr, SocketAddr>,
    ref_count: usize,
}

impl UdpSession {
    async fn open() -> io::Result<Self> {
        Ok(Self {
            remote_socket_v4: Arc::new(UdpSocket::bind("0.0.0.0:0").await?),
            remote_socket_v6: Arc::new(UdpSocket::bind("[::]:0").await?),
            remote_addr_map: HashMap::new(),
            client_addr_map: HashMap::new(),
            ref_count: 1,
        })
    }

    async fn resolve_addr(&mut self, addr: &Address) -> io::Result<SocketAddr> {
        Ok(match self.remote_addr_map.entry(addr.clone()) {
            Entry::Occupied(entry) => *entry.into_mut(),
            Entry::Vacant(entry) => match addr {
                Address::SocketAddress(v) => *entry.insert(*v),
                Address::DomainAddress(domain, port) => {
                    match net::lookup_host((domain.as_str(), *port)).await?.next() {
                        Some(v) => {
                            debug!("domain name {domain} resolved to {}", v.ip());
                            *entry.insert(v)
                        }
                        None => {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "no addresses to send data to",
                            ));
                        }
                    }
                }
            },
        })
    }

    pub fn remote_socket_v4(&self) -> Arc<UdpSocket> {
        self.remote_socket_v4.clone()
    }

    pub fn remote_socket_v6(&self) -> Arc<UdpSocket> {
        self.remote_socket_v6.clone()
    }
}

pub struct UdpSessionManager {
    client_socket: Arc<UdpSocket>,
    session_map: HashMap<IpAddr, UdpSession>,
}

impl UdpSessionManager {
    pub fn new(client_socket: Arc<UdpSocket>) -> Self {
        Self {
            client_socket,
            session_map: HashMap::new(),
        }
    }

    pub fn client_socket(&self) -> Arc<UdpSocket> {
        self.client_socket.clone()
    }

    pub async fn open_session(&mut self, client_ip: IpAddr) -> io::Result<&mut UdpSession> {
        Ok(match self.session_map.entry(client_ip) {
            Entry::Occupied(entry) => {
                let session = entry.into_mut();
                session.ref_count += 1;
                session
            }
            Entry::Vacant(entry) => {
                debug!("udp session for client {client_ip} opened");
                entry.insert(UdpSession::open().await?)
            }
        })
    }

    pub fn close_session(&mut self, client_ip: &IpAddr) {
        if let Some(session) = self.session_map.get_mut(client_ip) {
            session.ref_count -= 1;
            if session.ref_count < 1 {
                debug!("udp session for client {client_ip} closed");
                self.session_map.remove(&client_ip);
            }
        }
    }

    pub fn get_session(&self, client_ip: &IpAddr) -> Option<&UdpSession> {
        let Some(session) = self.session_map.get(client_ip) else {
            return None;
        };
        Some(session)
    }

    pub fn get_session_mut(&mut self, client_ip: &IpAddr) -> Option<&mut UdpSession> {
        let Some(session) = self.session_map.get_mut(client_ip) else {
            return None;
        };
        Some(session)
    }
}

pub async fn handle_client(
    data: &[u8],
    client_addr: SocketAddr,
    manager: Arc<Mutex<UdpSessionManager>>,
) -> Result<(), Error> {
    let mut manager = manager.lock().await;
    let Some(session) = manager.get_session_mut(&client_addr.ip()) else {
        return Err(Error::new(ErrorKind::InvalidUdpPacketReceived));
    };
    let mut reader = BufReader::new(data);
    let header = UdpHeader::read_from(&mut reader).await?;
    let remote_addr = session.resolve_addr(&header.addr).await?;
    let mut data = Vec::new();
    reader.read_to_end(&mut data).await?;
    let remote_socket = match remote_addr {
        SocketAddr::V4(_) => session.remote_socket_v4.as_ref(),
        SocketAddr::V6(_) => session.remote_socket_v6.as_ref(),
    };
    remote_socket.send_to(data.as_slice(), &remote_addr).await?;
    session.client_addr_map.insert(remote_addr, client_addr);
    Ok(())
}

pub async fn handle_remote(
    data: &[u8],
    remote_addr: SocketAddr,
    client_ip: IpAddr,
    manager: Arc<Mutex<UdpSessionManager>>,
) -> Result<(), Error> {
    let manager = manager.lock().await;
    let Some(session) = manager.get_session(&client_ip) else {
        return Err(Error::new(ErrorKind::InvalidUdpPacketReceived));
    };
    let Some(client_addr) = session.client_addr_map.get(&remote_addr) else {
        return Err(Error::new(ErrorKind::InvalidUdpPacketReceived));
    };
    let mut writer = BufWriter::new(Vec::new());
    UdpHeader::new(Address::SocketAddress(remote_addr))
        .write_to(&mut writer)
        .await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    manager
        .client_socket()
        .send_to(writer.get_ref(), &client_addr)
        .await?;
    Ok(())
}
