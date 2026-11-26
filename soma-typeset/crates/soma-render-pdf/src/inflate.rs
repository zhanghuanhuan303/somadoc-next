//! A deflate / zlib decompressor, used to read PNG image data.
//!
//! The encoder in `deflate.rs` writes one block per stream; a PNG reader has to
//! accept everything a tool like `zopfli` or `libpng` produces, so this decoder
//! covers all three block types of RFC 1951 and both Huffman alphabets.
//!
//! There is no table-driven fast path: symbols are decoded one bit at a time
//! with the canonical first-code trick. A figure is decoded once per render, so
//! the constant factor does not matter, and the simple loop is easy to check
//! against the specification.

use alloc::vec;
use alloc::vec::Vec;

use crate::font::PdfError;

/// Longest Huffman code deflate allows.
const MAX_BITS: usize = 15;

pub(crate) fn zlib(data: &[u8]) -> Result<Vec<u8>, PdfError> {
    if data.len() < 6 {
        return Err(bad("zlib stream is truncated"));
    }
    let (cmf, flg) = (data[0], data[1]);
    if cmf & 0x0f != 8 {
        return Err(bad("zlib stream is not deflate"));
    }
    if (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 {
        return Err(bad("zlib header check bits are wrong"));
    }
    if flg & 0x20 != 0 {
        return Err(bad("zlib preset dictionaries are unsupported"));
    }
    let body = &data[2..data.len() - 4];
    let out = inflate(body)?;
    let expected = u32::from_be_bytes([
        data[data.len() - 4],
        data[data.len() - 3],
        data[data.len() - 2],
        data[data.len() - 1],
    ]);
    if adler32(&out) != expected {
        return Err(bad("zlib adler32 does not match the payload"));
    }
    Ok(out)
}

pub(crate) fn inflate(data: &[u8]) -> Result<Vec<u8>, PdfError> {
    let mut bits = Bits {
        data,
        position: 0,
        buffer: 0,
        count: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = bits.bits(1)? == 1;
        match bits.bits(2)? {
            0 => stored(&mut bits, &mut out)?,
            1 => {
                let (litlen, distance) = fixed();
                codes(&mut bits, &litlen, &distance, &mut out)?;
            }
            2 => {
                let (litlen, distance) = dynamic(&mut bits)?;
                codes(&mut bits, &litlen, &distance, &mut out)?;
            }
            other => return Err(bad(alloc::format!("deflate block type {other}"))),
        }
        if last {
            return Ok(out);
        }
    }
}

fn stored(bits: &mut Bits<'_>, out: &mut Vec<u8>) -> Result<(), PdfError> {
    bits.align();
    let length = bits.bits(16)? as usize;
    let complement = bits.bits(16)? as usize;
    if length != (!complement & 0xffff) {
        return Err(bad("stored block length check failed"));
    }
    let start = bits.position;
    let bytes = bits
        .data
        .get(start..start + length)
        .ok_or_else(|| bad("stored block runs past the stream"))?;
    out.extend_from_slice(bytes);
    bits.position += length;
    Ok(())
}

/// One Huffman alphabet: how many symbols sit at each length, and which.
struct Table {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl Table {
    fn new(lengths: &[u8]) -> Result<Self, PdfError> {
        let mut counts = [0u16; MAX_BITS + 1];
        for length in lengths {
            if *length > MAX_BITS as u8 {
                return Err(bad("Huffman code is longer than 15 bits"));
            }
            counts[*length as usize] += 1;
        }
        // Symbol 0 has length 0 by definition and is not a code.
        counts[0] = 0;
        let mut left = 1i32;
        for count in counts.iter().skip(1) {
            left <<= 1;
            left -= i32::from(*count);
            if left < 0 {
                return Err(bad("Huffman code is over-subscribed"));
            }
        }
        let mut offsets = [0u16; MAX_BITS + 1];
        for length in 1..MAX_BITS {
            offsets[length + 1] = offsets[length] + counts[length];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, length) in lengths.iter().enumerate() {
            if *length == 0 {
                continue;
            }
            let slot = offsets[*length as usize] as usize;
            symbols[slot] = symbol as u16;
            offsets[*length as usize] += 1;
        }
        symbols.truncate(offsets[MAX_BITS] as usize);
        Ok(Self { counts, symbols })
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, PdfError> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for length in 1..=MAX_BITS {
            code |= bits.bits(1)? as i32;
            let count = i32::from(self.counts[length]);
            if code - first < count {
                let at = (index + (code - first)) as usize;
                return self
                    .symbols
                    .get(at)
                    .copied()
                    .ok_or_else(|| bad("Huffman symbol is out of range"));
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(bad("Huffman code does not decode"))
    }
}

/// The literal/length and distance tables of a fixed-Huffman block.
fn fixed() -> (Table, Table) {
    let mut litlen = [0u8; 288];
    for (symbol, length) in litlen.iter_mut().enumerate() {
        *length = match symbol {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let distance = [5u8; 32];
    (
        Table::new(&litlen).expect("the fixed table is valid"),
        Table::new(&distance).expect("the fixed table is valid"),
    )
}

const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn dynamic(bits: &mut Bits<'_>) -> Result<(Table, Table), PdfError> {
    let hlit = bits.bits(5)? as usize + 257;
    let hdist = bits.bits(5)? as usize + 1;
    let hclen = bits.bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return Err(bad("deflate block declares too many codes"));
    }
    let mut code_lengths = [0u8; 19];
    for &symbol in CODE_LENGTH_ORDER.iter().take(hclen) {
        code_lengths[symbol] = bits.bits(3)? as u8;
    }
    let code_length = Table::new(&code_lengths)?;
    let mut lengths = vec![0u8; hlit + hdist];
    let mut position = 0;
    while position < lengths.len() {
        let symbol = code_length.decode(bits)?;
        match symbol {
            0..=15 => {
                lengths[position] = symbol as u8;
                position += 1;
            }
            16 => {
                let previous = *lengths
                    .get(position.wrapping_sub(1))
                    .ok_or_else(|| bad("deflate repeat has no previous length"))?;
                let repeat = 3 + bits.bits(2)? as usize;
                if position + repeat > lengths.len() {
                    return Err(bad("deflate repeat runs past the table"));
                }
                for _ in 0..repeat {
                    lengths[position] = previous;
                    position += 1;
                }
            }
            17 | 18 => {
                let repeat = if symbol == 17 {
                    3 + bits.bits(3)? as usize
                } else {
                    11 + bits.bits(7)? as usize
                };
                if position + repeat > lengths.len() {
                    return Err(bad("deflate zero run runs past the table"));
                }
                position += repeat;
            }
            other => return Err(bad(alloc::format!("code-length symbol {other}"))),
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

fn codes(
    bits: &mut Bits<'_>,
    litlen: &Table,
    distance: &Table,
    out: &mut Vec<u8>,
) -> Result<(), PdfError> {
    loop {
        let symbol = litlen.decode(bits)?;
        match symbol {
            0..=255 => out.push(symbol as u8),
            256 => return Ok(()),
            _ => {
                let index = usize::from(symbol) - 257;
                if index >= LENGTH_BASE.len() {
                    return Err(bad("deflate length symbol is unused"));
                }
                let length = usize::from(LENGTH_BASE[index])
                    + bits.bits(u32::from(LENGTH_EXTRA[index]))? as usize;
                let code = usize::from(distance.decode(bits)?);
                if code >= DISTANCE_BASE.len() {
                    return Err(bad("deflate distance symbol is unused"));
                }
                let back = usize::from(DISTANCE_BASE[code])
                    + bits.bits(u32::from(DISTANCE_EXTRA[code]))? as usize;
                if back == 0 || back > out.len() {
                    return Err(bad("deflate match reaches before the start"));
                }
                for _ in 0..length {
                    let byte = out[out.len() - back];
                    out.push(byte);
                }
            }
        }
    }
}

fn adler32(data: &[u8]) -> u32 {
    let (mut low, mut high) = (1u32, 0u32);
    for byte in data {
        low = (low + u32::from(*byte)) % 65521;
        high = (high + low) % 65521;
    }
    (high << 16) | low
}

fn bad(reason: impl Into<alloc::string::String>) -> PdfError {
    PdfError::new(alloc::format!("image data: {}", reason.into()))
}

struct Bits<'a> {
    data: &'a [u8],
    /// Byte index; also used by stored blocks, which read whole bytes.
    position: usize,
    buffer: u32,
    count: u32,
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<u32, PdfError> {
        if self.count == 0 {
            let byte = *self
                .data
                .get(self.position)
                .ok_or_else(|| bad("deflate stream is truncated"))?;
            self.position += 1;
            self.buffer = u32::from(byte);
            self.count = 8;
        }
        let value = self.buffer & 1;
        self.buffer >>= 1;
        self.count -= 1;
        Ok(value)
    }

    fn bits(&mut self, count: u32) -> Result<u32, PdfError> {
        let mut value = 0;
        for index in 0..count {
            value |= self.bit()? << index;
        }
        Ok(value)
    }

    /// Stored blocks start on a byte boundary.
    fn align(&mut self) {
        self.buffer = 0;
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_block_type_round_trips_through_the_encoder() {
        // The encoder in `deflate.rs` picks fixed or dynamic blocks; both must
        // come back byte for byte.
        let mut data = Vec::new();
        for line in 0..500 {
            data.extend_from_slice(b"a repetitive line with a few numbers ");
            data.extend_from_slice(line.to_string().as_bytes());
            data.push(b'\n');
        }
        let packed = crate::deflate::zlib(&data).expect("compresses");
        assert_eq!(zlib(&packed).expect("inflates"), data);
    }

    #[test]
    fn a_skewed_alphabet_round_trips() {
        let mut data = Vec::new();
        for index in 0..20_000 {
            data.push(if index % 5 == 0 {
                0x00
            } else {
                (index % 251) as u8
            });
        }
        let packed = crate::deflate::zlib(&data).expect("compresses");
        assert_eq!(zlib(&packed).expect("inflates"), data);
    }

    #[test]
    fn a_stored_block_decodes() {
        // Build a zlib stream with one stored block by hand.
        let payload = b"stored bytes, no compression at all";
        let body = &payload[..payload.len().min(30)];
        let mut stream = alloc::vec![0x78, 0x01, 0x01];
        stream.extend_from_slice(&(body.len() as u16).to_le_bytes());
        stream.extend_from_slice(&(!(body.len() as u16)).to_le_bytes());
        stream.extend_from_slice(body);
        stream.extend_from_slice(&adler32(body).to_be_bytes());
        assert_eq!(zlib(&stream).expect("inflates"), body);
    }

    #[test]
    fn a_corrupt_adler_is_refused() {
        let data = b"the checksum is part of the contract".repeat(10);
        let mut packed = crate::deflate::zlib(&data).expect("compresses");
        let last = packed.len() - 1;
        packed[last] ^= 0xff;
        assert!(zlib(&packed).is_err());
    }
}
