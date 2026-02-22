//! Format readers and writers for robotics dataset formats.
//!
//! Each supported format (LeRobot v3, RLDS, HDF5, Zarr, ROS bags, etc.)
//! is implemented as a reader and/or writer that converts between the
//! format's on-disk representation and the canonical [`forge_core::Episode`]
//! type.
//!
//! Implementation arrives in Phase 2 (LeRobot v3) and Phase 5-6 (remaining formats).
