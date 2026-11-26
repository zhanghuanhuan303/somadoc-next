//! Package relationships and image parts.
//!
//! `word/_rels/document.xml.rels` maps relationship ids to targets. Only
//! relationships whose type ends in `/image` are interesting; each one is
//! resolved to a zip entry and, when present, materialised exactly once.
//!
//! Nothing here is fatal. A missing relationships part means "no images", and
//! a relationship that points at an absent or unreadable entry is ignored so
//! the surrounding text still imports.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::xml::{Token, Tokenizer};
use crate::zip::Archive;
use crate::MediaFile;

/// Relationship id to media source name (`media/image1.png`).
#[derive(Default)]
pub(crate) struct ImageIndex {
    entries: Vec<(String, String)>,
}

impl ImageIndex {
    pub(crate) fn get(&self, id: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, source)| source.as_str())
    }
}

/// Read every image relationship and the media parts it points at.
pub(crate) fn load(archive: &Archive<'_>) -> (ImageIndex, Vec<MediaFile>) {
    let relationships = archive.read(b"word/_rels/document.xml.rels").ok().flatten();
    let Some(xml) = relationships
        .as_deref()
        .and_then(|bytes| core::str::from_utf8(bytes).ok())
    else {
        return (ImageIndex::default(), Vec::new());
    };

    let mut entries: Vec<(String, String)> = Vec::new();
    let mut media = Vec::new();
    // Zip name -> source name, so two relationships that share a part share one
    // `MediaFile`.
    let mut loaded: Vec<(String, String)> = Vec::new();

    for (id, target) in image_relationships(xml) {
        if entries.iter().any(|(known, _)| known == &id) {
            continue;
        }
        let Some((zip_name, source)) = package_target(&target) else {
            continue;
        };
        if let Some((_, existing)) = loaded.iter().find(|(name, _)| name == &zip_name) {
            entries.push((id, existing.clone()));
            continue;
        }
        if let Some(bytes) = archive.read(zip_name.as_bytes()).ok().flatten() {
            loaded.push((zip_name, source.clone()));
            entries.push((id, source.clone()));
            media.push(MediaFile {
                name: source,
                bytes,
            });
        }
    }

    (ImageIndex { entries }, media)
}

fn image_relationships(xml: &str) -> Vec<(String, String)> {
    let mut relationships = Vec::new();
    let mut tokens = Tokenizer::new(xml);
    while let Ok(Some(token)) = tokens.next_token() {
        let Token::Start(tag) = token else {
            continue;
        };
        if tag.name != "Relationship" {
            continue;
        }
        let Some(kind) = tag.attr("Type") else {
            continue;
        };
        if !kind.trim().ends_with("/image") {
            continue;
        }
        let Some(id) = tag.attr("Id").map(str::trim).filter(|id| !id.is_empty()) else {
            continue;
        };
        let Some(target) = tag
            .attr("Target")
            .map(str::trim)
            .filter(|target| !target.is_empty())
        else {
            continue;
        };
        relationships.push((String::from(id), String::from(target)));
    }
    relationships
}

/// Turn a relationship target into `(zip entry name, media source name)`.
///
/// Targets are relative to `word/document.xml`, so `media/image1.png` lives at
/// `word/media/image1.png`; an absolute `/word/media/image1.png` needs no join.
/// The source name drops the package's `word/` prefix, because that is what
/// `ImageRef::src` (and therefore [`MediaFile::name`]) carries.
fn package_target(target: &str) -> Option<(String, String)> {
    let target = target.trim();
    if target.is_empty() || target.contains("://") {
        return None;
    }

    let absolute = target.starts_with('/');
    let mut segments: Vec<&str> = if absolute { Vec::new() } else { vec!["word"] };
    for segment in target.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                // Never let a target escape the package root.
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        return None;
    }

    let mut zip_name = String::new();
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            zip_name.push('/');
        }
        zip_name.push_str(segment);
    }
    let source = match zip_name.strip_prefix("word/") {
        Some(rest) => String::from(rest),
        None => zip_name.clone(),
    };
    if source.is_empty() {
        return None;
    }
    Some((zip_name, source))
}
