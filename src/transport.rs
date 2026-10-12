//! SDK line transport with linear-time bounded framing before JSON allocation.
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use std::io;
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PROMPT_BYTES: usize = 1024 * 1024;
pub fn incoming(
    reader: impl AsyncRead + Unpin + Send + 'static,
) -> impl futures::Stream<Item = io::Result<String>> + Send {
    futures::stream::unfold(
        (reader, Vec::<u8>::new(), false),
        |(mut reader, mut buffer, stopped)| async move {
            if stopped {
                return None;
            }
            let mut scanned = 0;
            loop {
                if let Some(offset) = buffer[scanned..].iter().position(|b| *b == b'\n') {
                    let end = scanned + offset;
                    if end > MAX_FRAME_BYTES {
                        return Some((
                            Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "ACP frame exceeds limit",
                            )),
                            (reader, Vec::new(), true),
                        ));
                    }
                    let rest = buffer.split_off(end + 1);
                    buffer.truncate(end);
                    let line = String::from_utf8(buffer).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid ACP UTF-8")
                    });
                    let stop = line.is_err();
                    return Some((line, (reader, rest, stop)));
                }
                if buffer.len() > MAX_FRAME_BYTES {
                    return Some((
                        Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "ACP frame exceeds limit",
                        )),
                        (reader, Vec::new(), true),
                    ));
                }
                scanned = buffer.len();
                let mut chunk = [0u8; 8192];
                match reader.read(&mut chunk).await {
                    Ok(0) if buffer.is_empty() => return None,
                    Ok(0) => {
                        return Some((
                            Err(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "unterminated ACP frame",
                            )),
                            (reader, Vec::new(), true),
                        ));
                    }
                    Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                    Err(e) => return Some((Err(e), (reader, Vec::new(), true))),
                }
            }
        },
    )
}
pub fn outgoing(
    writer: impl AsyncWrite + Unpin + Send + 'static,
) -> impl futures::Sink<String, Error = io::Error> + Send {
    futures::sink::unfold(writer, |mut writer, line: String| async move {
        if line.len() > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ACP output exceeds limit",
            ));
        }
        writer.write_all(line.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        Ok(writer)
    })
}
