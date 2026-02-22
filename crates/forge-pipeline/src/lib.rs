//! Parallel conversion pipeline.
//!
//! Orchestrates format readers and writers with rayon-based parallelism,
//! error continuation, and progress reporting. Coordinates the read-transform-
//! write flow using the canonical [`forge_core::Episode`] type as the
//! intermediate representation.
//!
//! Implementation arrives in Phase 3.
