mod address;
mod auth;
mod tcp;
mod udp;

use crate::socks::error::Error;
use crate::socks::socks5::address::Address;
use crate::socks::socks5::tcp::{Command, Reply, ReplyCode, Request};
pub use crate::socks::socks5::udp::handle_client as handle_udp;
pub use crate::socks::socks5::udp::SessionManager as UdpSessionManager;
use log::{debug, error, info};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, BufStream};
use tokio::net::TcpStream;
use tokio::select;
use tokio::sync::Mutex;

async fn handle_remote_udp(
    data: Vec<u8>,
    remote_addr: SocketAddr,
    client_ip: IpAddr,
    manager: Arc<Mutex<UdpSessionManager>>,
) {
    if let Err(err) = udp::handle_remote(data.as_slice(), remote_addr, client_ip, manager).await {
        error!("failed to handle udp packet from remote {remote_addr}: {err}");
    }
}

async fn handle_connect(stream: &mut BufStream<TcpStream>, remote_addr: &str) -> Result<(), Error> {
    match TcpStream::connect(remote_addr).await {
        Ok(mut remote) => {
            debug!("tcp://{remote_addr} connected");
            Reply::new(ReplyCode::Succeeded as u8, None)
                .write_to(stream)
                .await?;
            tokio::io::copy_bidirectional(stream, &mut remote).await?;
            debug!("tcp://{remote_addr} disconnected");
            Ok(())
        }
        Err(err) => {
            Reply::new(ReplyCode::GeneralFailure as u8, None)
                .write_to(stream)
                .await
                .unwrap_or(());
            Err(err.into())
        }
    }
}

async fn handle_udp_associate(
    stream: &mut BufStream<TcpStream>,
    client_ip: IpAddr,
    manager: Arc<Mutex<UdpSessionManager>>,
) -> Result<(), Error> {
    let remote_socket_v4;
    let remote_socket_v6;
    let local_addr;
    {
        let mut manager = manager.lock().await;
        local_addr = manager.client_socket().local_addr()?;
        let session = match manager.open_session(client_ip).await {
            Ok(v) => v,
            Err(err) => {
                Reply::new(ReplyCode::GeneralFailure as u8, None)
                    .write_to(stream)
                    .await
                    .unwrap_or(());
                return Err(err.into());
            }
        };
        remote_socket_v4 = session.remote_socket_v4();
        remote_socket_v6 = session.remote_socket_v6();
    }
    Reply::new(
        ReplyCode::Succeeded as u8,
        Some(Address::SocketAddress(local_addr)),
    )
    .write_to(stream)
    .await?;
    let mut tcp_buf = [0u8; 65536];
    let mut udp_v4_buf = [0u8; 65536];
    let mut udp_v6_buf = [0u8; 65536];
    loop {
        select! {
            v = stream.read(&mut tcp_buf) => if v? == 0 { break; },
            v = remote_socket_v4.recv_from(&mut udp_v4_buf) => {
                match v {
                    Ok((len, remote_addr)) => {
                        tokio::spawn(handle_remote_udp(udp_v4_buf[..len].to_vec(), remote_addr, client_ip, manager.clone()));
                    }
                    Err(err) => {
                        error!("failed to receive udp packet from remote: {err}");
                    }
                }
            }
            v = remote_socket_v6.recv_from(&mut udp_v6_buf) => {
                match v {
                    Ok((len, remote_addr)) => {
                        tokio::spawn(handle_remote_udp(udp_v6_buf[..len].to_vec(), remote_addr, client_ip, manager.clone()));
                    }
                    Err(err) => {
                        error!("failed to receive udp packet from remote: {err}");
                    }
                }
            }
        }
    }
    {
        let mut manager = manager.lock().await;
        manager.close_session(&client_ip);
    }
    Ok(())
}

pub async fn handle_tcp(
    stream: TcpStream,
    client_addr: SocketAddr,
    manager: Arc<Mutex<UdpSessionManager>>,
) -> Result<(), Error> {
    let mut stream = BufStream::new(stream);
    auth::authenticate(&mut stream).await?;
    let request = Request::read_from(&mut stream).await?;
    let remote_addr = request.addr.to_string();

    match request.cmd {
        Command::Connect => {
            info!(
                "socks5 connect request from client {client_addr} to tcp://{remote_addr} accepted"
            );
            handle_connect(&mut stream, &remote_addr).await?;
        }
        Command::Bind => {
            info!("socks5 bind request from client {client_addr} rejected: not implemented");
            Reply::new(ReplyCode::CommandNotSupported as u8, None)
                .write_to(&mut stream)
                .await?;
        }
        Command::UdpAssociate => {
            info!(
                "socks5 udp associate request from client {client_addr} to udp://{remote_addr} accepted"
            );
            handle_udp_associate(&mut stream, client_addr.ip(), manager).await?;
        }
    }

    Ok(())
}
