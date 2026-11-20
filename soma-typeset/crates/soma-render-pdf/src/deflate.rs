//! A deterministic zlib stream for PDF `/FlateDecode`.
//!
//! The writer emits one fixed-Huffman deflate block with greedy LZ77 matches:
//! the table is in the specification, so the output depends only on the input
//! bytes. No floating point, no hash-map iteration order, no timestamps.
//!
//! Determinism is the reason this is hand-written rather than pulled in: the
//! backend promises byte-identical PDFs for a given layout and font, and a
//! compressor that changes its mind would break `canonicalHash`.

use alloc::vec;
use alloc::vec::Vec;

/// Deflate's window: a match never reaches further back than this.
const WINDOW: usize = 32_768;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
/// Hash table size for the 3-byte prefix. 2^15 keeps the table small enough
/// for a `no_std` build and long enough that collisions rarely matter.
const HASH_BITS: u32 = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
/// How many candidates a match may look at. Bounded so the encoder's cost is
/// linear in the input rather than quadratic.
const MAX_CHAIN: usize = 48;
/// Below this, storing bytes costs more than leaving the stream alone.
const MIN_GAIN: usize = 16;

// Length codes 257..=285: the smallest length each code stands for, and how
// many extra bits follow it.
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
// Distance codes 0..=29, same idea. A 32 KiB window never needs code 30/31.
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Compress `data` into a zlib stream, or return `None` when the result would
/// not be smaller.
pub(crate) fn zlib(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < MIN_GAIN * 2 {
        return None;
    }
    let mut bits = Bits::default();
    // BFINAL = 1, BTYPE = 01 (fixed Huffman), both least significant bit first.
    bits.write(1, 1);
    bits.write(1, 2);

    let mut head = vec![usize::MAX; HASH_SIZE];
    let mut previous = vec![usize::MAX; WINDOW];
    let mut position = 0usize;
    while position < data.len() {
        let (length, distance) = match_at(data, position, &head, &previous);
        if length >= MIN_MATCH {
            emit_length(&mut bits, length);
            emit_distance(&mut bits, distance);
            for offset in 0..length {
                insert(data, position + offset, &mut head, &mut previous);
            }
            position += length;
        } else {
            emit_symbol(&mut bits, data[position] as u32);
            insert(data, position, &mut head, &mut previous);
            position += 1;
        }
    }
    // End of block.
    emit_symbol(&mut bits, 256);
    let body = bits.finish();

    let mut out = Vec::with_capacity(body.len() + 6);
    // CM = 8 (deflate), CINFO = 7 (32 KiB window); FCHECK so the pair is a
    // multiple of 31; no preset dictionary.
    out.push(0x78);
    out.push(0x01);
    out.extend_from_slice(&body);
    out.extend_from_slice(&adler32(data).to_be_bytes());
    if out.len() + MIN_GAIN < data.len() {
        Some(out)
    } else {
        None
    }
}

/// Length and distance of the longest match for `position`, or `(0, 0)`.
fn match_at(data: &[u8], position: usize, head: &[usize], previous: &[usize]) -> (usize, usize) {
    let available = data.len() - position;
    if available < MIN_MATCH {
        return (0, 0);
    }
    let hash = hash3(data, position);
    let mut candidate = head[hash];
    let mut best_length = 0usize;
    let mut best_distance = 0usize;
    let mut visited = 0usize;
    let limit = available.min(MAX_MATCH);
    while candidate != usize::MAX && visited < MAX_CHAIN {
        let distance = position - candidate;
        if distance > WINDOW {
            break;
        }
        // The previous four bytes already matched to get here, so a quick
        // reject on the byte before the match keeps the chain cheap.
        let mut length = 0usize;
        while length < limit && data[candidate + length] == data[position + length] {
            length += 1;
        }
        if length > best_length {
            best_length = length;
            best_distance = distance;
            if length == limit {
                break;
            }
        }
        candidate = previous[candidate % WINDOW];
        visited += 1;
    }
    if best_length >= MIN_MATCH {
        (best_length, best_distance)
    } else {
        (0, 0)
    }
}

fn hash3(data: &[u8], at: usize) -> usize {
    let value = ((data[at] as u32) << 16) | ((data[at + 1] as u32) << 8) | data[at + 2] as u32;
    (value.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

fn insert(data: &[u8], at: usize, head: &mut [usize], previous: &mut [usize]) {
    if at + MIN_MATCH > data.len() {
        return;
    }
    let hash = hash3(data, at);
    previous[at % WINDOW] = head[hash];
    head[hash] = at;
}

fn emit_symbol(bits: &mut Bits, symbol: u32) {
    // RFC 1951 §3.2.6: the fixed literal/length code.
    match symbol {
        0..=143 => bits.code(0x30 + symbol, 8),
        144..=255 => bits.code(0x190 + (symbol - 144), 9),
        256..=279 => bits.code(symbol - 256, 7),
        280..=287 => bits.code(0xC0 + (symbol - 280), 8),
        _ => unreachable!("literal/length symbol {symbol} is outside the fixed table"),
    }
}

fn emit_length(bits: &mut Bits, length: usize) {
    let mut index = LENGTH_BASE.len() - 1;
    while LENGTH_BASE[index] as usize > length {
        index -= 1;
    }
    emit_symbol(bits, 257 + index as u32);
    let extra = LENGTH_EXTRA[index] as u32;
    if extra > 0 {
        bits.write((length - LENGTH_BASE[index] as usize) as u32, extra);
    }
}

fn emit_distance(bits: &mut Bits, distance: usize) {
    let mut index = DISTANCE_BASE.len() - 1;
    while DISTANCE_BASE[index] as usize > distance {
        index -= 1;
    }
    bits.code(index as u32, 5);
    let extra = DISTANCE_EXTRA[index] as u32;
    if extra > 0 {
        bits.write((distance - DISTANCE_BASE[index] as usize) as u32, extra);
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

/// Least-significant-bit-first bit packing, as deflate's blocks need.
#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    accumulator: u64,
    count: u32,
}

impl Bits {
    fn write(&mut self, value: u32, count: u32) {
        if count == 0 {
            return;
        }
        self.accumulator |= u64::from(value & ((1 << count) - 1)) << self.count;
        self.count += count;
        while self.count >= 8 {
            self.out.push(self.accumulator as u8);
            self.accumulator >>= 8;
            self.count -= 8;
        }
    }

    /// Huffman codes are transmitted most significant bit first, unlike plain
    /// integers.
    fn code(&mut self, value: u32, count: u32) {
        for index in (0..count).rev() {
            self.write((value >> index) & 1, 1);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push(self.accumulator as u8);
        }
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A decoder for the one block type this encoder emits. It is deliberately
    /// independent of the encoder's bookkeeping: a round trip proves the codes,
    /// the bit order and the match lengths agree with RFC 1951.
    fn inflate(stream: &[u8]) -> Vec<u8> {
        assert_eq!(stream[0], 0x78, "zlib CMF");
        assert_eq!((u16::from(stream[0]) * 256 + u16::from(stream[1])) % 31, 0);
        let body = &stream[2..stream.len() - 4];
        let mut reader = Reader::new(body);
        assert_eq!(reader.bits(1), 1, "BFINAL");
        assert_eq!(reader.bits(2), 1, "BTYPE must be fixed Huffman");
        let mut out = Vec::new();
        loop {
            let symbol = reader.symbol();
            match symbol {
                0..=255 => out.push(symbol as u8),
                256 => break,
                _ => {
                    let length = reader.length(symbol);
                    let distance = reader.distance();
                    assert!(distance <= out.len(), "match reaches before the start");
                    for _ in 0..length {
                        let byte = out[out.len() - distance];
                        out.push(byte);
                    }
                }
            }
        }
        let expected = u32::from_be_bytes([
            stream[stream.len() - 4],
            stream[stream.len() - 3],
            stream[stream.len() - 2],
            stream[stream.len() - 1],
        ]);
        assert_eq!(adler32(&out), expected, "adler32");
        out
    }

    struct Reader<'a> {
        data: &'a [u8],
        position: usize,
        accumulator: u32,
        count: u32,
    }

    impl<'a> Reader<'a> {
        fn new(data: &'a [u8]) -> Self {
            Self {
                data,
                position: 0,
                accumulator: 0,
                count: 0,
            }
        }

        fn bit(&mut self) -> u32 {
            if self.count == 0 {
                self.accumulator = u32::from(self.data[self.position]);
                self.position += 1;
                self.count = 8;
            }
            let value = self.accumulator & 1;
            self.accumulator >>= 1;
            self.count -= 1;
            value
        }

        fn code(&mut self, count: u32) -> u32 {
            let mut value = 0;
            for _ in 0..count {
                value = (value << 1) | self.bit();
            }
            value
        }

        /// Plain integers: least significant bit first.
        fn bits(&mut self, count: u32) -> u32 {
            let mut value = 0;
            for index in 0..count {
                value |= self.bit() << index;
            }
            value
        }

        fn symbol(&mut self) -> u32 {
            let mut code = 0u32;
            for length in 1..=9u32 {
                code = (code << 1) | self.bit();
                match length {
                    7 if code <= 0x17 => return 256 + code,
                    8 if (0x30..=0xBF).contains(&code) => return code - 0x30,
                    8 if (0xC0..=0xC7).contains(&code) => return 280 + code - 0xC0,
                    9 if (0x190..=0x1FF).contains(&code) => return 144 + code - 0x190,
                    _ => {}
                }
            }
            panic!("no symbol matches code {code:#x}");
        }

        fn length(&mut self, symbol: u32) -> usize {
            let index = (symbol - 257) as usize;
            LENGTH_BASE[index] as usize + self.bits(LENGTH_EXTRA[index] as u32) as usize
        }

        fn distance(&mut self) -> usize {
            // Distance codes are Huffman codes: most significant bit first.
            let code = self.code(5) as usize;
            DISTANCE_BASE[code] as usize + self.bits(DISTANCE_EXTRA[code] as u32) as usize
        }
    }

    fn round_trip(data: &[u8]) {
        let packed = zlib(data).expect("compresses");
        assert!(packed.len() < data.len());
        assert_eq!(inflate(&packed), data);
    }

    #[test]
    fn a_repetitive_stream_round_trips() {
        round_trip(&vec![b'a'; 5000]);
    }

    #[test]
    fn text_with_distance_round_trips() {
        let mut data = Vec::new();
        for line in 0..400 {
            data.extend_from_slice(b"the same words appear at a distance, ");
            data.extend_from_slice(line.to_string().as_bytes());
            data.push(b'\n');
        }
        round_trip(&data);
    }

    #[test]
    fn every_byte_value_round_trips() {
        let mut data = Vec::new();
        for round in 0..8 {
            for byte in 0..=255u8 {
                data.push(byte ^ round as u8);
            }
        }
        round_trip(&data);
    }

    #[test]
    fn short_inputs_are_left_alone() {
        assert!(zlib(b"x").is_none());
        assert!(zlib(b"short").is_none());
    }

    #[test]
    fn output_is_deterministic() {
        let data = b"deterministic output is the whole point, deterministic output".repeat(20);
        assert_eq!(zlib(&data), zlib(&data));
    }
}
