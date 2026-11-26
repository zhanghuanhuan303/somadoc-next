//! A dependency-free, line-oriented code lexer.
//!
//! Highlighting is presentation, so this crate only classifies characters; it
//! never measures, breaks or paints. A [`Lexer`] keeps the state a line needs
//! from the previous one (block comments, multi-line strings, Markdown fences)
//! and returns character-indexed [`ClassSpan`]s. Colors come from the
//! template's `syntax:` palette (ADR-0091).
//!
//! The lexer is deliberately conservative: an unknown construct stays the code
//! block's own color. Every input character is emitted at most once, spans are
//! sorted and non-overlapping, and no input can make it panic.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use soma_core::SyntaxClass;

/// A classified run of characters. `start`/`end` are character indices into
/// the line, not bytes, so a caller can match them against atoms directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassSpan {
    pub start: usize,
    pub end: usize,
    pub class: SyntaxClass,
}

/// A stateful highlighter for one code block.
pub struct Lexer {
    language: Language,
    state: State,
}

#[derive(Clone, Copy, Debug, Default)]
struct State {
    /// Open block-comment depth (Rust nests; the other C-like languages use 1).
    block_comment: u8,
    /// A string that continues on the next line (shell, JavaScript template).
    quote: Option<char>,
    /// An open Python triple-quoted string and its quote count.
    triple: Option<(char, u8)>,
    /// A Rust raw string and the number of `#` in its delimiter.
    raw_hashes: Option<u8>,
    /// An open HTML comment.
    html_comment: bool,
    /// An open Markdown fence: marker and count.
    md_fence: Option<(char, usize)>,
}

impl Lexer {
    /// `lang` is a fence info string (`rust,ignore` is accepted); unknown tags
    /// select plain text.
    pub fn new(lang: Option<&str>) -> Self {
        Self {
            language: Language::parse(lang),
            state: State::default(),
        }
    }

    /// The canonical name of the selected profile (`plain` for unknown tags).
    pub fn language_name(&self) -> &'static str {
        self.language.name()
    }

    /// Classify one source line and advance the multi-line state.
    pub fn line(&mut self, text: &str) -> Vec<ClassSpan> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        match self.language {
            Language::Plain => {}
            Language::Rust => self.scan_code(&chars, &mut out),
            Language::Markdown => self.scan_markdown(&chars, &mut out),
            Language::Shell => self.scan_shell(&chars, &mut out),
            Language::Latex => self.scan_latex(&chars, &mut out),
            Language::Html => self.scan_html(&chars, &mut out),
            Language::Css => self.scan_css(&chars, &mut out),
            Language::Yaml | Language::Toml => self.scan_key_value(&chars, &mut out),
            _ => self.scan_code(&chars, &mut out),
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Language {
    Plain,
    Rust,
    C,
    Cpp,
    Shell,
    Markdown,
    Latex,
    Python,
    JavaScript,
    TypeScript,
    Json,
    Yaml,
    Toml,
    Html,
    Css,
    Sql,
    Go,
    Java,
    Kotlin,
    Swift,
    CSharp,
    Ruby,
    Php,
    Lua,
    Haskell,
}

impl Language {
    fn parse(lang: Option<&str>) -> Self {
        let Some(lang) = lang else {
            return Self::Plain;
        };
        let mut word = String::new();
        for character in lang.chars() {
            if character == ',' || character == '{' || character.is_whitespace() {
                break;
            }
            word.push(character);
        }
        let word = word.to_ascii_lowercase();
        match word.as_str() {
            "rust" | "rs" => Self::Rust,
            "c" | "h" => Self::C,
            "cpp" | "c++" | "cxx" | "cc" | "hpp" | "hh" | "hxx" => Self::Cpp,
            "sh" | "shell" | "bash" | "zsh" | "fish" | "console" | "terminal" => Self::Shell,
            "md" | "markdown" | "mdown" => Self::Markdown,
            "tex" | "latex" => Self::Latex,
            "py" | "python" => Self::Python,
            "js" | "javascript" | "jsx" | "node" => Self::JavaScript,
            "ts" | "typescript" | "tsx" => Self::TypeScript,
            "json" | "jsonc" => Self::Json,
            "yaml" | "yml" => Self::Yaml,
            "toml" => Self::Toml,
            "html" | "htm" | "xml" | "xhtml" | "svg" => Self::Html,
            "css" | "scss" | "less" => Self::Css,
            "sql" | "mysql" | "postgres" | "postgresql" | "sqlite" => Self::Sql,
            "go" | "golang" => Self::Go,
            "java" => Self::Java,
            "kt" | "kotlin" => Self::Kotlin,
            "swift" => Self::Swift,
            "cs" | "c#" | "csharp" => Self::CSharp,
            "rb" | "ruby" => Self::Ruby,
            "php" => Self::Php,
            "lua" => Self::Lua,
            "hs" | "haskell" => Self::Haskell,
            "text" | "txt" | "plain" | "plaintext" => Self::Plain,
            _ => Self::Plain,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Rust => "rust",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Shell => "shell",
            Self::Markdown => "markdown",
            Self::Latex => "latex",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
            Self::Html => "html",
            Self::Css => "css",
            Self::Sql => "sql",
            Self::Go => "go",
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::Swift => "swift",
            Self::CSharp => "csharp",
            Self::Ruby => "ruby",
            Self::Php => "php",
            Self::Lua => "lua",
            Self::Haskell => "haskell",
        }
    }
}

/// A language-shaped set of syntactic facts used by the shared scanner.
struct Profile {
    line_comments: &'static [&'static str],
    hash_comment: bool,
    block_comment: bool,
    preprocessor: bool,
    dollar_vars: bool,
    template_strings: bool,
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    literals: &'static [&'static str],
    uppercase_types: bool,
    case_insensitive: bool,
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "box", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "macro", "match", "mod",
    "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
    "true", "type", "unsafe", "use", "where", "while", "union", "yield",
];
const RUST_TYPES: &[&str] = &[
    "bool",
    "char",
    concat!("f", "32"),
    concat!("f", "64"),
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "str",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "String",
    "Vec",
    "Option",
    "Result",
    "Box",
];
const C_KEYWORDS: &[&str] = &[
    "auto", "break", "case", "const", "continue", "default", "do", "else", "enum", "extern", "for",
    "goto", "if", "inline", "register", "restrict", "return", "sizeof", "static", "struct",
    "switch", "typedef", "union", "volatile", "while", "_Bool", "bool", "true", "false",
];
const C_TYPES: &[&str] = &[
    "char", "double", "float", "int", "long", "short", "signed", "unsigned", "void", "size_t",
    "ssize_t", "int8_t", "int16_t", "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t",
    "uint64_t", "FILE",
];
const CPP_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "asm",
    "auto",
    "break",
    "case",
    "catch",
    "class",
    "co_await",
    "co_return",
    "co_yield",
    "concept",
    "const",
    "consteval",
    "constexpr",
    "constinit",
    "const_cast",
    "continue",
    "decltype",
    "default",
    "delete",
    "do",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "final",
    "for",
    "friend",
    "goto",
    "if",
    "import",
    "inline",
    "module",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "not",
    "nullptr",
    "operator",
    "or",
    "override",
    "private",
    "protected",
    "public",
    "register",
    "reinterpret_cast",
    "requires",
    "return",
    "sizeof",
    "static",
    "static_assert",
    "static_cast",
    "struct",
    "switch",
    "template",
    "this",
    "thread_local",
    "throw",
    "true",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "using",
    "virtual",
    "volatile",
    "while",
];
const CPP_TYPES: &[&str] = &[
    "bool",
    "char",
    "double",
    "float",
    "int",
    "long",
    "short",
    "signed",
    "unsigned",
    "void",
    "size_t",
    "string",
    "vector",
    "map",
    "set",
    "unique_ptr",
    "shared_ptr",
    "optional",
    "variant",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
];
const SHELL_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "select", "time", "coproc", "break", "continue", "return", "export", "local",
    "readonly", "declare", "typeset", "unset", "shift", "source", "alias", "unalias", "set",
    "trap", "exit", "exec", "eval", "test", "true", "false",
];
const PYTHON_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "False", "finally", "for", "from", "global", "if", "import", "in", "is",
    "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True", "try", "while",
    "with", "yield", "match", "case",
];
const PYTHON_TYPES: &[&str] = &[
    "bool",
    "bytes",
    "complex",
    "dict",
    "float",
    "frozenset",
    "int",
    "list",
    "object",
    "set",
    "str",
    "tuple",
    "type",
];
const JS_KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "interface",
    "type",
    "enum",
    "implements",
    "namespace",
    "declare",
    "readonly",
    "private",
    "public",
    "protected",
    "abstract",
    "as",
    "satisfies",
];
const JS_TYPES: &[&str] = &[
    "any", "bigint", "boolean", "never", "number", "object", "string", "symbol", "unknown", "void",
];
const SQL_KEYWORDS: &[&str] = &[
    "add",
    "all",
    "alter",
    "and",
    "as",
    "asc",
    "between",
    "by",
    "case",
    "column",
    "create",
    "cross",
    "default",
    "delete",
    "desc",
    "distinct",
    "drop",
    "else",
    "end",
    "exists",
    "foreign",
    "from",
    "full",
    "group",
    "having",
    "in",
    "index",
    "inner",
    "insert",
    "into",
    "is",
    "join",
    "key",
    "left",
    "like",
    "limit",
    "not",
    "null",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "primary",
    "references",
    "right",
    "select",
    "set",
    "table",
    "then",
    "union",
    "unique",
    "update",
    "values",
    "view",
    "when",
    "where",
    "with",
];
const SQL_TYPES: &[&str] = &[
    "bigint",
    "boolean",
    "char",
    "date",
    "decimal",
    "double",
    "float",
    "int",
    "integer",
    "numeric",
    "real",
    "serial",
    "smallint",
    "text",
    "time",
    "timestamp",
    "varchar",
];
const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];
const GO_TYPES: &[&str] = &[
    "bool",
    "byte",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
];
const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "interface",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "record",
    "return",
    "sealed",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "var",
    "volatile",
    "while",
    "yield",
    "permits",
];
const JAVA_TYPES: &[&str] = &[
    "boolean", "byte", "char", "double", "float", "int", "long", "short", "void", "String",
    "Object", "Integer", "Boolean", "List", "Map", "Set",
];
const KOTLIN_KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "val",
    "var",
    "when",
    "while",
    "by",
    "catch",
    "constructor",
    "data",
    "finally",
    "get",
    "import",
    "init",
    "set",
    "where",
    "abstract",
    "actual",
    "annotation",
    "companion",
    "const",
    "crossinline",
    "enum",
    "expect",
    "external",
    "final",
    "infix",
    "inline",
    "inner",
    "internal",
    "lateinit",
    "noinline",
    "open",
    "operator",
    "out",
    "override",
    "private",
    "protected",
    "public",
    "reified",
    "sealed",
    "suspend",
    "tailrec",
    "vararg",
];
const KOTLIN_TYPES: &[&str] = &[
    "Any", "Boolean", "Byte", "Char", "Double", "Float", "Int", "Long", "Nothing", "Short",
    "String", "Unit", "Array", "List", "Map", "Set",
];
const SWIFT_KEYWORDS: &[&str] = &[
    "associatedtype",
    "class",
    "deinit",
    "enum",
    "extension",
    "fileprivate",
    "func",
    "import",
    "init",
    "inout",
    "internal",
    "let",
    "open",
    "operator",
    "private",
    "protocol",
    "public",
    "rethrows",
    "static",
    "struct",
    "subscript",
    "typealias",
    "var",
    "break",
    "case",
    "continue",
    "default",
    "defer",
    "do",
    "else",
    "fallthrough",
    "for",
    "guard",
    "if",
    "in",
    "repeat",
    "return",
    "switch",
    "where",
    "while",
    "as",
    "catch",
    "false",
    "is",
    "nil",
    "super",
    "self",
    "Self",
    "throw",
    "throws",
    "true",
    "try",
];
const SWIFT_TYPES: &[&str] = &[
    "Bool",
    "Character",
    "Double",
    "Float",
    "Int",
    "String",
    "UInt",
    "Void",
    "Array",
    "Dictionary",
    "Set",
    "Optional",
];
const CSHARP_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "base",
    "break",
    "case",
    "catch",
    "checked",
    "class",
    "const",
    "continue",
    "default",
    "delegate",
    "do",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "interface",
    "internal",
    "is",
    "lock",
    "namespace",
    "new",
    "null",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sealed",
    "sizeof",
    "stackalloc",
    "static",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "unchecked",
    "unsafe",
    "using",
    "virtual",
    "volatile",
    "while",
    "async",
    "await",
    "record",
    "var",
    "init",
];
const CSHARP_TYPES: &[&str] = &[
    "bool",
    "byte",
    "char",
    "decimal",
    "double",
    "float",
    "int",
    "long",
    "object",
    "sbyte",
    "short",
    "string",
    "uint",
    "ulong",
    "ushort",
    "void",
    "String",
    "Object",
    "List",
    "Dictionary",
];
const RUBY_KEYWORDS: &[&str] = &[
    "alias",
    "and",
    "begin",
    "break",
    "case",
    "class",
    "def",
    "defined?",
    "do",
    "else",
    "elsif",
    "end",
    "ensure",
    "false",
    "for",
    "if",
    "in",
    "module",
    "next",
    "nil",
    "not",
    "or",
    "redo",
    "rescue",
    "retry",
    "return",
    "self",
    "super",
    "then",
    "true",
    "undef",
    "unless",
    "until",
    "when",
    "while",
    "yield",
    "require",
    "require_relative",
    "attr_reader",
    "attr_writer",
    "attr_accessor",
];
const RUBY_TYPES: &[&str] = &[
    "Array", "Hash", "String", "Integer", "Float", "Symbol", "Proc", "Range", "Regexp", "Time",
];
const PHP_KEYWORDS: &[&str] = &[
    "abstract",
    "and",
    "array",
    "as",
    "break",
    "callable",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "do",
    "echo",
    "else",
    "elseif",
    "empty",
    "enddeclare",
    "endfor",
    "endforeach",
    "endif",
    "endswitch",
    "endwhile",
    "eval",
    "exit",
    "extends",
    "final",
    "finally",
    "fn",
    "for",
    "foreach",
    "function",
    "global",
    "goto",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "insteadof",
    "interface",
    "isset",
    "list",
    "match",
    "namespace",
    "new",
    "or",
    "print",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "require_once",
    "return",
    "static",
    "switch",
    "throw",
    "trait",
    "try",
    "unset",
    "use",
    "var",
    "while",
    "xor",
    "yield",
];
const PHP_TYPES: &[&str] = &[
    "bool", "float", "int", "string", "array", "object", "callable", "iterable", "void", "mixed",
    "null", "false", "true",
];
const LUA_KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];
const HASKELL_KEYWORDS: &[&str] = &[
    "case",
    "class",
    "data",
    "default",
    "deriving",
    "do",
    "else",
    "foreign",
    "if",
    "import",
    "in",
    "infix",
    "infixl",
    "infixr",
    "instance",
    "let",
    "module",
    "newtype",
    "of",
    "then",
    "type",
    "where",
    "forall",
    "qualified",
    "as",
    "hiding",
];
const HASKELL_TYPES: &[&str] = &[
    "Bool", "Char", "Double", "Either", "Float", "Int", "Integer", "IO", "Maybe", "Ordering",
    "String", "Word",
];

fn profile(language: Language) -> Profile {
    match language {
        Language::Rust => Profile {
            line_comments: &["//"],
            hash_comment: false,
            block_comment: true,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: RUST_KEYWORDS,
            types: RUST_TYPES,
            literals: &["true", "false"],
            uppercase_types: true,
            case_insensitive: false,
        },
        Language::C => Profile {
            line_comments: &["//"],
            hash_comment: false,
            block_comment: true,
            preprocessor: true,
            dollar_vars: false,
            template_strings: false,
            keywords: C_KEYWORDS,
            types: C_TYPES,
            literals: &["true", "false", "NULL"],
            uppercase_types: false,
            case_insensitive: false,
        },
        Language::Cpp => Profile {
            line_comments: &["//"],
            hash_comment: false,
            block_comment: true,
            preprocessor: true,
            dollar_vars: false,
            template_strings: false,
            keywords: CPP_KEYWORDS,
            types: CPP_TYPES,
            literals: &["true", "false", "nullptr"],
            uppercase_types: true,
            case_insensitive: false,
        },
        Language::Python => Profile {
            line_comments: &[],
            hash_comment: true,
            block_comment: false,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: PYTHON_KEYWORDS,
            types: PYTHON_TYPES,
            literals: &["True", "False", "None"],
            uppercase_types: false,
            case_insensitive: false,
        },
        Language::JavaScript | Language::TypeScript => Profile {
            line_comments: &["//"],
            hash_comment: false,
            block_comment: true,
            preprocessor: false,
            dollar_vars: false,
            template_strings: true,
            keywords: JS_KEYWORDS,
            types: JS_TYPES,
            literals: &["true", "false", "null", "undefined"],
            uppercase_types: true,
            case_insensitive: false,
        },
        Language::Json => Profile {
            line_comments: &[],
            hash_comment: false,
            block_comment: false,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: &[],
            types: &[],
            literals: &["true", "false", "null"],
            uppercase_types: false,
            case_insensitive: false,
        },
        Language::Sql => Profile {
            line_comments: &["--"],
            hash_comment: false,
            block_comment: true,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: SQL_KEYWORDS,
            types: SQL_TYPES,
            literals: &["null", "true", "false"],
            uppercase_types: false,
            case_insensitive: true,
        },
        Language::Go => Profile {
            line_comments: &["//"],
            hash_comment: false,
            block_comment: true,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: GO_KEYWORDS,
            types: GO_TYPES,
            literals: &["true", "false", "nil", "iota"],
            uppercase_types: false,
            case_insensitive: false,
        },
        Language::Java => Profile {
            line_comments: &["//"],
            hash_comment: false,
            block_comment: true,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: JAVA_KEYWORDS,
            types: JAVA_TYPES,
            literals: &["true", "false", "null"],
            uppercase_types: true,
            case_insensitive: false,
        },
        Language::Kotlin => Profile {
            line_comments: &["//"],
            hash_comment: false,
            block_comment: true,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: KOTLIN_KEYWORDS,
            types: KOTLIN_TYPES,
            literals: &["true", "false", "null"],
            uppercase_types: true,
            case_insensitive: false,
        },
        Language::Swift => Profile {
            line_comments: &["//"],
            hash_comment: false,
            block_comment: true,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: SWIFT_KEYWORDS,
            types: SWIFT_TYPES,
            literals: &["true", "false", "nil"],
            uppercase_types: true,
            case_insensitive: false,
        },
        Language::CSharp => Profile {
            line_comments: &["//"],
            hash_comment: false,
            block_comment: true,
            preprocessor: true,
            dollar_vars: false,
            template_strings: false,
            keywords: CSHARP_KEYWORDS,
            types: CSHARP_TYPES,
            literals: &["true", "false", "null"],
            uppercase_types: true,
            case_insensitive: false,
        },
        Language::Ruby => Profile {
            line_comments: &[],
            hash_comment: true,
            block_comment: false,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: RUBY_KEYWORDS,
            types: RUBY_TYPES,
            literals: &["true", "false", "nil"],
            uppercase_types: true,
            case_insensitive: false,
        },
        Language::Php => Profile {
            line_comments: &["//"],
            hash_comment: true,
            block_comment: true,
            preprocessor: false,
            dollar_vars: true,
            template_strings: false,
            keywords: PHP_KEYWORDS,
            types: PHP_TYPES,
            literals: &["true", "false", "null"],
            uppercase_types: false,
            case_insensitive: false,
        },
        Language::Lua => Profile {
            line_comments: &["--"],
            hash_comment: false,
            block_comment: false,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: LUA_KEYWORDS,
            types: &[],
            literals: &["true", "false", "nil"],
            uppercase_types: false,
            case_insensitive: false,
        },
        Language::Haskell => Profile {
            line_comments: &["--"],
            hash_comment: false,
            block_comment: false,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: HASKELL_KEYWORDS,
            types: HASKELL_TYPES,
            literals: &["True", "False", "Nothing", "Just"],
            uppercase_types: true,
            case_insensitive: false,
        },
        Language::Plain
        | Language::Shell
        | Language::Markdown
        | Language::Latex
        | Language::Yaml
        | Language::Toml
        | Language::Html
        | Language::Css => Profile {
            line_comments: &[],
            hash_comment: false,
            block_comment: false,
            preprocessor: false,
            dollar_vars: false,
            template_strings: false,
            keywords: &[],
            types: &[],
            literals: &[],
            uppercase_types: false,
            case_insensitive: false,
        },
    }
}

fn emit(out: &mut Vec<ClassSpan>, start: usize, end: usize, class: SyntaxClass) {
    if end <= start {
        return;
    }
    if let Some(last) = out.last_mut() {
        if last.end == start && last.class == class {
            last.end = end;
            return;
        }
    }
    out.push(ClassSpan { start, end, class });
}

fn starts_with(chars: &[char], at: usize, pattern: &str) -> bool {
    pattern
        .chars()
        .enumerate()
        .all(|(offset, character)| chars.get(at + offset) == Some(&character))
}

fn find_from(chars: &[char], from: usize, pattern: &str) -> Option<usize> {
    let pattern: Vec<char> = pattern.chars().collect();
    if pattern.is_empty() || from > chars.len() {
        return None;
    }
    (from..=chars.len().saturating_sub(pattern.len()))
        .find(|start| chars[*start..*start + pattern.len()] == pattern[..])
}

fn is_ident_start(character: char) -> bool {
    character == '_' || character.is_alphabetic()
}

fn is_ident_continue(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

fn next_non_space(chars: &[char], mut at: usize) -> Option<char> {
    while at < chars.len() && chars[at].is_whitespace() {
        at += 1;
    }
    chars.get(at).copied()
}

impl Lexer {
    fn scan_code(&mut self, chars: &[char], out: &mut Vec<ClassSpan>) {
        let language = self.language;
        let profile = profile(language);
        let len = chars.len();
        let mut at = 0;

        // Continuations from the previous line.
        if self.state.block_comment > 0 {
            at = self.close_block_comment(chars, 0, out);
            if self.state.block_comment > 0 {
                return;
            }
        }
        if let Some((quote, count)) = self.state.triple {
            at = self.close_triple(chars, 0, 0, quote, count, out);
            if self.state.triple.is_some() {
                return;
            }
        }
        if let Some(quote) = self.state.quote {
            at = self.close_quote(chars, 0, quote, out);
            if self.state.quote.is_some() {
                return;
            }
        }
        if let Some(hashes) = self.state.raw_hashes {
            at = self.close_raw(chars, 0, hashes, out);
            if self.state.raw_hashes.is_some() {
                return;
            }
        }

        while at < len {
            let character = chars[at];

            // Python triple quotes and Rust raw strings start before the
            // ordinary string branch.
            if language == Language::Python {
                if let Some(count) = triple_at(chars, at) {
                    let quote = chars[at];
                    let next = self.close_triple(chars, at, at + 3, quote, count, out);
                    at = next;
                    continue;
                }
            }
            if language == Language::Rust {
                if let Some((prefix, hashes)) = rust_raw_at(chars, at) {
                    let start = at;
                    let mut index = prefix;
                    // `prefix` points at the opening quote.
                    index += 1;
                    match find_raw_end(chars, index, hashes) {
                        Some(end) => {
                            emit(out, start, end, SyntaxClass::String);
                            at = end;
                        }
                        None => {
                            self.state.raw_hashes = Some(hashes);
                            emit(out, start, len, SyntaxClass::String);
                            return;
                        }
                    }
                    continue;
                }
                if starts_with(chars, at, "#[") || starts_with(chars, at, "#![") {
                    let start = at;
                    let mut index = at + if starts_with(chars, at, "#![") { 3 } else { 2 };
                    let mut depth = 1usize;
                    while index < len {
                        match chars[index] {
                            '[' => depth += 1,
                            ']' => {
                                depth -= 1;
                                index += 1;
                                if depth == 0 {
                                    break;
                                }
                                continue;
                            }
                            _ => {}
                        }
                        index += 1;
                    }
                    emit(out, start, index, SyntaxClass::Meta);
                    at = index;
                    continue;
                }
            }

            if character.is_whitespace() {
                at += 1;
                continue;
            }

            // Line comments.
            let mut line_comment = false;
            for marker in profile.line_comments {
                if starts_with(chars, at, marker) {
                    emit(out, at, len, SyntaxClass::Comment);
                    return;
                }
            }
            if profile.hash_comment && character == '#' {
                // In shell a `#` only starts a comment at a word boundary; the
                // shell scanner handles that case. Python/Ruby allow it after
                // any code.
                line_comment = true;
            }
            if line_comment {
                emit(out, at, len, SyntaxClass::Comment);
                return;
            }

            // Block comments.
            if profile.block_comment && starts_with(chars, at, "/*") {
                let start = at;
                let mut depth = 1u8;
                let mut index = at + 2;
                let nested = language == Language::Rust;
                loop {
                    if index >= len {
                        self.state.block_comment = self.state.block_comment.max(depth);
                        emit(out, start, len, SyntaxClass::Comment);
                        return;
                    }
                    if nested && starts_with(chars, index, "/*") {
                        depth = depth.saturating_add(1);
                        index += 2;
                    } else if starts_with(chars, index, "*/") {
                        depth -= 1;
                        index += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        index += 1;
                    }
                }
                emit(out, start, index, SyntaxClass::Comment);
                at = index;
                continue;
            }

            // C preprocessor and C# directives.
            if profile.preprocessor && character == '#' && only_space_before(chars, at) {
                emit(out, at, len, SyntaxClass::Meta);
                return;
            }

            // Strings and characters.
            if character == '"' || character == '\'' || character == '`' {
                if character == '`' && !profile.template_strings {
                    at += 1;
                    continue;
                }
                if character == '\'' && language == Language::Rust && !rust_char_closes(chars, at) {
                    // A lifetime, not a character literal.
                    at += 1;
                    continue;
                }
                let start = at;
                let multiline =
                    (language == Language::Shell) || (character == '`' && profile.template_strings);
                match scan_string(chars, at) {
                    Some(end) => {
                        emit(out, start, end, SyntaxClass::String);
                        at = end;
                    }
                    None => {
                        emit(out, start, len, SyntaxClass::String);
                        if multiline {
                            self.state.quote = Some(character);
                        }
                        return;
                    }
                }
                continue;
            }

            // Dollar variables in shell-like languages.
            if profile.dollar_vars && character == '$' {
                let start = at;
                let mut index = at + 1;
                if chars.get(index) == Some(&'{') {
                    index += 1;
                    while index < len && chars[index] != '}' {
                        index += 1;
                    }
                    if index < len {
                        index += 1;
                    }
                } else {
                    while index < len && (is_ident_continue(chars[index]) || chars[index] == '?') {
                        index += 1;
                    }
                }
                emit(out, start, index, SyntaxClass::Variable);
                at = index;
                continue;
            }

            // Numbers.
            if character.is_ascii_digit()
                || (character == '.' && chars.get(at + 1).is_some_and(|next| next.is_ascii_digit()))
            {
                let end = scan_number(chars, at);
                emit(out, at, end, SyntaxClass::Number);
                at = end;
                continue;
            }

            // Identifiers and keywords.
            if is_ident_start(character) {
                let start = at;
                let mut end = at + 1;
                while end < len && is_ident_continue(chars[end]) {
                    end += 1;
                }
                let word: Vec<char> = chars[start..end].to_vec();
                let word: alloc::string::String = word.into_iter().collect();
                if let Some(class) = classify(&profile, &word, next_non_space(chars, end), language)
                {
                    emit(out, start, end, class);
                }
                at = end;
                continue;
            }

            at += 1;
        }
    }

    fn close_block_comment(
        &mut self,
        chars: &[char],
        from: usize,
        out: &mut Vec<ClassSpan>,
    ) -> usize {
        let len = chars.len();
        let start = from;
        let mut index = from;
        let mut depth = self.state.block_comment;
        let nested = self.language == Language::Rust;
        loop {
            if index >= len {
                self.state.block_comment = depth;
                emit(out, start, len, SyntaxClass::Comment);
                return len;
            }
            if nested && starts_with(chars, index, "/*") {
                depth = depth.saturating_add(1);
                index += 2;
            } else if starts_with(chars, index, "*/") {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    self.state.block_comment = 0;
                    emit(out, start, index, SyntaxClass::Comment);
                    return index;
                }
            } else {
                index += 1;
            }
        }
    }

    fn close_triple(
        &mut self,
        chars: &[char],
        emit_from: usize,
        from: usize,
        quote: char,
        count: u8,
        out: &mut Vec<ClassSpan>,
    ) -> usize {
        let len = chars.len();
        let mut index = from;
        while index < len {
            if chars[index] == '\\' {
                index += 2;
                continue;
            }
            if chars[index] == quote {
                let run = chars[index..].iter().take_while(|c| **c == quote).count();
                if run >= count as usize {
                    index += count as usize;
                    self.state.triple = None;
                    emit(out, emit_from, index, SyntaxClass::String);
                    return index;
                }
                index += run;
                continue;
            }
            index += 1;
        }
        self.state.triple = Some((quote, count));
        emit(out, emit_from, len, SyntaxClass::String);
        len
    }

    fn close_quote(
        &mut self,
        chars: &[char],
        from: usize,
        quote: char,
        out: &mut Vec<ClassSpan>,
    ) -> usize {
        let len = chars.len();
        let mut index = from;
        while index < len {
            if chars[index] == '\\' {
                index += 2;
                continue;
            }
            if chars[index] == quote {
                index += 1;
                self.state.quote = None;
                emit(out, from, index, SyntaxClass::String);
                return index;
            }
            index += 1;
        }
        emit(out, from, len, SyntaxClass::String);
        len
    }

    fn close_raw(
        &mut self,
        chars: &[char],
        from: usize,
        hashes: u8,
        out: &mut Vec<ClassSpan>,
    ) -> usize {
        let len = chars.len();
        match find_raw_end(chars, from, hashes) {
            Some(end) => {
                self.state.raw_hashes = None;
                emit(out, from, end, SyntaxClass::String);
                end
            }
            None => {
                emit(out, from, len, SyntaxClass::String);
                len
            }
        }
    }

    fn scan_shell(&mut self, chars: &[char], out: &mut Vec<ClassSpan>) {
        let len = chars.len();
        let mut at = 0;
        if let Some(quote) = self.state.quote {
            at = self.close_quote(chars, 0, quote, out);
        }
        while at < len {
            let character = chars[at];
            if character.is_whitespace() {
                at += 1;
                continue;
            }
            if character == '#' && (at == 0 || chars[at - 1].is_whitespace()) {
                emit(out, at, len, SyntaxClass::Comment);
                return;
            }
            if character == '\'' || character == '"' {
                let start = at;
                match scan_string(chars, at) {
                    Some(end) => {
                        emit(out, start, end, SyntaxClass::String);
                        at = end;
                    }
                    None => {
                        emit(out, start, len, SyntaxClass::String);
                        self.state.quote = Some(character);
                        return;
                    }
                }
                continue;
            }
            if character == '$' {
                let start = at;
                let mut index = at + 1;
                if chars.get(index) == Some(&'{') {
                    index += 1;
                    while index < len && chars[index] != '}' {
                        index += 1;
                    }
                    if index < len {
                        index += 1;
                    }
                } else if chars.get(index) == Some(&'(') {
                    index += 1;
                    let mut depth = 1usize;
                    while index < len && depth > 0 {
                        match chars[index] {
                            '(' => depth += 1,
                            ')' => depth -= 1,
                            _ => {}
                        }
                        index += 1;
                    }
                } else {
                    while index < len
                        && (is_ident_continue(chars[index])
                            || matches!(chars[index], '?' | '!' | '@' | '*' | '#' | '-' | '$'))
                    {
                        index += 1;
                    }
                }
                emit(out, start, index, SyntaxClass::Variable);
                at = index;
                continue;
            }
            if is_ident_start(character) {
                let start = at;
                let mut end = at + 1;
                while end < len && is_ident_continue(chars[end]) {
                    end += 1;
                }
                let word: alloc::string::String = chars[start..end].iter().collect();
                if SHELL_KEYWORDS.contains(&word.as_str()) {
                    emit(out, start, end, SyntaxClass::Keyword);
                }
                at = end;
                continue;
            }
            if character.is_ascii_digit() {
                let end = scan_number(chars, at);
                emit(out, at, end, SyntaxClass::Number);
                at = end;
                continue;
            }
            at += 1;
        }
    }

    fn scan_markdown(&mut self, chars: &[char], out: &mut Vec<ClassSpan>) {
        let len = chars.len();
        let trimmed_start = chars.iter().position(|c| !c.is_whitespace()).unwrap_or(len);
        let trimmed = &chars[trimmed_start..];

        if let Some((marker, count)) = self.state.md_fence {
            if trimmed.first() == Some(&marker) {
                let run = trimmed.iter().take_while(|c| **c == marker).count();
                if run >= count {
                    emit(out, trimmed_start, len, SyntaxClass::Meta);
                    self.state.md_fence = None;
                    return;
                }
            }
            return;
        }
        if trimmed.len() >= 3 && (trimmed[0] == '`' || trimmed[0] == '~') {
            let marker = trimmed[0];
            let run = trimmed.iter().take_while(|c| **c == marker).count();
            if run >= 3 {
                emit(out, trimmed_start, len, SyntaxClass::Meta);
                self.state.md_fence = Some((marker, run));
                return;
            }
        }
        if trimmed.first() == Some(&'#') {
            let run = trimmed.iter().take_while(|c| **c == '#').count();
            if trimmed.get(run) == Some(&' ') {
                emit(out, trimmed_start, trimmed_start + run, SyntaxClass::Meta);
                let mut text_start = trimmed_start + run;
                while text_start < len && chars[text_start] == ' ' {
                    text_start += 1;
                }
                emit(out, text_start, len, SyntaxClass::Function);
                return;
            }
        }
        if trimmed.first() == Some(&'>') || is_list_marker(trimmed) {
            emit(out, trimmed_start, trimmed_start + 1, SyntaxClass::Meta);
        }

        let mut at = 0;
        while at < len {
            let character = chars[at];
            if character == '\\' && at + 1 < len {
                at += 2;
                continue;
            }
            if character == '`' {
                let start = at;
                let run = chars[at..].iter().take_while(|c| **c == '`').count();
                let mut index = at + run;
                let mut closed = None;
                while index < len {
                    if chars[index] == '`' {
                        let close = chars[index..].iter().take_while(|c| **c == '`').count();
                        if close == run {
                            closed = Some(index + close);
                            break;
                        }
                        index += close;
                    } else {
                        index += 1;
                    }
                }
                match closed {
                    Some(end) => {
                        emit(out, start, end, SyntaxClass::String);
                        at = end;
                    }
                    None => {
                        emit(out, start, len, SyntaxClass::String);
                        return;
                    }
                }
                continue;
            }
            if matches!(character, '*' | '_' | '~') {
                let run = chars[at..].iter().take_while(|c| **c == character).count();
                emit(out, at, at + run, SyntaxClass::Meta);
                at += run;
                continue;
            }
            if matches!(character, '[' | ']' | '(' | ')') {
                emit(out, at, at + 1, SyntaxClass::Meta);
            }
            at += 1;
        }
    }

    fn scan_latex(&mut self, chars: &[char], out: &mut Vec<ClassSpan>) {
        let len = chars.len();
        let mut at = 0;
        while at < len {
            let character = chars[at];
            if character == '%' {
                emit(out, at, len, SyntaxClass::Comment);
                return;
            }
            if character == '$' {
                let start = at;
                let doubled = chars.get(at + 1) == Some(&'$');
                let delimiter = if doubled { "$$" } else { "$" };
                let search_from = at + delimiter.len();
                if let Some(end) = find_from(chars, search_from, delimiter) {
                    emit(out, start, end + delimiter.len(), SyntaxClass::Literal);
                    at = end + delimiter.len();
                } else {
                    emit(out, start, len, SyntaxClass::Literal);
                    return;
                }
                continue;
            }
            if character == '\\' {
                let start = at;
                let mut end = at + 1;
                if chars.get(end) == Some(&'\\') {
                    end += 1;
                } else {
                    while end < len && chars[end].is_alphabetic() {
                        end += 1;
                    }
                }
                emit(out, start, end, SyntaxClass::Keyword);
                if end > start + 1 {
                    let command: String = chars[start + 1..end].iter().collect();
                    if command == "begin" || command == "end" {
                        let mut brace = end;
                        while brace < len && chars[brace].is_whitespace() {
                            brace += 1;
                        }
                        if chars.get(brace) == Some(&'{') {
                            if let Some(close) = find_from(chars, brace + 1, "}") {
                                emit(out, brace + 1, close, SyntaxClass::Type);
                                at = close + 1;
                                continue;
                            }
                        }
                    }
                }
                at = end;
                continue;
            }
            if character.is_ascii_digit() {
                let end = scan_number(chars, at);
                emit(out, at, end, SyntaxClass::Number);
                at = end;
                continue;
            }
            at += 1;
        }
    }

    fn scan_html(&mut self, chars: &[char], out: &mut Vec<ClassSpan>) {
        let len = chars.len();
        let mut at = 0;
        if self.state.html_comment {
            match find_from(chars, 0, "-->") {
                Some(end) => {
                    emit(out, 0, end + 3, SyntaxClass::Comment);
                    self.state.html_comment = false;
                    at = end + 3;
                }
                None => {
                    emit(out, 0, len, SyntaxClass::Comment);
                    return;
                }
            }
        }
        while at < len {
            if starts_with(chars, at, "<!--") {
                match find_from(chars, at + 4, "-->") {
                    Some(end) => {
                        emit(out, at, end + 3, SyntaxClass::Comment);
                        at = end + 3;
                    }
                    None => {
                        emit(out, at, len, SyntaxClass::Comment);
                        self.state.html_comment = true;
                        return;
                    }
                }
                continue;
            }
            if chars[at] == '&' {
                if let Some(end) = find_from(chars, at + 1, ";") {
                    emit(out, at, end + 1, SyntaxClass::Literal);
                    at = end + 1;
                    continue;
                }
            }
            if chars[at] != '<' {
                at += 1;
                continue;
            }
            let start = at;
            let mut index = at + 1;
            let mut prefix = false;
            if matches!(chars.get(index), Some(&'/') | Some(&'!') | Some(&'?')) {
                prefix = true;
                index += 1;
            }
            let name_start = index;
            while index < len
                && (chars[index].is_alphanumeric() || matches!(chars[index], ':' | '-' | '_'))
            {
                index += 1;
            }
            if index == name_start && !prefix {
                at += 1;
                continue;
            }
            emit(out, start, index, SyntaxClass::Keyword);
            // Attributes until the tag closes.
            while index < len && chars[index] != '>' {
                if chars[index].is_whitespace() {
                    index += 1;
                    continue;
                }
                if matches!(chars[index], '"' | '\'') {
                    match scan_string(chars, index) {
                        Some(end) => {
                            emit(out, index, end, SyntaxClass::String);
                            index = end;
                        }
                        None => {
                            emit(out, index, len, SyntaxClass::String);
                            return;
                        }
                    }
                    continue;
                }
                if is_ident_start(chars[index]) || chars[index] == '-' || chars[index] == ':' {
                    let name_start = index;
                    while index < len
                        && (is_ident_continue(chars[index])
                            || matches!(chars[index], '-' | ':' | '.'))
                    {
                        index += 1;
                    }
                    emit(out, name_start, index, SyntaxClass::Variable);
                    continue;
                }
                if chars[index] == '=' {
                    index += 1;
                    continue;
                }
                index += 1;
            }
            if index < len {
                emit(out, index, index + 1, SyntaxClass::Keyword);
                index += 1;
            }
            at = index;
        }
    }

    fn scan_css(&mut self, chars: &[char], out: &mut Vec<ClassSpan>) {
        let len = chars.len();
        let mut at = 0;
        while at < len {
            if starts_with(chars, at, "/*") {
                match find_from(chars, at + 2, "*/") {
                    Some(end) => {
                        emit(out, at, end + 2, SyntaxClass::Comment);
                        at = end + 2;
                    }
                    None => {
                        emit(out, at, len, SyntaxClass::Comment);
                        return;
                    }
                }
                continue;
            }
            if chars[at] == '"' || chars[at] == '\'' {
                match scan_string(chars, at) {
                    Some(end) => {
                        emit(out, at, end, SyntaxClass::String);
                        at = end;
                    }
                    None => {
                        emit(out, at, len, SyntaxClass::String);
                        return;
                    }
                }
                continue;
            }
            if chars[at] == '@' {
                let start = at;
                let mut end = at + 1;
                while end < len && (is_ident_continue(chars[end]) || chars[end] == '-') {
                    end += 1;
                }
                emit(out, start, end, SyntaxClass::Meta);
                at = end;
                continue;
            }
            if chars[at] == '#' {
                let start = at;
                let mut end = at + 1;
                while end < len && chars[end].is_ascii_hexdigit() {
                    end += 1;
                }
                if end > start + 1 {
                    emit(out, start, end, SyntaxClass::Number);
                    at = end;
                    continue;
                }
            }
            if is_ident_start(chars[at]) || chars[at] == '-' {
                let start = at;
                let mut end = at + 1;
                while end < len && (is_ident_continue(chars[end]) || chars[end] == '-') {
                    end += 1;
                }
                let next = next_non_space(chars, end);
                let class = if next == Some(':') {
                    SyntaxClass::Variable
                } else if let Some(found) = next {
                    if found == '{' || found == ';' || found == ',' || found == ')' {
                        SyntaxClass::Keyword
                    } else {
                        SyntaxClass::Literal
                    }
                } else {
                    SyntaxClass::Keyword
                };
                emit(out, start, end, class);
                at = end;
                continue;
            }
            if chars[at].is_ascii_digit() {
                let end = scan_number(chars, at);
                emit(out, at, end, SyntaxClass::Number);
                at = end;
                continue;
            }
            at += 1;
        }
    }

    fn scan_key_value(&mut self, chars: &[char], out: &mut Vec<ClassSpan>) {
        let len = chars.len();
        let mut at = 0;
        let toml = self.language == Language::Toml;
        while at < len && chars[at].is_whitespace() {
            at += 1;
        }
        if at < len && chars[at] == '#' {
            emit(out, at, len, SyntaxClass::Comment);
            return;
        }
        if toml && chars.get(at) == Some(&'[') {
            if let Some(end) = find_from(chars, at + 1, "]") {
                emit(out, at, end + 1, SyntaxClass::Meta);
                at = end + 1;
            }
        }
        if at < len && chars[at] == '-' {
            emit(out, at, at + 1, SyntaxClass::Meta);
            at += 1;
        }
        // A key is the first token before `:` (YAML) or `=` (TOML).
        let key_start = at;
        let mut index = at;
        while index < len {
            let character = chars[index];
            if character == '"' || character == '\'' {
                match scan_string(chars, index) {
                    Some(end) => {
                        index = end;
                        continue;
                    }
                    None => break,
                }
            }
            if (toml && character == '=') || (!toml && character == ':') {
                emit(out, key_start, index, SyntaxClass::Variable);
                at = index + 1;
                break;
            }
            if character == '#' || character == '\n' {
                break;
            }
            index += 1;
        }
        while at < len {
            let character = chars[at];
            if character == '#' {
                emit(out, at, len, SyntaxClass::Comment);
                return;
            }
            if character == '"' || character == '\'' {
                match scan_string(chars, at) {
                    Some(end) => {
                        emit(out, at, end, SyntaxClass::String);
                        at = end;
                    }
                    None => {
                        emit(out, at, len, SyntaxClass::String);
                        return;
                    }
                }
                continue;
            }
            if character.is_ascii_digit()
                || (character == '-' && chars.get(at + 1).is_some_and(|next| next.is_ascii_digit()))
            {
                let end = scan_number(chars, at);
                emit(out, at, end, SyntaxClass::Number);
                at = end;
                continue;
            }
            if is_ident_start(character) {
                let start = at;
                let mut end = at + 1;
                while end < len && is_ident_continue(chars[end]) {
                    end += 1;
                }
                let word: alloc::string::String = chars[start..end].iter().collect();
                let lower = word.to_ascii_lowercase();
                let class =
                    if lower == "true" || lower == "false" || lower == "null" || lower == "~" {
                        Some(SyntaxClass::Literal)
                    } else if chars.get(end) == Some(&':') || chars.get(end) == Some(&'=') {
                        Some(SyntaxClass::Variable)
                    } else {
                        None
                    };
                if let Some(class) = class {
                    emit(out, start, end, class);
                }
                at = end;
                continue;
            }
            at += 1;
        }
    }
}

fn classify(
    profile: &Profile,
    word: &str,
    next: Option<char>,
    language: Language,
) -> Option<SyntaxClass> {
    let lookup = if profile.case_insensitive {
        word.to_ascii_lowercase()
    } else {
        alloc::string::String::from(word)
    };
    let lookup = lookup.as_str();
    if profile.literals.contains(&lookup) {
        return Some(SyntaxClass::Literal);
    }
    if profile.keywords.contains(&lookup) {
        return Some(SyntaxClass::Keyword);
    }
    if profile.types.contains(&lookup) {
        return Some(SyntaxClass::Type);
    }
    if language == Language::Rust && word == "self" {
        return Some(SyntaxClass::Variable);
    }
    if next == Some('(') {
        return Some(SyntaxClass::Function);
    }
    if language == Language::Rust && next == Some('!') {
        return Some(SyntaxClass::Function);
    }
    if profile.uppercase_types
        && word
            .chars()
            .next()
            .is_some_and(|character| character.is_uppercase())
    {
        return Some(SyntaxClass::Type);
    }
    None
}

fn scan_string(chars: &[char], start: usize) -> Option<usize> {
    let quote = *chars.get(start)?;
    let mut index = start + 1;
    while index < chars.len() {
        let character = chars[index];
        if character == '\\' {
            index += 2;
            continue;
        }
        if character == quote {
            return Some(index + 1);
        }
        index += 1;
    }
    None
}

fn scan_number(chars: &[char], start: usize) -> usize {
    let len = chars.len();
    let mut index = start;
    if chars.get(index) == Some(&'0') {
        let radix = chars.get(index + 1).copied();
        if matches!(
            radix,
            Some('x') | Some('X') | Some('b') | Some('B') | Some('o') | Some('O')
        ) {
            index += 2;
            while index < len && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            return index;
        }
    }
    while index < len && (chars[index].is_ascii_digit() || chars[index] == '_') {
        index += 1;
    }
    if chars.get(index) == Some(&'.')
        && chars
            .get(index + 1)
            .is_some_and(|next| next.is_ascii_digit())
    {
        index += 1;
        while index < len && (chars[index].is_ascii_digit() || chars[index] == '_') {
            index += 1;
        }
    }
    if matches!(chars.get(index), Some(&'e') | Some(&'E')) {
        let mut exponent = index + 1;
        if matches!(chars.get(exponent), Some(&'+') | Some(&'-')) {
            exponent += 1;
        }
        if chars
            .get(exponent)
            .is_some_and(|next| next.is_ascii_digit())
        {
            index = exponent;
            while index < len && (chars[index].is_ascii_digit() || chars[index] == '_') {
                index += 1;
            }
        }
    }
    // A numeric suffix (`1u32`, `10f64`) belongs to the number.
    while index < len && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
        index += 1;
    }
    index
}

fn only_space_before(chars: &[char], at: usize) -> bool {
    chars[..at]
        .iter()
        .all(|character| character.is_whitespace())
}

fn is_list_marker(chars: &[char]) -> bool {
    match chars {
        ['-', ' ', ..] | ['*', ' ', ..] | ['+', ' ', ..] => true,
        _ => {
            let mut index = 0;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
            index > 0 && matches!(chars.get(index), Some(&'.') | Some(&')'))
        }
    }
}

fn rust_char_closes(chars: &[char], at: usize) -> bool {
    let mut index = at + 1;
    if chars.get(index) == Some(&'\\') {
        index += 2;
    } else {
        index += 1;
    }
    chars.get(index) == Some(&'\'')
}

fn triple_at(chars: &[char], at: usize) -> Option<u8> {
    let quote = *chars.get(at)?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    if chars.get(at + 1) == Some(&quote) && chars.get(at + 2) == Some(&quote) {
        Some(3)
    } else {
        None
    }
}

/// Return `(opening_quote_index, hash_count)` for a Rust raw string at `at`.
fn rust_raw_at(chars: &[char], at: usize) -> Option<(usize, u8)> {
    let mut index = at;
    if chars.get(index) == Some(&'b') {
        index += 1;
    }
    if chars.get(index) != Some(&'r') {
        return None;
    }
    index += 1;
    let mut hashes = 0u8;
    while chars.get(index) == Some(&'#') {
        hashes = hashes.saturating_add(1);
        index += 1;
    }
    if chars.get(index) == Some(&'"') {
        Some((index, hashes))
    } else {
        None
    }
}

fn find_raw_end(chars: &[char], from: usize, hashes: u8) -> Option<usize> {
    let mut index = from;
    while index < chars.len() {
        if chars[index] == '"' {
            let mut end = index + 1;
            let mut seen = 0u8;
            while seen < hashes && chars.get(end) == Some(&'#') {
                seen += 1;
                end += 1;
            }
            if seen == hashes {
                return Some(end);
            }
        }
        index += 1;
    }
    None
}
