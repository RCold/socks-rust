mod address;
mod auth;
mod command;
mod tcp;
mod udp;

use crate::socks::error::Error;
use crate::socks::socks5::address::Address;
use crate::socks::socks5::command::Command;
use crate::socks::socks5::tcp::{Reply, ReplyCode, Request};
use crate::socks::socks5::udp::{UdpHeader, UdpListener, UdpSession};
use log::{debug, error, info};
use std::net::{IpAddr, SocketAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufStream, BufWriter};
use tokio::net::{TcpStream, UdpSocket};
use tokio::{io, select};

async fn handle_connect(stream: &mut BufStream<TcpStream>, remote_addr: &str) -> io::Result<()> {
    match TcpStream::connect(remote_addr).await {
        Ok(remote) => {
            debug!("tcp://{remote_addr} connected");
            Reply::new(ReplyCode::Succeeded as u8, None)
                .write_to(stream)
                .await?;
            let mut remote = BufStream::new(remote);
            io::copy_bidirectional(stream, &mut remote).await?;
            debug!("tcp://{remote_addr} disconnected");
            Ok(())
        }
        Err(err) => {
            Reply::new(ReplyCode::GeneralFailure as u8, None)
                .write_to(stream)
                .await
                .unwrap_or_default();
            Err(err)
        }
    }
}

async fn handle_remote_udp(
    session: &UdpSession,
    data: &[u8],
    remote_addr: SocketAddr,
) -> io::Result<()> {
    let mut writer = BufWriter::new(Vec::new());
    UdpHeader::new(Address::SocketAddress(remote_addr))
        .write_to(&mut writer)
        .await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    session.send(writer.into_inner()).await
}

async fn handle_udp(mut session: UdpSession) -> Result<(), Error> {
    let remote_socket_v4 = UdpSocket::bind("0.0.0.0:0").await?;
    let remote_socket_v6 = UdpSocket::bind("[::]:0").await?;
    let mut buf_v4 = [0u8; 65536];
    let mut buf_v6 = [0u8; 65536];
    loop {
        select! {
            v = session.recv() => {
                match v {
                    Some(data) => {
                        let mut reader = BufReader::new(data.as_slice());
                        let header = UdpHeader::read_from(&mut reader).await?;
                        let remote_addr = session.resolve_addr(header.addr()).await?;
                        let remote_socket = match remote_addr {
                            SocketAddr::V4(_) => &remote_socket_v4,
                            SocketAddr::V6(_) => &remote_socket_v6,
                        };
                        let mut data = Vec::new();
                        reader.read_to_end(&mut data).await?;
                        remote_socket.send_to(data.as_slice(), &remote_addr).await?;
                    }
                    None => {
                        break;
                    }
                }
            }
            Ok((len, remote_addr)) = remote_socket_v4.recv_from(&mut buf_v4) => {
                handle_remote_udp(&session, &buf_v4[..len], remote_addr).await?;
            }
            Ok((len, remote_addr)) = remote_socket_v6.recv_from(&mut buf_v6) => {
                handle_remote_udp(&session, &buf_v6[..len], remote_addr).await?;
            }
        }
    }
    Ok(())
}

async fn handle_udp_associate(
    stream: &mut BufStream<TcpStream>,
    client_ip: IpAddr,
) -> io::Result<()> {
    let mut bind = stream.get_ref().local_addr()?;
    bind.set_port(0);
    let mut udp_listener;
    match UdpListener::bind(bind).await {
        Ok(v) => {
            udp_listener = v;
        }
        Err(err) => {
            Reply::new(ReplyCode::GeneralFailure as u8, None)
                .write_to(stream)
                .await
                .unwrap_or_default();
            return Err(err);
        }
    }
    Reply::new(
        ReplyCode::Succeeded as u8,
        Some(Address::SocketAddress(udp_listener.local_addr())),
    )
    .write_to(stream)
    .await?;
    let mut buf = [0u8; 65536];
    loop {
        select! {
            v = stream.read(&mut buf) => if v? == 0 { break },
            v = udp_listener.accept() => {
                match v {
                    Ok((session, client_addr)) => {
                        if client_addr.ip() != client_ip {
                            error!("invalid udp packet received from {client_addr}");
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
                        error!("failed to receive udp packet: {err}");
                    }
                }
            }
        }
    }
    Ok(())
}

pub async fn handle_tcp(stream: TcpStream, client_addr: SocketAddr) -> Result<(), Error> {
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
            handle_udp_associate(&mut stream, client_addr.ip()).await?;
        }
    }
    Ok(())
}
