// SPDX-License-Identifier: Apache-2.0
// Copyright (C) 2025 Yeuham Wang <rcold@rcold.name>

use crate::error::Error;
use crate::socks5::Address;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::net::SocketAddr;
use tokio::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
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

    pub async fn write_to<W: AsyncWrite + Unpin>(&self, writer: &mut W) -> Result<(), Error> {
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
}

impl UdpSession {
    fn new(
        peer_addr: SocketAddr,
        tx: Sender<(Vec<u8>, SocketAddr)>,
        rx: Receiver<Vec<u8>>,
    ) -> Self {
        Self { peer_addr, tx, rx }
    }

    pub fn send(&self, data: Vec<u8>) {
        self.tx.try_send((data, self.peer_addr)).unwrap_or_default();
    }

    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        self.rx.recv().await
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
            let mut clients: HashMap<_, Sender<Vec<u8>>> = HashMap::new();
            let mut buf = [0u8; 65536];
            loop {
                tokio::select! {
                    v = socket.recv_from(&mut buf) => {
                        match v {
                            Ok((len, addr)) => {
                                let data = buf[..len].to_vec();
                                match clients.entry(addr) {
                                    Entry::Occupied(entry) => {
                                        entry.into_mut().try_send(data).unwrap_or_default();
                                    }
                                    Entry::Vacant(entry) => {
                                        let (tx, rx) = mpsc::channel(32);
                                        entry.insert(tx).try_send(data).unwrap_or_default();
                                        let session = UdpSession::new(addr, send_tx.clone(), rx);
                                        accept_tx.try_send(Ok((session, addr))).unwrap_or_default();
                                    }
                                }
                            }
                            Err(err) => accept_tx.try_send(Err(err)).unwrap_or_default(),
                        }
                    }
                    Some((data, addr)) = send_rx.recv() => {
                        socket.try_send_to(data.as_slice(), addr).unwrap_or_default();
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
