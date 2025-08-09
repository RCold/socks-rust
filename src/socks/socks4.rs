use crate::socks::error::{Error, ErrorKind};
use log::{debug, info};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::TcpStream;

#[repr(u8)]
enum Reply {
    RequestGranted = 90u8,
    RequestRejectedOrFailed = 91u8,
}

async fn send_response(stream: &mut BufStream<TcpStream>, rep: Reply) -> Result<(), Error> {
    stream
        .write_all(&[0u8, rep as u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8])
        .await?;
    stream.flush().await?;
    Ok(())
}

pub async fn handle_tcp(stream: TcpStream, client_addr: SocketAddr) -> Result<(), Error> {
    let mut stream = BufStream::new(stream);
    let cmd = stream.read_u8().await?;
    if cmd != 1u8 {
        send_response(&mut stream, Reply::RequestRejectedOrFailed).await?;
        return Err(Error::new(ErrorKind::CommandNotSupported));
    }
    let port = stream.read_u16().await?;
    let mut ip = [0u8; 4];
    stream.read_exact(&mut ip).await?;

    let mut stream = stream.take(255);
    let mut _user_id = Vec::new();
    stream.read_until(0u8, &mut _user_id).await?;

    let mut stream = stream.into_inner().take(255);
    let addr = if ip[..3] == [0u8, 0u8, 0u8] && ip[3] != 0u8 {
        let mut domain = Vec::new();
        stream.read_until(0u8, &mut domain).await?;
        domain.pop();
        String::from_utf8(domain).map_err(|_| Error::new(ErrorKind::InvalidDomainName))?
    } else {
        Ipv4Addr::from(ip).to_string()
    };
    let remote_addr = if let Ok(addr) = Ipv6Addr::from_str(&addr) {
        format!("[{addr}]:{port}")
    } else {
        format!("{addr}:{port}")
    };

    let mut stream = stream.into_inner();
    info!("socks4 request from client {client_addr} to tcp://{remote_addr} accepted");
    match TcpStream::connect(&remote_addr).await {
        Ok(mut remote) => {
            debug!("tcp://{remote_addr} connected");
            send_response(&mut stream, Reply::RequestGranted).await?;
            tokio::io::copy_bidirectional(&mut stream, &mut remote).await?;
            debug!("tcp://{remote_addr} disconnected");
            Ok(())
        }
        Err(err) => {
            send_response(&mut stream, Reply::RequestRejectedOrFailed).await?;
            Err(err.into())
        }
    }
}
