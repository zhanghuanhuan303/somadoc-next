//! Minimal zip reader for `.docx` archives.
//!
//! Only what OOXML needs is implemented: the end-of-central-directory record,
//! the central directory, local file headers, and the stored/deflated methods.
//! Zip64, encryption, multi-disk archives, and the optional data descriptor are
//! not modelled; a data descriptor is harmless because the central directory is
//! the source of truth for sizes and offsets.
//!
//! Every index is checked. A malformed archive returns a static reason instead
//! of panicking, and no entry is materialised beyond [`MAX_PART_BYTES`].

use alloc::vec::Vec;

use crate::inflate;
use crate::DocxError;

const EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const CENTRAL_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const LOCAL_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];

/// Largest part this reader will materialise. `word/document.xml` and
/// `docProps/core.xml` are XML text, not embedded assets; a declared size above
/// this is treated as hostile rather than as a huge document.
const MAX_PART_BYTES: usize = 64 * 1024 * 1024;

fn not_zip() -> DocxError {
    DocxError::new("not a zip archive")
}

fn corrupt_directory() -> DocxError {
    DocxError::new("zip central directory is corrupt")
}

fn corrupt_entry() -> DocxError {
    DocxError::new("zip entry is corrupt")
}

/// One central-directory record. The name borrows the archive.
struct Entry<'a> {
    name: &'a [u8],
    flags: u16,
    method: u16,
    crc: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    local_offset: u32,
}

/// An archive ready for random access by part name.
pub(crate) struct Archive<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry<'a>>,
}

impl<'a> Archive<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Result<Self, DocxError> {
        let eocd = find_eocd(bytes).ok_or_else(not_zip)?;
        let disk = le16(bytes, eocd + 4).ok_or_else(not_zip)?;
        let central_disk = le16(bytes, eocd + 6).ok_or_else(not_zip)?;
        let entries_on_disk = le16(bytes, eocd + 8).ok_or_else(not_zip)?;
        let total = le16(bytes, eocd + 10).ok_or_else(not_zip)?;
        let central_size = le32(bytes, eocd + 12).ok_or_else(not_zip)?;
        let central_offset = le32(bytes, eocd + 16).ok_or_else(not_zip)?;

        // 0xffff/0xffff_ffff are the zip64 sentinels. Reading them as ordinary
        // numbers would silently point at the wrong bytes.
        if total == u16::MAX || central_size == u32::MAX || central_offset == u32::MAX {
            return Err(DocxError::new("zip64 archives are unsupported"));
        }
        if disk != 0 || central_disk != 0 || entries_on_disk != total {
            return Err(DocxError::new("multi-disk zip archives are unsupported"));
        }

        let start = central_offset as usize;
        let directory_end = start
            .checked_add(central_size as usize)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(corrupt_directory)?;

        let mut entries = Vec::with_capacity(total as usize);
        let mut position = start;
        for _ in 0..total {
            if !has_signature(bytes, position, &CENTRAL_SIGNATURE) {
                return Err(corrupt_directory());
            }
            if position
                .checked_add(46)
                .is_none_or(|header_end| header_end > directory_end)
            {
                return Err(corrupt_directory());
            }

            let flags = le16(bytes, position + 8).ok_or_else(corrupt_directory)?;
            let method = le16(bytes, position + 10).ok_or_else(corrupt_directory)?;
            let crc = le32(bytes, position + 16).ok_or_else(corrupt_directory)?;
            let compressed_size = le32(bytes, position + 20).ok_or_else(corrupt_directory)?;
            let uncompressed_size = le32(bytes, position + 24).ok_or_else(corrupt_directory)?;
            let name_len = le16(bytes, position + 28).ok_or_else(corrupt_directory)? as usize;
            let extra_len = le16(bytes, position + 30).ok_or_else(corrupt_directory)? as usize;
            let comment_len = le16(bytes, position + 32).ok_or_else(corrupt_directory)? as usize;
            let local_offset = le32(bytes, position + 42).ok_or_else(corrupt_directory)?;

            let name_start = position + 46;
            let name_end = name_start
                .checked_add(name_len)
                .ok_or_else(corrupt_directory)?;
            let next = name_end
                .checked_add(extra_len)
                .and_then(|next| next.checked_add(comment_len))
                .filter(|next| *next <= directory_end)
                .ok_or_else(corrupt_directory)?;

            entries.push(Entry {
                name: bytes
                    .get(name_start..name_end)
                    .ok_or_else(corrupt_directory)?,
                flags,
                method,
                crc,
                compressed_size,
                uncompressed_size,
                local_offset,
            });
            position = next;
        }

        Ok(Self { bytes, entries })
    }

    /// Read and decompress one part by name. Returns `None` when the name is
    /// absent; a present but corrupt entry is an error because silently
    /// treating it as absent would hide a broken document.
    pub(crate) fn read(&self, wanted: &[u8]) -> Result<Option<Vec<u8>>, DocxError> {
        let Some(entry) = self
            .entries
            .iter()
            .find(|entry| matches_name(entry.name, wanted))
        else {
            return Ok(None);
        };

        if entry.flags & 1 != 0 {
            return Err(DocxError::new("zip entry is encrypted"));
        }
        if entry.method != 0 && entry.method != 8 {
            return Err(DocxError::new("zip entry compression is unsupported"));
        }

        let offset = entry.local_offset as usize;
        if !has_signature(self.bytes, offset, &LOCAL_SIGNATURE) {
            return Err(corrupt_entry());
        }
        if offset
            .checked_add(30)
            .is_none_or(|end| end > self.bytes.len())
        {
            return Err(corrupt_entry());
        }

        let local_method = le16(self.bytes, offset + 8).ok_or_else(corrupt_entry)?;
        let local_crc = le32(self.bytes, offset + 14).ok_or_else(corrupt_entry)?;
        let local_compressed = le32(self.bytes, offset + 18).ok_or_else(corrupt_entry)?;
        let local_uncompressed = le32(self.bytes, offset + 22).ok_or_else(corrupt_entry)?;
        let name_len = le16(self.bytes, offset + 26).ok_or_else(corrupt_entry)? as usize;
        let extra_len = le16(self.bytes, offset + 28).ok_or_else(corrupt_entry)? as usize;

        if local_method != entry.method {
            return Err(corrupt_entry());
        }

        let data_start = offset
            .checked_add(30)
            .and_then(|start| start.checked_add(name_len))
            .and_then(|start| start.checked_add(extra_len))
            .filter(|start| *start <= self.bytes.len())
            .ok_or_else(corrupt_entry)?;

        // Central-directory sizes are authoritative. Falling back to the local
        // header accepts small hand-built archives that leave one copy zeroed.
        let compressed_size = if entry.compressed_size != 0 {
            entry.compressed_size
        } else {
            local_compressed
        } as usize;
        let hint = if entry.uncompressed_size != 0 {
            entry.uncompressed_size
        } else {
            local_uncompressed
        } as usize;

        let data_end = data_start
            .checked_add(compressed_size)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(corrupt_entry)?;
        let data = &self.bytes[data_start..data_end];

        if hint > MAX_PART_BYTES || data.len() > MAX_PART_BYTES {
            return Err(DocxError::new("zip entry is too large"));
        }
        let limit = if hint == 0 { MAX_PART_BYTES } else { hint };
        let output = match entry.method {
            0 => {
                if data.len() > limit {
                    return Err(DocxError::new("zip entry is too large"));
                }
                data.to_vec()
            }
            8 => inflate::inflate(data, limit)?,
            _ => return Err(DocxError::new("zip entry compression is unsupported")),
        };

        if hint != 0 && output.len() != hint {
            return Err(corrupt_entry());
        }

        // A zero CRC is common in minimal hand-written fixtures; when a real
        // checksum is present it is never skipped.
        let expected_crc = if entry.crc != 0 { entry.crc } else { local_crc };
        if expected_crc != 0 && crc32(&output) != expected_crc {
            return Err(DocxError::new("zip entry checksum does not match"));
        }

        Ok(Some(output))
    }
}

/// A leading slash is not part of an OPC part name, but some producers store
/// one. Accept either spelling; do not accept a case-insensitive match.
fn matches_name(entry: &[u8], wanted: &[u8]) -> bool {
    entry == wanted || (entry.first() == Some(&b'/') && entry.get(1..) == Some(wanted))
}

fn has_signature(bytes: &[u8], at: usize, signature: &[u8; 4]) -> bool {
    let Some(end) = at.checked_add(4) else {
        return false;
    };
    bytes.get(at..end) == Some(&signature[..])
}

fn find_eocd(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 22 {
        return None;
    }
    let last = bytes.len() - 22;
    // The comment is at most 65535 bytes, so a valid record cannot start more
    // than that far from the end.
    let first = last.saturating_sub(u16::MAX as usize);
    let mut position = last;
    loop {
        if has_signature(bytes, position, &EOCD_SIGNATURE) {
            let comment = le16(bytes, position + 20)?;
            let end = position
                .checked_add(22)
                .and_then(|end| end.checked_add(comment as usize))?;
            if end <= bytes.len() {
                return Some(position);
            }
        }
        if position == first {
            return None;
        }
        position -= 1;
    }
}

fn le16(data: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    let bytes = data.get(at..end)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn le32(data: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let bytes = data.get(at..end)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0usize;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                0xedb8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

fn crc32(data: &[u8]) -> u32 {
    let mut value = 0xffff_ffffu32;
    for byte in data {
        let slot = ((value ^ u32::from(*byte)) & 0xff) as usize;
        value = CRC_TABLE[slot] ^ (value >> 8);
    }
    !value
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Spec<'a> {
        name: &'a str,
        method: u16,
        data: &'a [u8],
        compressed: &'a [u8],
    }

    fn put_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn build(entries: &[Spec<'_>]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut offsets = Vec::new();

        for entry in entries {
            offsets.push(out.len() as u32);
            out.extend_from_slice(&LOCAL_SIGNATURE);
            put_u16(&mut out, 20);
            put_u16(&mut out, 0);
            put_u16(&mut out, entry.method);
            put_u16(&mut out, 0);
            put_u16(&mut out, 0);
            put_u32(&mut out, crc32(entry.data));
            put_u32(&mut out, entry.compressed.len() as u32);
            put_u32(&mut out, entry.data.len() as u32);
            put_u16(&mut out, entry.name.len() as u16);
            put_u16(&mut out, 0);
            out.extend_from_slice(entry.name.as_bytes());
            out.extend_from_slice(entry.compressed);
        }

        let directory_offset = out.len();
        for (entry, offset) in entries.iter().zip(&offsets) {
            out.extend_from_slice(&CENTRAL_SIGNATURE);
            put_u16(&mut out, 20);
            put_u16(&mut out, 20);
            put_u16(&mut out, 0);
            put_u16(&mut out, entry.method);
            put_u16(&mut out, 0);
            put_u16(&mut out, 0);
            put_u32(&mut out, crc32(entry.data));
            put_u32(&mut out, entry.compressed.len() as u32);
            put_u32(&mut out, entry.data.len() as u32);
            put_u16(&mut out, entry.name.len() as u16);
            put_u16(&mut out, 0);
            put_u16(&mut out, 0);
            put_u16(&mut out, 0);
            put_u16(&mut out, 0);
            put_u32(&mut out, 0);
            put_u32(&mut out, *offset);
            out.extend_from_slice(entry.name.as_bytes());
        }
        let directory_size = out.len() - directory_offset;

        out.extend_from_slice(&EOCD_SIGNATURE);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u16(&mut out, entries.len() as u16);
        put_u16(&mut out, entries.len() as u16);
        put_u32(&mut out, directory_size as u32);
        put_u32(&mut out, directory_offset as u32);
        put_u16(&mut out, 0);
        out
    }

    #[test]
    fn crc32_matches_the_standard_vector() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn a_stored_entry_is_returned() {
        let data = b"<w:document/>";
        let archive = build(&[Spec {
            name: "word/document.xml",
            method: 0,
            data,
            compressed: data,
        }]);
        let archive = Archive::new(&archive).expect("valid archive");
        assert_eq!(
            archive.read(b"word/document.xml").expect("read"),
            Some(data.to_vec())
        );
        assert_eq!(archive.read(b"word/missing.xml").expect("read"), None);
    }

    #[test]
    fn a_deflated_entry_is_returned() {
        // One fixed-Huffman block for `<w:t>fixed Huffman stream</w:t>`.
        const COMPRESSED: &[u8] = &[
            0xb3, 0x29, 0xb7, 0x2a, 0xb1, 0x4b, 0xcb, 0xac, 0x48, 0x4d, 0x51, 0xf0, 0x28, 0x4d,
            0x4b, 0xcb, 0x4d, 0xcc, 0x53, 0x28, 0x2e, 0x29, 0x4a, 0x4d, 0xcc, 0xb5, 0xd1, 0x07,
            0xc9, 0x00, 0x00,
        ];
        let data = b"<w:t>fixed Huffman stream</w:t>";
        let archive = build(&[Spec {
            name: "word/document.xml",
            method: 8,
            data,
            compressed: COMPRESSED,
        }]);
        let archive = Archive::new(&archive).expect("valid archive");
        assert_eq!(
            archive.read(b"word/document.xml").expect("read"),
            Some(data.to_vec())
        );
    }

    #[test]
    fn a_leading_slash_is_accepted() {
        let data = b"part";
        let archive = build(&[Spec {
            name: "/word/document.xml",
            method: 0,
            data,
            compressed: data,
        }]);
        let archive = Archive::new(&archive).expect("valid archive");
        assert_eq!(
            archive.read(b"word/document.xml").expect("read"),
            Some(data.to_vec())
        );
    }

    #[test]
    fn garbage_is_not_an_archive() {
        assert!(Archive::new(b"").is_err());
        assert!(Archive::new(b"this is not a zip file at all").is_err());
        assert!(Archive::new(&[0x50, 0x4b, 0x05, 0x06]).is_err());
    }

    #[test]
    fn a_broken_checksum_is_refused() {
        let data = b"payload";
        let mut archive = build(&[Spec {
            name: "word/document.xml",
            method: 0,
            data,
            compressed: data,
        }]);
        // Corrupt the payload but leave the directory checksum intact.
        let position = archive
            .windows(data.len())
            .position(|window| window == data)
            .expect("payload");
        archive[position] ^= 0xff;
        let archive = Archive::new(&archive).expect("valid archive");
        assert!(archive.read(b"word/document.xml").is_err());
    }
}
