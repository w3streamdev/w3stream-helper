//! Chrome native-messaging stdio framing:
//!   each message is `[u32 little-endian length][JSON bytes]`.
//! Max length per spec: 1MB. We refuse anything larger to avoid memory abuse.

use anyhow::{anyhow, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde_json::Value;
use std::io::{ErrorKind, Read, Write};

const MAX_MESSAGE_LEN: u32 = 1024 * 1024;

pub fn read_message<R: Read>(reader: &mut R) -> Result<Option<Value>> {
    let len = match reader.read_u32::<LittleEndian>() {
        Ok(n) => n,
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(anyhow!(e).context("reading length prefix")),
    };
    if len == 0 {
        return Ok(Some(Value::Null));
    }
    if len > MAX_MESSAGE_LEN {
        return Err(anyhow!("message length {len} exceeds limit {MAX_MESSAGE_LEN}"));
    }
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).context("reading body")?;
    let val: Value = serde_json::from_slice(&buf).context("parsing JSON")?;
    Ok(Some(val))
}

pub fn write_message<W: Write>(writer: &mut W, value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    let len = body.len();
    if len > MAX_MESSAGE_LEN as usize {
        return Err(anyhow!("outbound message {len} bytes exceeds limit"));
    }
    writer.write_u32::<LittleEndian>(len as u32)?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip() {
        let mut buf = Vec::new();
        let v = serde_json::json!({"hello": "world"});
        write_message(&mut buf, &v).unwrap();
        let mut cur = Cursor::new(buf);
        let out = read_message(&mut cur).unwrap().unwrap();
        assert_eq!(out, v);
    }

    #[test]
    fn eof_returns_none() {
        let mut cur = Cursor::new(Vec::new());
        assert!(read_message(&mut cur).unwrap().is_none());
    }

    #[test]
    fn rejects_oversize() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(MAX_MESSAGE_LEN + 1).to_le_bytes());
        let mut cur = Cursor::new(buf);
        assert!(read_message(&mut cur).is_err());
    }
}
