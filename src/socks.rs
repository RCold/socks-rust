mod error;
mod socks4;
mod socks5;

use crate::socks::error::{Error, ErrorKind};
use crate::socks::socks5::UdpSessionManager;
use log::{debug, error};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::select;
use tokio::sync::Mutex;

async fn handle_socks_tcp(
    mut stream: TcpStream,
    client_addr: SocketAddr,
    manager: Arc<Mutex<UdpSessionManager>>,
) -> Result<(), Error> {
    match stream.read_u8().await? {
        4u8 => {
            debug!("handle socks4 request from client {client_addr}");
            socks4::handle_tcp(stream, client_addr).await
        }
        5u8 => {
            debug!("handle socks5 request from client {client_addr}");
            socks5::handle_tcp(stream, client_addr, manager).await
        }
        _ => Err(Error::new(ErrorKind::VersionMismatch)),
    }
}

async fn handle_tcp(
    stream: TcpStream,
    client_addr: SocketAddr,
    manager: Arc<Mutex<UdpSessionManager>>,
) {
    debug!("client {client_addr} connected");
    if let Err(err) = handle_socks_tcp(stream, client_addr, manager).await {
        error!("failed to handle socks request from client {client_addr}: {err}");
    }
    debug!("client {client_addr} disconnected");
}

async fn handle_udp(
    data: Vec<u8>,
    client_addr: SocketAddr,
    manager: Arc<Mutex<UdpSessionManager>>,
) {
    if let Err(err) = socks5::handle_udp(data.as_slice(), client_addr, manager).await {
        error!("failed to handle socks udp packet from client {client_addr}: {err}");
    }
}

pub async fn start_socks_server(tcp_listener: TcpListener, udp_socket: UdpSocket) {
    let client_socket = Arc::new(udp_socket);
    let manager = Arc::new(Mutex::new(UdpSessionManager::new(client_socket.clone())));
    let mut udp_buf = [0u8; 65536];
    loop {
        select! {
            v = tcp_listener.accept() => {
                match v {
                    Ok((stream, client_addr)) => {
                        tokio::spawn(handle_tcp(stream, client_addr, manager.clone()));
                    }
                    Err(err) => {
                        error!("failed to accept tcp connection from client: {err}");
                    }
                }
            }
            v = client_socket.recv_from(&mut udp_buf) => {
                match v {
                    Ok((len, client_addr)) => {
                        tokio::spawn(handle_udp(udp_buf[..len].to_vec(), client_addr, manager.clone()));
                    }
                    Err(err) => {
                        error!("failed to receive udp packet from client: {err}");
                    }
                }
            }
        }
    }
}
