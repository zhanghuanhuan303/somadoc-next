//! Deterministic hashing for DocIR.
//!
//! Same FNV-1a variant as `ParamHash`, so the project has one hash primitive.
//! Hashes are computed over canonical JSON written into a [`core::fmt::Write`]
//! sink; there is no second serialization path.

use crate::canonical::{self, CanonicalMode};
use crate::doc::Document;

/// 64-bit document hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocHash(pub u64);

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Incremental FNV-1a sink. Writing to it cannot fail.
pub(crate) struct FnvWriter {
    state: u64,
}

impl FnvWriter {
    pub(crate) const fn new() -> Self {
        Self { state: FNV_OFFSET }
    }

    pub(crate) const fn finish(self) -> DocHash {
        DocHash(self.state)
    }

    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= *byte as u64;
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }
}

impl core::fmt::Write for FnvWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.update(s.as_bytes());
        Ok(())
    }
}

/// FNV-1a over raw bytes.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut writer = FnvWriter::new();
    writer.update(bytes);
    writer.state
}

/// Full hash: ids, spans, source map, and the template reference included.
///
/// Use this to prove exact reproducibility of one input.
pub fn document_hash(doc: &Document) -> DocHash {
    let mut writer = FnvWriter::new();
    canonical::write_document(&mut writer, doc, CanonicalMode::Full)
        .expect("hashing into a sink cannot fail");
    writer.finish()
}

/// Content hash: ids, spans, source map, and the template reference normalised
/// away.
///
/// Two documents with the same semantic content hash equal even when they were
/// parsed from different files or assigned different node ids. This is the cache
/// key for "did the content change?".
pub fn content_hash(doc: &Document) -> DocHash {
    let mut writer = FnvWriter::new();
    canonical::write_document(&mut writer, doc, CanonicalMode::Content)
        .expect("hashing into a sink cannot fail");
    writer.finish()
}
