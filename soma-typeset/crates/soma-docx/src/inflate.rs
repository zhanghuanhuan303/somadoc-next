//! Raw deflate (RFC 1951) decoder.
//!
//! `.docx` entries use method 8, which is a *raw* deflate stream: no zlib
//! header and no checksum (the zip directory carries the CRC instead). The
//! decoder covers stored, fixed-Huffman, and dynamic-Huffman blocks, including
//! overlapping LZ77 matches.
//!
//! Codes are decoded one bit at a time with the canonical first-code trick.
//! `document.xml` is decoded once per import, so the constant factor of a table
//! lookup is irrelevant next to the clarity of a loop that matches the
//! specification. Every read is bounds checked; a truncated or over-subscribed
//! stream returns `Err`, never panics.

use alloc::vec;
use alloc::vec::Vec;

use crate::DocxError;

/// Longest Huffman code deflate allows.
const MAX_BITS: usize = 15;

/// Decompress one raw deflate stream.
///
/// `limit` bounds the output. Zip entries declare an uncompressed size, so the
/// caller can turn a decompression bomb into an error instead of allocating
/// until the process dies.
pub(crate) fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, DocxError> {
    let mut bits = Bits {
        data,
        position: 0,
        buffer: 0,
        count: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = bits.bit()? == 1;
        match bits.take(2)? {
            0 => stored(&mut bits, &mut out, limit)?,
            1 => {
                let (litlen, distance) = fixed();
                codes(&mut bits, &litlen, &distance, &mut out, limit)?;
            }
            2 => {
                let (litlen, distance) = dynamic(&mut bits)?;
                codes(&mut bits, &litlen, &distance, &mut out, limit)?;
            }
            _ => return Err(corrupt()),
        }
        if last {
            return Ok(out);
        }
    }
}

fn corrupt() -> DocxError {
    DocxError::new("deflate stream is corrupt")
}

/// A stored block is a length-prefixed byte run on the next byte boundary.
fn stored(bits: &mut Bits<'_>, out: &mut Vec<u8>, limit: usize) -> Result<(), DocxError> {
    bits.align();
    let length = bits.take(16)? as usize;
    let complement = bits.take(16)? as usize;
    if length != (!complement & 0xffff) {
        return Err(corrupt());
    }
    let end = bits.position.checked_add(length).ok_or_else(corrupt)?;
    let payload = bits.data.get(bits.position..end).ok_or_else(corrupt)?;
    if out
        .len()
        .checked_add(length)
        .is_none_or(|total| total > limit)
    {
        return Err(corrupt());
    }
    out.extend_from_slice(payload);
    bits.position = end;
    Ok(())
}

/// One Huffman alphabet: how many codes sit at each length, and the symbols in
/// canonical order.
struct Table {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl Table {
    fn new(lengths: &[u8]) -> Result<Self, DocxError> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &length in lengths {
            if length as usize > MAX_BITS {
                return Err(corrupt());
            }
            counts[length as usize] += 1;
        }
        // Length 0 means "no code"; it is not part of the canonical alphabet.
        counts[0] = 0;

        // A complete prefix code leaves exactly one unused slot per level. More
        // codes than slots is over-subscription and cannot be decoded.
        let mut left = 1i32;
        for &count in counts.iter().skip(1) {
            left = (left << 1) - i32::from(count);
            if left < 0 {
                return Err(corrupt());
            }
        }

        let mut offsets = [0u16; MAX_BITS + 1];
        for length in 1..MAX_BITS {
            offsets[length + 1] = offsets[length] + counts[length];
        }

        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length == 0 {
                continue;
            }
            let slot = offsets[length as usize] as usize;
            symbols[slot] = symbol as u16;
            offsets[length as usize] += 1;
        }
        symbols.truncate(offsets[MAX_BITS] as usize);
        Ok(Self { counts, symbols })
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, DocxError> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for length in 1..=MAX_BITS {
            code |= bits.bit()? as i32;
            let count = i32::from(self.counts[length]);
            let offset = code - first;
            if offset < count {
                let at = (index + offset) as usize;
                return self.symbols.get(at).copied().ok_or_else(corrupt);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(corrupt())
    }
}

/// The fixed literal/length and distance alphabets of RFC 1951 §3.2.6.
fn fixed() -> (Table, Table) {
    let mut litlen = [0u8; 288];
    for (symbol, slot) in litlen.iter_mut().enumerate() {
        *slot = match symbol {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let distance = [5u8; 32];
    // Both tables are constants from the specification, so construction cannot
    // fail; a panic here would be a programming error, not malformed input.
    (
        Table::new(&litlen).expect("the fixed literal table is valid"),
        Table::new(&distance).expect("the fixed distance table is valid"),
    )
}

const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Read the two alphabets a dynamic block describes.
fn dynamic(bits: &mut Bits<'_>) -> Result<(Table, Table), DocxError> {
    let hlit = bits.take(5)? as usize + 257;
    let hdist = bits.take(5)? as usize + 1;
    let hclen = bits.take(4)? as usize + 4;
    // HLIT may describe at most 286 literal/length codes. HDIST is 1..=32 by
    // construction; codes 30 and 31 are reserved and rejected only if used.
    if hlit > 286 {
        return Err(corrupt());
    }

    let mut code_lengths = [0u8; 19];
    for &symbol in CODE_LENGTH_ORDER.iter().take(hclen) {
        code_lengths[symbol] = bits.take(3)? as u8;
    }
    let code_length = Table::new(&code_lengths)?;

    let total = hlit + hdist;
    let mut lengths = vec![0u8; total];
    let mut position = 0usize;
    while position < total {
        match code_length.decode(bits)? {
            symbol @ 0..=15 => {
                lengths[position] = symbol as u8;
                position += 1;
            }
            16 => {
                if position == 0 {
                    return Err(corrupt());
                }
                let previous = lengths[position - 1];
                let repeat = 3 + bits.take(2)? as usize;
                if position + repeat > total {
                    return Err(corrupt());
                }
                for _ in 0..repeat {
                    lengths[position] = previous;
                    position += 1;
                }
            }
            symbol @ (17 | 18) => {
                let repeat = if symbol == 17 {
                    3 + bits.take(3)? as usize
                } else {
                    11 + bits.take(7)? as usize
                };
                if position + repeat > total {
                    return Err(corrupt());
                }
                position += repeat;
            }
            _ => return Err(corrupt()),
        }
    }

    Ok((Table::new(&lengths[..hlit])?, Table::new(&lengths[hlit..])?))
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Decode one Huffman-coded block. Overlapping matches are copied one byte at
/// a time on purpose: `back` may be smaller than `length`, and each byte of the
/// run refers to the output already produced by that same run.
fn codes(
    bits: &mut Bits<'_>,
    litlen: &Table,
    distance: &Table,
    out: &mut Vec<u8>,
    limit: usize,
) -> Result<(), DocxError> {
    loop {
        let symbol = litlen.decode(bits)?;
        match symbol {
            0..=255 => {
                if out.len() >= limit {
                    return Err(corrupt());
                }
                out.push(symbol as u8);
            }
            256 => return Ok(()),
            _ => {
                let index = usize::from(symbol) - 257;
                let length_base = *LENGTH_BASE.get(index).ok_or_else(corrupt)?;
                let length =
                    usize::from(length_base) + bits.take(u32::from(LENGTH_EXTRA[index]))? as usize;

                let code = distance.decode(bits)? as usize;
                let distance_base = *DISTANCE_BASE.get(code).ok_or_else(corrupt)?;
                let back = usize::from(distance_base)
                    + bits.take(u32::from(DISTANCE_EXTRA[code]))? as usize;
                if back == 0 || back > out.len() {
                    return Err(corrupt());
                }
                if out
                    .len()
                    .checked_add(length)
                    .is_none_or(|total| total > limit)
                {
                    return Err(corrupt());
                }
                for _ in 0..length {
                    let byte = out[out.len() - back];
                    out.push(byte);
                }
            }
        }
    }
}

/// Least-significant-bit-first reader, as deflate's bit packing requires.
struct Bits<'a> {
    data: &'a [u8],
    /// Byte index; stored blocks also use it directly after alignment.
    position: usize,
    buffer: u32,
    count: u32,
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<u32, DocxError> {
        if self.count == 0 {
            let byte = *self.data.get(self.position).ok_or_else(corrupt)?;
            self.position += 1;
            self.buffer = u32::from(byte);
            self.count = 8;
        }
        let value = self.buffer & 1;
        self.buffer >>= 1;
        self.count -= 1;
        Ok(value)
    }

    fn take(&mut self, count: u32) -> Result<u32, DocxError> {
        let mut value = 0u32;
        for shift in 0..count {
            value |= self.bit()? << shift;
        }
        Ok(value)
    }

    /// Stored blocks begin on the next byte boundary; the remaining bits of the
    /// current byte are padding and must be discarded.
    fn align(&mut self) {
        self.buffer = 0;
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STORED: &[u8] = b"stored bytes, no compression at all";

    const FIXED_INPUT: &str = "<w:t>fixed Huffman stream</w:t>";

    /// Raw deflate, one fixed-Huffman block, produced from `FIXED_INPUT` with
    /// zlib's `Z_FIXED` strategy. Embedded so the test pins RFC 1951 bit order
    /// rather than only round-tripping our own output.
    const FIXED_STREAM: &[u8] = &[
        0xb3, 0x29, 0xb7, 0x2a, 0xb1, 0x4b, 0xcb, 0xac, 0x48, 0x4d, 0x51, 0xf0, 0x28, 0x4d, 0x4b,
        0xcb, 0x4d, 0xcc, 0x53, 0x28, 0x2e, 0x29, 0x4a, 0x4d, 0xcc, 0xb5, 0xd1, 0x07, 0xc9, 0x00,
        0x00,
    ];

    const DYNAMIC_INPUT: &str = concat!(
        "<?xml version=\"1.0\"?><w:document><w:body>",
        "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>",
        "<w:r><w:t>Deflated</w:t></w:r></w:p>",
        "<w:p><w:r><w:rPr><w:b/></w:rPr>",
        "<w:t>bold text survives a dynamic Huffman block</w:t></w:r></w:p>",
        "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>",
        "<w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
        "</w:body></w:document>",
    );

    /// Raw deflate, one dynamic-Huffman block, produced from `DYNAMIC_INPUT`
    /// with zlib level 9. The first byte is `0x75`: BFINAL=1, BTYPE=2.
    const DYNAMIC_STREAM: &[u8] = &[
        0x75, 0x90, 0x4d, 0x0e, 0x82, 0x30, 0x10, 0x85, 0xaf, 0xd2, 0xf4, 0x00, 0x56, 0xb6, 0x06,
        0x4a, 0x34, 0x2e, 0x58, 0x9a, 0x78, 0x82, 0xfe, 0x61, 0x1a, 0xfb, 0x63, 0xca, 0x50, 0xe0,
        0xf6, 0x86, 0x16, 0x88, 0x46, 0xdc, 0xcc, 0x9b, 0xc9, 0x7c, 0xf3, 0x5e, 0x32, 0x65, 0x3d,
        0x5a, 0x83, 0xa2, 0x0a, 0x9d, 0xf6, 0xae, 0xc2, 0xc5, 0xe1, 0x88, 0x6b, 0x5a, 0x0e, 0x27,
        0xe9, 0x45, 0x6f, 0x95, 0x83, 0xb9, 0xe7, 0x5e, 0x4e, 0xb3, 0xbe, 0x52, 0xb9, 0x85, 0x24,
        0x77, 0x98, 0x8c, 0x42, 0xc3, 0x29, 0x32, 0x53, 0xe1, 0x46, 0x31, 0xa9, 0xdd, 0xa3, 0xc0,
        0x84, 0x96, 0x64, 0x63, 0x52, 0x01, 0x7a, 0x55, 0xad, 0x61, 0xa0, 0xe4, 0xbc, 0x81, 0xb4,
        0x0f, 0x99, 0xda, 0x3c, 0x33, 0x9d, 0x8f, 0x78, 0xb6, 0x58, 0x26, 0xa0, 0xdc, 0x1b, 0x89,
        0x40, 0x8d, 0x80, 0xba, 0x3e, 0x44, 0x1d, 0x55, 0x87, 0x18, 0x92, 0x93, 0x63, 0x56, 0x0b,
        0xd4, 0xf4, 0x6d, 0x6b, 0x99, 0x43, 0xdc, 0x78, 0xf1, 0xdc, 0xf5, 0x07, 0x6e, 0x92, 0x64,
        0x37, 0xf1, 0x1d, 0x09, 0xf4, 0xbc, 0x73, 0x44, 0x56, 0xf0, 0x17, 0xbf, 0xfc, 0xc7, 0x49,
        0x0e, 0x21, 0x4b, 0x24, 0x59, 0xdf, 0x46, 0x3e, 0x7e, 0xf9, 0x06,
    ];

    /// Raw deflate, one fixed-Huffman block for 30 `a`s. The encoder emits one
    /// literal and then an overlapping match (distance 1, length 29), which is
    /// the case a naive forward copy gets wrong.
    const OVERLAP_INPUT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OVERLAP_STREAM: &[u8] = &[0x4b, 0x4c, 0xc4, 0x07, 0x00];

    fn stored_block(data: &[u8], last: bool) -> Vec<u8> {
        let length = data.len() as u16;
        let mut block = vec![if last { 0x01 } else { 0x00 }];
        block.extend_from_slice(&length.to_le_bytes());
        block.extend_from_slice(&(!length).to_le_bytes());
        block.extend_from_slice(data);
        block
    }

    fn stored_stream(data: &[u8]) -> Vec<u8> {
        stored_block(data, true)
    }

    #[test]
    fn a_stored_block_decodes() {
        let decoded = inflate(&stored_stream(STORED), 1024).expect("stored block");
        assert_eq!(decoded.as_slice(), STORED);
    }

    #[test]
    fn a_fixed_huffman_stream_decodes() {
        assert_eq!((FIXED_STREAM[0] >> 1) & 3, 1, "BTYPE must be fixed");
        let decoded = inflate(FIXED_STREAM, 1024).expect("fixed block");
        assert_eq!(decoded.as_slice(), FIXED_INPUT.as_bytes());
    }

    #[test]
    fn a_dynamic_huffman_stream_decodes() {
        assert_eq!((DYNAMIC_STREAM[0] >> 1) & 3, 2, "BTYPE must be dynamic");
        let decoded = inflate(DYNAMIC_STREAM, 4096).expect("dynamic block");
        assert_eq!(decoded.as_slice(), DYNAMIC_INPUT.as_bytes());
    }

    #[test]
    fn an_overlapping_lz77_match_is_copied() {
        let decoded = inflate(OVERLAP_STREAM, 1024).expect("overlap block");
        assert_eq!(decoded.as_slice(), OVERLAP_INPUT.as_bytes());
    }

    #[test]
    fn multiple_blocks_are_concatenated() {
        let mut stream = stored_block(b"first, ", false);
        stream.extend_from_slice(&stored_block(b"second", true));
        let decoded = inflate(&stream, 1024).expect("two blocks");
        assert_eq!(decoded.as_slice(), b"first, second");
    }

    #[test]
    fn a_truncated_stream_is_an_error() {
        assert!(inflate(&[], 1024).is_err());
        assert!(inflate(&FIXED_STREAM[..5], 1024).is_err());
        assert!(inflate(&FIXED_STREAM[..FIXED_STREAM.len() - 1], 1024).is_err());
    }

    #[test]
    fn a_reserved_block_type_is_an_error() {
        // BFINAL=1, BTYPE=3.
        assert!(inflate(&[0x07], 1024).is_err());
    }

    #[test]
    fn the_output_limit_is_enforced() {
        assert!(inflate(&stored_stream(STORED), 4).is_err());
        assert!(inflate(FIXED_STREAM, 4).is_err());
    }
}
