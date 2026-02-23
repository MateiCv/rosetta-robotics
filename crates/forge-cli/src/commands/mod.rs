//! CLI subcommand handlers.
//!
//! Each subcommand lives in its own module with a `pub fn run(...)` entry point.

pub mod convert;
pub mod export_video;
pub mod filter;
pub mod formats;
pub mod inspect;
pub mod quality;
pub mod version;
