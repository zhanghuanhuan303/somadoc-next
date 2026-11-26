//! Soma Studio UI. The whole typesetting engine is linked into this wasm
//! module; the Axum server only provides files, assets, fonts and PDF.

pub mod ai;
pub mod api;
pub mod app;
pub mod engine;
pub mod inspect;
pub mod local;
pub mod probe;
pub mod proof;
pub mod selftest;
pub mod shell;
pub mod source;
pub mod state;
pub mod verify;

pub use app::App;
