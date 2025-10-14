// SPDX-License-Identifier: Apache-2.0
// Copyright (C) 2025 Yeuham Wang <rcold@rcold.name>

use crate::error::Error;
use log::{debug, info};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use tokio::io;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::TcpStream;

#[repr(u8)]
enum Command {
    Connect = 1u8,
    Bind = 2u8,
}

#[repr(u8)]
enum ReplyCode {
    RequestGranted = 90u8,
    RequestRejectedOrFailed = 91u8,
}

async fn send_response(stream: &mut BufStream<TcpStream>, rep: ReplyCode) -> io::Result<()> {
    stream
        .write_all(&[0u8, rep as u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8])
        .await?;
    stream.flush().await
}

async fn handle_connect(
    mut stream: BufStream<TcpStream>,
    remote_addr: String,
) -> Result<(), Error> {
    match TcpStream::connect(&remote_addr).await {
        Ok(mut remote) => {
            remote.set_nodelay(true).unwrap_or_default();
            debug!("tcp://{remote_addr} connected");
            send_response(&mut stream, ReplyCode::RequestGranted).await?;
            io::copy_bidirectional(&mut stream, &mut remote).await?;
            debug!("tcp://{remote_addr} disconnected");
            Ok(())
        }
        Err(err) => {
            send_response(&mut stream, ReplyCode::RequestRejectedOrFailed)
                .await
                .unwrap_or_default();
            Err(err.into())
        }
    }
}

pub async fn handle_tcp(stream: TcpStream, client_addr: SocketAddr) -> Result<(), Error> {
    let mut stream = BufStream::new(stream);
    let cmd = stream.read_u8().await?;
    if ![Command::Connect as u8, Command::Bind as u8].contains(&cmd) {
        send_response(&mut stream, ReplyCode::RequestRejectedOrFailed)
            .await
            .unwrap_or_default();
        return Err(Error::InvalidCommand);
    }
    let port = stream.read_u16().await?;
    let mut ip = [0u8; 4];
    stream.read_exact(&mut ip).await?;
    let mut stream = stream.take(255);
    let mut _user_id = Vec::new();
    stream.read_until(0u8, &mut _user_id).await?;
    let addr = if ip[..3] == [0u8, 0u8, 0u8] && ip[3] != 0u8 {
        let mut domain = Vec::new();
        stream.set_limit(255);
        stream.read_until(0u8, &mut domain).await?;
        domain.pop();
        if domain.is_empty() {
            return Err(Error::InvalidDomainName);
        }
        String::from_utf8(domain).map_err(|_| Error::InvalidDomainName)?
    } else {
        Ipv4Addr::from(ip).to_string()
    };
    let remote_addr = if let Ok(addr) = Ipv6Addr::from_str(&addr) {
        format!("[{addr}]:{port}")
    } else {
        format!("{addr}:{port}")
    };
    let mut stream = stream.into_inner();
    if cmd == Command::Connect as u8 {
        info!("socks4 connect request from client {client_addr} to tcp://{remote_addr} accepted");
        handle_connect(stream, remote_addr).await?;
    } else if cmd == Command::Bind as u8 {
        info!("socks4 bind request from client {client_addr} rejected: not implemented");
        send_response(&mut stream, ReplyCode::RequestRejectedOrFailed).await?;
    }
    Ok(())
}
