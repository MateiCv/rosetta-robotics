//! HDF5 format reader for robomimic and ALOHA datasets.
//!
//! Supports two major HDF5 robotics dataset layouts:
//! - **Robomimic**: Single file with `/data/demo_N/` groups
//! - **ALOHA/ACT**: Root-level `/action`, `/observations/` groups
//!
//! Layout is auto-detected by inspecting the HDF5 group hierarchy.
//! Requires the system HDF5 library (libHDF5) to be installed.

pub mod layout;
pub mod reader;
pub mod writer;

pub use layout::{detect_layout, detect_layout_from_path, HDF5Layout};
pub use reader::{HDF5Detector, HDF5Reader};
pub use writer::HDF5Writer;
