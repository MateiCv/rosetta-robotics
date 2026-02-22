//! SIMD-accelerated kernel functions for quality metric computation.
//!
//! Uses [`pulp`] for runtime CPU feature detection and automatic dispatch
//! to the best available SIMD instruction set (AVX2, SSE4.1, NEON, or scalar).
//!
//! Kernels are implemented in Plan 01 Task 2.
