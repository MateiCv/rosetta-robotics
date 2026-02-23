//! CLI subcommand handlers.
//!
//! Each subcommand lives in its own module with a `pub fn run(...)` entry point.

pub mod completions;
pub mod convert;
pub mod export_video;
pub mod filter;
pub mod formats;
pub mod hub;
pub mod inspect;
pub mod quality;
pub mod version;
pub mod visualize;
