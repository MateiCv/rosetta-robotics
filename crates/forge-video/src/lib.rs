//! Video encoding and decoding via FFmpeg.
//!
//! This crate wraps FFmpeg's libavcodec/libavformat for H.264 video
//! encoding and decoding. It is isolated in its own crate so that
//! FFmpeg build failures do not cascade to the rest of the workspace.
//!
//! Implementation arrives in Phase 2.
