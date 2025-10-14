// SPDX-License-Identifier: Apache-2.0
// Copyright (C) 2025 Yeuham Wang <rcold@rcold.name>

use crate::error::Error;
use tokio::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[repr(u8)]
enum Method {
    NoAuth = 0x00u8,
    _UserPass = 0x02u8,
    NoAcceptable = 0xFFu8,
}

async fn send_response<W: AsyncWrite + Unpin>(writer: &mut W, method: Method) -> io::Result<()> {
    writer.write_all(&[5u8, method as u8]).await?;
    writer.flush().await
}

pub async fn authenticate<RW>(stream: &mut RW) -> Result<bool, Error>
where
    RW: AsyncRead + AsyncWrite + Unpin,
{
    let len = stream.read_u8().await? as usize;
    if len < 1 {
        return Err(Error::InvalidAuthMethod);
    }
    let mut methods = vec![0u8; len];
    stream.read_exact(&mut methods).await?;
    if methods.contains(&(Method::NoAuth as u8)) {
        send_response(stream, Method::NoAuth).await?;
        Ok(true)
    } else {
        send_response(stream, Method::NoAcceptable).await?;
        Ok(false)
    }
}
