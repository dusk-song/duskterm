// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2017-2020 Alexey Arbuzov
// Copyright (c) 2023-2026 Jarkko Sakkinen

//! ZMODEM file metadata helpers.

use crate::buffer::Buffer;
use crate::error::Error;
use crate::header::{Encoding, Frame, Header};
use crate::io::Write;
use crate::wire::{SUBPACKET_MAX_SIZE, SubpacketType, write_subpacket};
use core::fmt::Write as _;

/// Parses a u32 from a slice of ASCII decimal bytes.
pub(crate) fn parse_file_size(bytes: &[u8]) -> Result<u32, Error> {
    if bytes.is_empty() {
        return Ok(0);
    }

    let mut result: u32 = 0;
    for &byte in bytes {
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            _ => return Err(Error::MalformedFileSize),
        };
        result = result
            .checked_mul(10)
            .and_then(|r| r.checked_add(digit))
            .ok_or(Error::MalformedFileSize)?;
    }
    Ok(result)
}

/// Write ZRFILE
pub(crate) fn write_zfile<P>(
    port: &mut P,
    buf: &mut Buffer<SUBPACKET_MAX_SIZE>,
    name: &[u8],
    size: u32,
    modified_unix_seconds: Option<u64>,
) -> Result<Option<()>, Error>
where
    P: Write + ?Sized,
{
    buf.clear();
    buf.extend_from_slice(name)
        .map_err(|_| Error::OutOfMemory)?;
    buf.push(b'\0').map_err(|_| Error::OutOfMemory)?;

    if let Some(modified) = modified_unix_seconds {
        // ZMODEM encodes the size in decimal, then the modification time and
        // mode in octal. Mode 0 keeps the receiver's platform defaults.
        write!(buf, "{size} {modified:o} 0\0").map_err(|_| Error::OutOfMemory)?;
    } else {
        write!(buf, "{size}\0").map_err(|_| Error::OutOfMemory)?;
    }

    if Header::new(Encoding::ZBIN32, Frame::ZFILE, [0; 4])
        .write(port)?
        .is_none()
    {
        return Ok(None);
    }
    write_subpacket(port, Encoding::ZBIN32, SubpacketType::ZCRCW, buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_file_size_as_zero() {
        assert_eq!(parse_file_size(b""), Ok(0));
    }

    #[test]
    fn reject_bad_file_size() {
        assert_eq!(parse_file_size(b"12x"), Err(Error::MalformedFileSize));
    }

    #[test]
    fn writes_standard_zfile_metadata() {
        let mut wire = Buffer::<4096>::new();
        let mut port = crate::wire::BufferWriter::new(&mut wire);
        let mut metadata = Buffer::<SUBPACKET_MAX_SIZE>::new();

        write_zfile(
            &mut port,
            &mut metadata,
            b"archive.tar",
            42,
            Some(1_700_000_000),
        )
        .unwrap();

        assert_eq!(
            &metadata[..],
            format!("archive.tar\0{} {:o} 0\0", 42, 1_700_000_000u64).as_bytes()
        );
    }
}
