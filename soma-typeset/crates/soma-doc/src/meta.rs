//! Document metadata.
//!
//! Metadata is semantic: who wrote it, what it is called, which template it
//! asks for, and free-form fields a template may expose. Page geometry, fonts,
//! and colours are not here; they belong to `soma-template`.

use alloc::string::String;
use alloc::vec::Vec;

use crate::inline::Inline;

/// A calendar date without time zones. Integers only; no floats anywhere in the
/// IR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Date {
    pub const fn new(year: i32, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    /// Range and calendar check used by validation: 1 <= year <= 9999,
    /// 1 <= month <= 12, and the day exists in that month under the Gregorian
    /// calendar. The year is never invented: an impossible date is rejected so
    /// a bad source is reported instead of propagated.
    pub const fn is_in_range(self) -> bool {
        self.year >= 1
            && self.year <= 9999
            && self.month >= 1
            && self.month <= 12
            && self.day >= 1
            && self.day <= days_in_month(self.year, self.month)
    }

    /// True when `year` is a Gregorian leap year: divisible by 4, except
    /// centuries that are not divisible by 400.
    const fn is_leap_year(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }
}

/// Number of days in a Gregorian month. Months outside 1..=12 yield 0 so that
/// [`Date::is_in_range`] rejects them independently of the day.
const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if Date::is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// One author.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Author {
    pub name: String,
    pub affiliation: Option<String>,
    pub email: Option<String>,
    pub orcid: Option<String>,
}

impl Author {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn with_affiliation(mut self, affiliation: impl Into<String>) -> Self {
        self.affiliation = Some(affiliation.into());
        self
    }

    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }
}

/// Free-form key/value pair, in document order, for template slots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetaField {
    pub key: String,
    pub value: String,
}

impl MetaField {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Which template the document asks for. Resolution and validation happen in
/// `soma-template`; the IR only carries the reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateRef {
    pub name: String,
    pub version: Option<String>,
}

impl TemplateRef {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
        }
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
}

/// Image reference without node identity. Used by metadata and figures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRef {
    pub src: String,
    pub alt: Option<String>,
    pub title: Option<String>,
    /// Intrinsic pixel size, when the front end knows it. This is a fact about
    /// the asset (used for aspect ratio), never a layout width.
    pub intrinsic: Option<ImageIntrinsic>,
}

impl ImageRef {
    pub fn new(src: impl Into<String>) -> Self {
        Self {
            src: src.into(),
            alt: None,
            title: None,
            intrinsic: None,
        }
    }

    pub fn with_alt(mut self, alt: impl Into<String>) -> Self {
        self.alt = Some(alt.into());
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_intrinsic(mut self, width_px: u32, height_px: u32) -> Self {
        self.intrinsic = Some(ImageIntrinsic {
            width_px,
            height_px,
        });
        self
    }
}

/// Intrinsic pixel size of an image asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageIntrinsic {
    pub width_px: u32,
    pub height_px: u32,
}

/// Data a cover template can lay out. Structure is fixed; the template decides
/// whether to show any of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoverData {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub authors: Vec<String>,
    pub date: Option<String>,
    pub logo: Option<ImageRef>,
    pub fields: Vec<MetaField>,
}

impl CoverData {
    pub const fn new() -> Self {
        Self {
            title: None,
            subtitle: None,
            authors: Vec::new(),
            date: None,
            logo: None,
            fields: Vec::new(),
        }
    }
}

/// Document-level metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentMeta {
    /// Rich title: markup in a title is common (`**Bold** title`).
    pub title: Option<Vec<Inline>>,
    pub authors: Vec<Author>,
    /// Primary language tag, e.g. `zh-Hans`.
    pub language: Option<String>,
    pub date: Option<Date>,
    pub keywords: Vec<String>,
    pub template: Option<TemplateRef>,
    pub cover: Option<CoverData>,
    pub fields: Vec<MetaField>,
}

impl DocumentMeta {
    pub const fn new() -> Self {
        Self {
            title: None,
            authors: Vec::new(),
            language: None,
            date: None,
            keywords: Vec::new(),
            template: None,
            cover: None,
            fields: Vec::new(),
        }
    }

    pub fn with_title(mut self, title: Vec<Inline>) -> Self {
        self.title = Some(title);
        self
    }

    pub fn with_author(mut self, author: Author) -> Self {
        self.authors.push(author);
        self
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    pub fn with_date(mut self, date: Date) -> Self {
        self.date = Some(date);
        self
    }

    pub fn with_keyword(mut self, keyword: impl Into<String>) -> Self {
        self.keywords.push(keyword.into());
        self
    }

    pub fn with_template(mut self, template: TemplateRef) -> Self {
        self.template = Some(template);
        self
    }

    pub fn with_cover(mut self, cover: CoverData) -> Self {
        self.cover = Some(cover);
        self
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.push(MetaField::new(key, value));
        self
    }
}
