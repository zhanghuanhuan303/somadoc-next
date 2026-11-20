//! A deterministic zlib stream for PDF `/FlateDecode`.
//!
//! The writer emits one deflate block, choosing between the fixed Huffman
//! table of RFC 1951 §3.2.6 and a dynamic table built from the block's own
//! frequencies (§3.2.7) — whichever is smaller. LZ77 matching is greedy.
//!
//! Every tie is broken by symbol order, so the output depends only on the
//! input bytes: no floating point, no hash-map iteration order, no timestamps.
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

/// The literal/length alphabet has 286 symbols, the distance alphabet 30.
const LITLEN_SYMBOLS: usize = 286;
const DISTANCE_SYMBOLS: usize = 30;
/// RFC 1951 §3.2.7 order for the code-length code lengths.
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

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
    let tokens = tokenize(data);
    // A dynamic table pays for its code lengths up front and earns them back
    // on the payload; a tiny block often does better with the fixed table. Both
    // encodings are built and the shorter one wins, so no heuristic has to be
    // trusted. Ties go to the fixed table, which has no header to get wrong.
    let fixed = encode_fixed(&tokens);
    let dynamic = encode_dynamic(&tokens);
    let body = if dynamic.len() < fixed.len() {
        dynamic
    } else {
        fixed
    };

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

// ---------------------------------------------------------------------------
// LZ77
// ---------------------------------------------------------------------------

enum Token {
    Literal(u8),
    Match { length: u16, distance: u16 },
}

fn tokenize(data: &[u8]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut head = vec![usize::MAX; HASH_SIZE];
    let mut previous = vec![usize::MAX; WINDOW];
    let mut position = 0usize;
    while position < data.len() {
        let (length, distance) = match_at(data, position, &head, &previous);
        if length >= MIN_MATCH {
            tokens.push(Token::Match {
                length: length as u16,
                distance: distance as u16,
            });
            for offset in 0..length {
                insert(data, position + offset, &mut head, &mut previous);
            }
            position += length;
        } else {
            tokens.push(Token::Literal(data[position]));
            insert(data, position, &mut head, &mut previous);
            position += 1;
        }
    }
    tokens
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

// ---------------------------------------------------------------------------
// Huffman tables
// ---------------------------------------------------------------------------

/// A canonical Huffman code: a length per symbol and the code that length
/// implies.
struct Huffman {
    lengths: Vec<u8>,
    codes: Vec<u16>,
}

impl Huffman {
    /// Build a code for `frequencies`, with no length above `limit`.
    ///
    /// Frequencies are counts, so the tree only depends on the input bytes.
    /// Leaves are merged smallest-first with the node index as the tie-break,
    /// which makes the shape reproducible without a comparator that could
    /// disagree between targets.
    fn build(frequencies: &[u32], limit: u8) -> Self {
        let depths = tree_depths(frequencies);
        let lengths = limit_depths(frequencies, &depths, limit);
        let codes = canonical(&lengths);
        Self { lengths, codes }
    }

    fn write(&self, bits: &mut Bits, symbol: usize) {
        let length = self.lengths[symbol];
        debug_assert!(length > 0, "symbol {symbol} has no code");
        bits.code(u32::from(self.codes[symbol]), u32::from(length));
    }
}

/// Depth of every symbol in the Huffman tree, zero when unused.
fn tree_depths(frequencies: &[u32]) -> Vec<u8> {
    let mut used: Vec<usize> = (0..frequencies.len())
        .filter(|symbol| frequencies[*symbol] > 0)
        .collect();
    if used.is_empty() {
        // An alphabet nobody used still needs a complete code, otherwise a PDF
        // reader rejects the block. Two 1-bit codes is the cheapest way.
        return match frequencies.len() {
            0 => Vec::new(),
            1 => vec![1],
            _ => {
                let mut depths = vec![0u8; frequencies.len()];
                depths[0] = 1;
                depths[1] = 1;
                depths
            }
        };
    }
    if used.len() == 1 {
        // One symbol cannot form a complete code on its own: give it a sibling
        // that is never emitted, exactly as zlib does.
        let sibling = if used[0] == 0 { 1 } else { 0 };
        if sibling < frequencies.len() {
            used.push(sibling);
            used.sort_unstable();
        }
    }

    let leaves = used.len();
    let mut weight = vec![0u64; leaves * 2 - 1];
    for (index, symbol) in used.iter().enumerate() {
        weight[index] = u64::from(frequencies[*symbol]).max(1);
    }
    let mut parent = vec![usize::MAX; leaves * 2 - 1];
    for next in leaves..(leaves * 2 - 1) {
        let (first, second) = two_smallest(&weight, &parent, next);
        weight[next] = weight[first] + weight[second];
        parent[first] = next;
        parent[second] = next;
    }

    let mut depths = vec![0u8; frequencies.len()];
    for (index, symbol) in used.iter().enumerate() {
        let mut node = index;
        let mut depth = 0u8;
        while parent[node] != usize::MAX {
            depth += 1;
            node = parent[node];
        }
        depths[*symbol] = depth;
    }
    depths
}

/// The two roots with the smallest weight, ties broken by lower index. Scanning
/// is fine here: an alphabet never has more than 286 symbols.
fn two_smallest(weight: &[u64], parent: &[usize], count: usize) -> (usize, usize) {
    let (mut first, mut second) = (usize::MAX, usize::MAX);
    for node in 0..count {
        if parent[node] != usize::MAX {
            continue;
        }
        let better_than_first = first == usize::MAX
            || weight[node] < weight[first]
            || (weight[node] == weight[first] && node < first);
        if better_than_first {
            second = first;
            first = node;
        } else if second == usize::MAX
            || weight[node] < weight[second]
            || (weight[node] == weight[second] && node < second)
        {
            second = node;
        }
    }
    (first, second)
}

/// Depth per symbol, with the depth-limited fallback.
///
/// A code deeper than `limit` would make an invalid block. Rather than
/// re-balancing the tree with an overflow heuristic — those are hard to prove
/// exact, and an incomplete code makes a reader refuse the file — the alphabet
/// falls back to the two-level balanced code, which always fits: `n` symbols
/// cost `floor(log2 n)` or `ceil(log2 n)` bits and the Kraft sum is exactly one.
/// Symbols are still ordered by frequency, so the common ones get the short
/// codes.
fn limit_depths(frequencies: &[u32], depths: &[u8], limit: u8) -> Vec<u8> {
    if depths.iter().all(|depth| *depth <= limit) {
        return depths.to_vec();
    }
    let mut used: Vec<usize> = (0..frequencies.len())
        .filter(|symbol| depths[*symbol] > 0)
        .collect();
    let mut lengths = vec![0u8; frequencies.len()];
    let count = used.len();
    debug_assert!(count >= 2, "tree_depths always yields two symbols");
    let bits_needed = (count as u32).next_power_of_two().trailing_zeros();
    let short = (bits_needed - 1) as u8;
    let long_count = 2 * (count - (1usize << short));
    used.sort_by_key(|symbol| (core::cmp::Reverse(frequencies[*symbol]), *symbol));
    for (index, symbol) in used.iter().enumerate() {
        lengths[*symbol] = if index < count - long_count {
            short
        } else {
            short + 1
        };
    }
    lengths
}

/// RFC 1951 §3.2.2 canonical codes: symbols ordered by length, then by symbol.
fn canonical(lengths: &[u8]) -> Vec<u16> {
    let maximum = lengths.iter().copied().max().unwrap_or(0) as usize;
    let mut counts = vec![0u16; maximum + 1];
    for length in lengths {
        if *length > 0 {
            counts[*length as usize] += 1;
        }
    }
    let mut next = vec![0u16; maximum + 1];
    let mut code = 0u16;
    for bits in 1..=maximum {
        code = (code + counts[bits - 1]) << 1;
        next[bits] = code;
    }
    let mut codes = vec![0u16; lengths.len()];
    for (symbol, length) in lengths.iter().enumerate() {
        if *length > 0 {
            codes[symbol] = next[*length as usize];
            next[*length as usize] += 1;
        }
    }
    codes
}

// ---------------------------------------------------------------------------
// Block writers
// ---------------------------------------------------------------------------

fn encode_fixed(tokens: &[Token]) -> Vec<u8> {
    let mut bits = Bits::default();
    // BFINAL = 1, BTYPE = 01 (fixed Huffman), both least significant bit first.
    bits.write(1, 1);
    bits.write(1, 2);
    write_tokens(&mut bits, tokens, &Codes::Fixed);
    bits.finish()
}

fn encode_dynamic(tokens: &[Token]) -> Vec<u8> {
    let mut litlen_counts = [0u32; LITLEN_SYMBOLS];
    let mut distance_counts = [0u32; DISTANCE_SYMBOLS];
    for token in tokens {
        match *token {
            Token::Literal(byte) => litlen_counts[u32::from(byte) as usize] += 1,
            Token::Match { length, distance } => {
                litlen_counts[257 + length_symbol(length as usize)] += 1;
                distance_counts[distance_symbol(distance as usize)] += 1;
            }
        }
    }
    litlen_counts[256] += 1; // end of block

    let litlen = Huffman::build(&litlen_counts, 15);
    let distance = Huffman::build(&distance_counts, 15);

    let hlit = last_used(&litlen.lengths).max(256) + 1;
    let hdist = last_used(&distance.lengths) + 1;
    let mut lengths = Vec::with_capacity(hlit + hdist);
    lengths.extend_from_slice(&litlen.lengths[..hlit]);
    lengths.extend_from_slice(&distance.lengths[..hdist]);

    let run_lengths = run_length_encode(&lengths);
    let mut code_counts = [0u32; 19];
    for (symbol, _) in &run_lengths {
        code_counts[*symbol as usize] += 1;
    }
    let code_length = Huffman::build(&code_counts, 7);
    let mut hclen = 19;
    while hclen > 4 && code_length.lengths[CODE_LENGTH_ORDER[hclen - 1]] == 0 {
        hclen -= 1;
    }

    let mut bits = Bits::default();
    bits.write(1, 1);
    bits.write(2, 2); // BTYPE = 10 (dynamic Huffman)
    bits.write((hlit - 257) as u32, 5);
    bits.write((hdist - 1) as u32, 5);
    bits.write((hclen - 4) as u32, 4);
    for &symbol in CODE_LENGTH_ORDER.iter().take(hclen) {
        bits.write(u32::from(code_length.lengths[symbol]), 3);
    }
    for (symbol, extra) in &run_lengths {
        code_length.write(&mut bits, *symbol as usize);
        match *symbol {
            16 => bits.write(extra - 3, 2),
            17 => bits.write(extra - 3, 3),
            18 => bits.write(extra - 11, 7),
            _ => {}
        }
    }
    write_tokens(
        &mut bits,
        tokens,
        &Codes::Dynamic {
            litlen: &litlen,
            distance: &distance,
        },
    );
    bits.finish()
}

/// Index of the last non-zero entry, or `usize::MAX` when there is none.
fn last_used(lengths: &[u8]) -> usize {
    lengths
        .iter()
        .rposition(|length| *length > 0)
        .unwrap_or(usize::MAX)
}

/// RFC 1951 §3.2.7 run-length encoding of the code lengths, as
/// `(symbol, extra)` where `extra` is the repeat count for 16/17/18 and zero
/// otherwise.
fn run_length_encode(lengths: &[u8]) -> Vec<(u8, u32)> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < lengths.len() {
        let value = lengths[index];
        let mut run = 1;
        while index + run < lengths.len() && lengths[index + run] == value {
            run += 1;
        }
        if value == 0 {
            let mut remaining = run;
            while remaining >= 11 {
                let take = remaining.min(138);
                out.push((18, take as u32));
                remaining -= take;
            }
            while remaining >= 3 {
                let take = remaining.min(10);
                out.push((17, take as u32));
                remaining -= take;
            }
            for _ in 0..remaining {
                out.push((0, 0));
            }
        } else {
            out.push((value, 0));
            let mut remaining = run - 1;
            while remaining >= 3 {
                let take = remaining.min(6);
                out.push((16, take as u32));
                remaining -= take;
            }
            for _ in 0..remaining {
                out.push((value, 0));
            }
        }
        index += run;
    }
    out
}

enum Codes<'a> {
    Fixed,
    Dynamic {
        litlen: &'a Huffman,
        distance: &'a Huffman,
    },
}

impl Codes<'_> {
    fn symbol(&self, bits: &mut Bits, symbol: u32) {
        match self {
            Self::Fixed => emit_fixed_symbol(bits, symbol),
            Self::Dynamic { litlen, .. } => litlen.write(bits, symbol as usize),
        }
    }

    fn distance(&self, bits: &mut Bits, code: usize) {
        match self {
            Self::Fixed => bits.code(code as u32, 5),
            Self::Dynamic { distance, .. } => distance.write(bits, code),
        }
    }
}

fn write_tokens(bits: &mut Bits, tokens: &[Token], codes: &Codes<'_>) {
    for token in tokens {
        match *token {
            Token::Literal(byte) => codes.symbol(bits, u32::from(byte)),
            Token::Match { length, distance } => {
                let index = length_symbol(length as usize);
                codes.symbol(bits, 257 + index as u32);
                let extra = u32::from(LENGTH_EXTRA[index]);
                if extra > 0 {
                    bits.write(
                        (length as usize - LENGTH_BASE[index] as usize) as u32,
                        extra,
                    );
                }
                let code = distance_symbol(distance as usize);
                codes.distance(bits, code);
                let extra = u32::from(DISTANCE_EXTRA[code]);
                if extra > 0 {
                    bits.write(
                        (distance as usize - DISTANCE_BASE[code] as usize) as u32,
                        extra,
                    );
                }
            }
        }
    }
    codes.symbol(bits, 256);
}

fn length_symbol(length: usize) -> usize {
    let mut index = LENGTH_BASE.len() - 1;
    while LENGTH_BASE[index] as usize > length {
        index -= 1;
    }
    index
}

fn distance_symbol(distance: usize) -> usize {
    let mut index = DISTANCE_BASE.len() - 1;
    while DISTANCE_BASE[index] as usize > distance {
        index -= 1;
    }
    index
}

fn emit_fixed_symbol(bits: &mut Bits, symbol: u32) {
    // RFC 1951 §3.2.6: the fixed literal/length code.
    match symbol {
        0..=143 => bits.code(0x30 + symbol, 8),
        144..=255 => bits.code(0x190 + (symbol - 144), 9),
        256..=279 => bits.code(symbol - 256, 7),
        280..=287 => bits.code(0xC0 + (symbol - 280), 8),
        _ => unreachable!("literal/length symbol {symbol} is outside the fixed table"),
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

    /// A decoder for the two block types this encoder emits. It is deliberately
    /// independent of the encoder's bookkeeping: a round trip proves the codes,
    /// the bit order and the match lengths agree with RFC 1951.
    fn inflate(stream: &[u8]) -> Vec<u8> {
        assert_eq!(stream[0], 0x78, "zlib CMF");
        assert_eq!((u16::from(stream[0]) * 256 + u16::from(stream[1])) % 31, 0);
        let body = &stream[2..stream.len() - 4];
        let mut reader = Reader::new(body);
        let mut out = Vec::new();
        loop {
            let last = reader.bits(1) == 1;
            match reader.bits(2) {
                1 => {
                    let (litlen, distance) = fixed_tables();
                    read_tokens(&mut reader, &litlen, &distance, &mut out);
                }
                2 => {
                    let (litlen, distance) = dynamic_tables(&mut reader);
                    read_tokens(&mut reader, &litlen, &distance, &mut out);
                }
                other => panic!("encoder must not emit BTYPE {other}"),
            }
            if last {
                break;
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

    fn read_tokens(reader: &mut Reader<'_>, litlen: &Table, distance: &Table, out: &mut Vec<u8>) {
        loop {
            let symbol = litlen.read(reader);
            match symbol {
                0..=255 => out.push(symbol as u8),
                256 => return,
                _ => {
                    let length = reader.length(symbol);
                    let code = distance.read(reader) as usize;
                    let distance = DISTANCE_BASE[code] as usize
                        + reader.bits(u32::from(DISTANCE_EXTRA[code])) as usize;
                    assert!(distance <= out.len(), "match reaches before the start");
                    for _ in 0..length {
                        let byte = out[out.len() - distance];
                        out.push(byte);
                    }
                }
            }
        }
    }

    fn fixed_tables() -> (Table, Table) {
        let mut litlen = vec![0u8; 288];
        for (symbol, length) in litlen.iter_mut().enumerate() {
            *length = match symbol {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        (
            Table::from_lengths(&litlen),
            Table::from_lengths(&[5u8; 32]),
        )
    }

    fn dynamic_tables(reader: &mut Reader<'_>) -> (Table, Table) {
        let hlit = reader.bits(5) as usize + 257;
        let hdist = reader.bits(5) as usize + 1;
        let hclen = reader.bits(4) as usize + 4;
        let mut code_lengths = [0u8; 19];
        for index in 0..hclen {
            code_lengths[CODE_LENGTH_ORDER[index]] = reader.bits(3) as u8;
        }
        let code_length = Table::from_lengths(&code_lengths);
        let mut lengths = vec![0u8; hlit + hdist];
        let mut position = 0;
        while position < lengths.len() {
            let symbol = code_length.read(reader);
            match symbol {
                0..=15 => {
                    lengths[position] = symbol as u8;
                    position += 1;
                }
                16 => {
                    let repeat = 3 + reader.bits(2) as usize;
                    let value = lengths[position - 1];
                    for _ in 0..repeat {
                        lengths[position] = value;
                        position += 1;
                    }
                }
                17 => {
                    let repeat = 3 + reader.bits(3) as usize;
                    for _ in 0..repeat {
                        lengths[position] = 0;
                        position += 1;
                    }
                }
                18 => {
                    let repeat = 11 + reader.bits(7) as usize;
                    for _ in 0..repeat {
                        lengths[position] = 0;
                        position += 1;
                    }
                }
                other => panic!("code-length symbol {other}"),
            }
            assert!(position <= lengths.len(), "run runs past the table");
        }
        (
            Table::from_lengths(&lengths[..hlit]),
            Table::from_lengths(&lengths[hlit..]),
        )
    }

    /// Decoding table: for each length, the `(code, symbol)` pairs, so a
    /// lookup never depends on how codes were generated.
    struct Table {
        by_length: Vec<Vec<(u32, u32)>>,
    }

    impl Table {
        fn from_lengths(lengths: &[u8]) -> Self {
            let maximum = lengths.iter().copied().max().unwrap_or(0) as usize;
            let mut by_length = vec![Vec::new(); maximum + 1];
            let codes = canonical(lengths);
            for (symbol, length) in lengths.iter().enumerate() {
                if *length > 0 {
                    by_length[*length as usize].push((u32::from(codes[symbol]), symbol as u32));
                }
            }
            // Index zero is unused; keep it so indices line up with lengths.
            assert!(by_length.len() > 1 || lengths.iter().all(|length| *length == 0));
            Self { by_length }
        }

        fn read(&self, reader: &mut Reader<'_>) -> u32 {
            let mut code = 0u32;
            for length in 1..self.by_length.len() {
                code = (code << 1) | reader.bit();
                for (candidate, symbol) in &self.by_length[length] {
                    if *candidate == code {
                        return *symbol;
                    }
                }
            }
            panic!("no symbol matches code {code:#x}");
        }
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

        /// Plain integers: least significant bit first.
        fn bits(&mut self, count: u32) -> u32 {
            let mut value = 0;
            for index in 0..count {
                value |= self.bit() << index;
            }
            value
        }

        fn length(&mut self, symbol: u32) -> usize {
            let index = (symbol - 257) as usize;
            LENGTH_BASE[index] as usize + self.bits(u32::from(LENGTH_EXTRA[index])) as usize
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
    fn skewed_binary_round_trips() {
        // A distribution that looks like a font program: a few bytes dominate
        // and the rest appear once, which is what makes dynamic tables pay.
        let mut data = Vec::new();
        for index in 0..20_000 {
            data.push(if index % 7 == 0 {
                0x00
            } else {
                (index % 251) as u8
            });
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

    #[test]
    fn a_dynamic_block_beats_the_fixed_one_on_a_skewed_alphabet() {
        // Two symbols only: the fixed table spends 8-9 bits per byte, a
        // two-symbol table one. The encoder must pick the dynamic block.
        let mut data = Vec::new();
        for index in 0..4000 {
            data.push(if index % 97 == 0 { b'b' } else { b'a' });
        }
        let packed = zlib(&data).expect("compresses");
        let mut reader = Reader::new(&packed[2..packed.len() - 4]);
        assert_eq!(reader.bits(1), 1, "BFINAL");
        assert_eq!(reader.bits(2), 2, "a dynamic block wins here");
    }

    #[test]
    fn lengths_never_exceed_the_limit() {
        // A Fibonacci-shaped alphabet pushes the natural tree past 15 bits.
        let mut frequencies = vec![0u32; LITLEN_SYMBOLS];
        let (mut first, mut second) = (1u32, 1u32);
        for frequency in frequencies.iter_mut().take(30) {
            *frequency = first;
            let next = first + second;
            first = second;
            second = next;
        }
        let depths = tree_depths(&frequencies);
        assert!(
            depths.iter().copied().max().unwrap_or(0) > 15,
            "the fixture must actually overflow"
        );
        let limited = limit_depths(&frequencies, &depths, 15);
        assert!(limited.iter().copied().max().unwrap_or(0) <= 15);
        // The balanced fallback is complete: the Kraft sum is exactly one.
        let kraft: u32 = limited
            .iter()
            .filter(|length| **length > 0)
            .map(|length| 1u32 << (15 - length))
            .sum();
        assert_eq!(kraft, 1 << 15, "the fallback must be complete");
    }
}
