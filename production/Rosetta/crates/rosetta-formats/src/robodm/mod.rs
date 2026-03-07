//! RoboDM format reader for Berkeley's EBML-based `.vla` files.
//!
//! RoboDM uses a novel EBML container format achieving ~70x compression
//! over RLDS. The format is used by Berkeley's robotics group for their
//! datasets.
//!
//! ## Schema Status
//!
//! The RoboDM EBML schema is not publicly documented. This reader uses
//! best-effort heuristic classification of EBML elements, with unknown
//! elements preserved as opaque metadata.

pub mod reader;
pub mod writer;

pub use reader::{fuzz_classify_element, fuzz_read_vint, RoboDMDetector, RoboDMReader};
pub use writer::RoboDMWriter;
