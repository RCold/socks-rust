use crate::error::Error;
use crate::socks5::Address;
use log::debug;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net;
use tokio::net::{ToSocketAddrs, UdpSocket};
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::AbortHandle;

pub struct UdpHeader {
    frag: u8,
    addr: Address,
}

impl UdpHeader {
    pub fn new(addr: Address) -> Self {
        Self { frag: 0, addr }
    }

    pub fn addr(&self) -> &Address {
        &self.addr
    }

    pub async fn read_from<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Self, Error> {
        let _rsv = reader.read_u16().await?;
        let frag = reader.read_u8().await?;
        if frag != 0u8 {
            return Err(Error::FragmentationNotSupported);
        }
        let addr = Address::read_from(reader).await?;
        Ok(Self { frag, addr })
    }

    pub async fn write_to<W: AsyncWrite + Unpin>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[0u8, 0u8, self.frag]).await?;
        self.addr.write_to(writer).await
    }

    pub fn _serialized_len(&self) -> usize {
        2 + 1 + self.addr._serialized_len()
    }
}

pub struct UdpSession {
    peer_addr: SocketAddr,
    tx: Sender<(Vec<u8>, SocketAddr)>,
    rx: Receiver<Vec<u8>>,
    resolve_cache: HashMap<Address, SocketAddr>,
}

impl UdpSession {
    fn new(
        peer_addr: SocketAddr,
        tx: Sender<(Vec<u8>, SocketAddr)>,
        rx: Receiver<Vec<u8>>,
    ) -> Self {
        Self {
            peer_addr,
            tx,
            rx,
            resolve_cache: HashMap::new(),
        }
    }

    pub async fn send(&self, data: Vec<u8>) -> io::Result<()> {
        self.tx
            .send((data, self.peer_addr))
            .await
            .map_err(|_| io::ErrorKind::BrokenPipe.into())
    }

    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        self.rx.recv().await
    }

    pub async fn resolve_addr(&mut self, addr: &Address) -> io::Result<SocketAddr> {
        Ok(*match self.resolve_cache.entry(addr.clone()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => match addr {
                Address::SocketAddress(v) => entry.insert(*v),
                Address::DomainAddress(domain, port) => {
                    let v = net::lookup_host((domain.as_str(), *port))
                        .await?
                        .next()
                        .ok_or(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "no addresses to send data to",
                        ))?;
                    debug!("domain name {domain} resolved to {}", v.ip());
                    entry.insert(v)
                }
            },
        })
    }
}

pub struct UdpListener {
    local_addr: SocketAddr,
    accept_rx: Receiver<io::Result<(UdpSession, SocketAddr)>>,
    abort_handle: AbortHandle,
}

impl UdpListener {
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        let socket = UdpSocket::bind(addr).await?;
        let local_addr = socket.local_addr()?;
        let (accept_tx, accept_rx) = mpsc::channel(32);
        let abort_handle = tokio::spawn(async move {
            let (send_tx, mut send_rx) = mpsc::channel(32);
            let mut clients: HashMap<SocketAddr, Sender<Vec<u8>>> = HashMap::new();
            let mut buf = [0u8; 65536];
            loop {
                tokio::select! {
                    v = socket.recv_from(&mut buf) => {
                        match v {
                            Ok((len, addr)) => {
                                let data = buf[..len].to_vec();
                                match clients.entry(addr) {
                                    Entry::Occupied(entry) => {
                                        entry.into_mut().send(data).await.unwrap_or_default();
                                    }
                                    Entry::Vacant(entry) => {
                                        let (tx, rx) = mpsc::channel(32);
                                        entry.insert(tx).send(data).await.unwrap();
                                        let session = UdpSession::new(addr, send_tx.clone(), rx);
                                        accept_tx.send(Ok((session, addr))).await.unwrap();
                                    }
                                }
                            }
                            Err(err) => accept_tx.send(Err(err)).await.unwrap(),
                        }
                    }
                    Some((data, addr)) = send_rx.recv() => {
                        socket.send_to(data.as_slice(), &addr).await.unwrap_or_default();
                    }
                }
            }
        })
        .abort_handle();
        Ok(Self {
            local_addr,
            accept_rx,
            abort_handle,
        })
    }

    pub async fn accept(&mut self) -> io::Result<(UdpSession, SocketAddr)> {
        self.accept_rx.recv().await.unwrap()
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

impl Drop for UdpListener {
    fn drop(&mut self) {
        self.abort_handle.abort()
    }
}
