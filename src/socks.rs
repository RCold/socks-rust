mod error;
mod socks4;
mod socks5;

use crate::socks::error::Error;
use log::{debug, error};
use std::net::SocketAddr;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};

async fn handle_socks_tcp(mut stream: TcpStream, client_addr: SocketAddr) -> Result<(), Error> {
    match stream.read_u8().await? {
        4u8 => {
            debug!("handle socks4 request from client {client_addr}");
            socks4::handle_tcp(stream, client_addr).await
        }
        5u8 => {
            debug!("handle socks5 request from client {client_addr}");
            socks5::handle_tcp(stream, client_addr).await
        }
        _ => Err(Error::VersionMismatch),
    }
}

pub async fn start_socks_server(tcp_listener: TcpListener) {
    loop {
        match tcp_listener.accept().await {
            Ok((stream, client_addr)) => {
                stream.set_nodelay(true).unwrap_or_default();
                tokio::spawn(async move {
                    debug!("client {client_addr} connected");
                    if let Err(err) = handle_socks_tcp(stream, client_addr).await {
                        error!("failed to handle socks request from client {client_addr}: {err}");
                    }
                    debug!("client {client_addr} disconnected");
                });
            }
            Err(err) => {
                error!("failed to accept tcp connection from a client: {err}");
            }
        }
    }
}
