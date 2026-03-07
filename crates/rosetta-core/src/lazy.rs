//! Lazy-loaded array and image types.
//!
//! These types defer expensive I/O (reading frames from video, loading arrays
//! from Parquet) until first access. They use [`std::sync::OnceLock`] for
//! thread-safe lazy initialization, ensuring `Send + Sync` from day one
//! (required for the rayon parallel pipeline in Phase 3+).
//!
//! The lazy loading pattern uses a `Mutex<Option<Box<dyn FnOnce>>>` to store
//! the loader closure. On first `load()` call, the closure is taken from the
//! mutex, invoked, and the result stored in the `OnceLock`. Subsequent calls
//! read directly from the `OnceLock` without locking.

use std::sync::{Mutex, OnceLock};

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::error::{RosettaError, RosettaResult};
use crate::schema::Dtype;

/// Describes how the image bytes stored in a [`LazyImage`] are encoded.
///
/// Downstream consumers (writers, visualizers) use this to decide whether
/// to decode the bytes or pass them through as-is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageEncoding {
    /// Uncompressed RGB/RGBA pixel bytes.
    Raw,
    /// JPEG-compressed bytes.
    Jpeg,
    /// PNG-compressed bytes.
    Png,
    /// Unknown or opaque compression (pass-through).
    CompressedBytes,
}

/// Boxed loader closure for image data.
type ImageLoader = Box<dyn FnOnce() -> Result<Bytes, RosettaError> + Send>;

/// Boxed loader closure for array data.
type ArrayLoader = Box<dyn FnOnce() -> Result<Vec<f64>, RosettaError> + Send>;

/// A lazily-loaded image frame.
///
/// Wraps raw pixel data as [`Bytes`] with known dimensions. The data is
/// loaded on first access via the provided closure, then cached for
/// subsequent reads.
pub struct LazyImage {
    loader: Mutex<Option<ImageLoader>>,
    cached: OnceLock<Bytes>,
    pub height: u32,
    pub width: u32,
    pub channels: u8,
    /// How the stored bytes are encoded (raw pixels, JPEG, PNG, etc.).
    pub encoding: ImageEncoding,
}

impl LazyImage {
    /// Create a new lazy image with a deferred loader.
    pub fn new(
        loader: impl FnOnce() -> Result<Bytes, RosettaError> + Send + 'static,
        height: u32,
        width: u32,
        channels: u8,
        encoding: ImageEncoding,
    ) -> Self {
        Self {
            loader: Mutex::new(Some(Box::new(loader))),
            cached: OnceLock::new(),
            height,
            width,
            channels,
            encoding,
        }
    }

    /// Create from already-loaded pixel data (no lazy loading needed).
    pub fn from_bytes(data: Bytes, height: u32, width: u32, channels: u8, encoding: ImageEncoding) -> Self {
        let cell = OnceLock::new();
        // Safe to unwrap: cell is freshly created and empty.
        let _ = cell.set(data);
        Self {
            loader: Mutex::new(None),
            cached: cell,
            height,
            width,
            channels,
            encoding,
        }
    }

    /// Load the image data, returning a reference to the cached bytes.
    ///
    /// On first call, invokes the loader closure and caches the result.
    /// Subsequent calls return the cached data without any locking overhead.
    pub fn load(&self) -> RosettaResult<&Bytes> {
        if let Some(data) = self.cached.get() {
            return Ok(data);
        }

        // Take the loader out of the mutex (only one thread can do this).
        let loader = {
            let mut guard = self
                .loader
                .lock()
                .map_err(|e| RosettaError::Video(format!("lock poisoned: {e}")))?;
            guard.take()
        };

        if let Some(loader) = loader {
            let data = loader()?;
            // Another thread may have raced us; OnceLock handles this correctly.
            let _ = self.cached.set(data);
        }

        self.cached
            .get()
            .ok_or_else(|| RosettaError::Video("image data not available".into()))
    }

    /// Returns `(height, width, channels)`.
    pub fn shape(&self) -> (u32, u32, u8) {
        (self.height, self.width, self.channels)
    }

    /// Whether the image data has already been loaded into memory.
    pub fn is_loaded(&self) -> bool {
        self.cached.get().is_some()
    }
}

impl std::fmt::Debug for LazyImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazyImage")
            .field("height", &self.height)
            .field("width", &self.width)
            .field("channels", &self.channels)
            .field("encoding", &self.encoding)
            .field("loaded", &self.is_loaded())
            .finish()
    }
}

/// A lazily-loaded numeric array.
///
/// Stores array data as `Vec<f64>` with known shape and dtype. Like
/// [`LazyImage`], the data is loaded on first access and cached.
pub struct LazyArray {
    loader: Mutex<Option<ArrayLoader>>,
    cached: OnceLock<Vec<f64>>,
    shape: Vec<usize>,
    dtype: Dtype,
}

impl LazyArray {
    /// Create a new lazy array with a deferred loader.
    pub fn new(
        loader: impl FnOnce() -> Result<Vec<f64>, RosettaError> + Send + 'static,
        shape: Vec<usize>,
        dtype: Dtype,
    ) -> Self {
        Self {
            loader: Mutex::new(Some(Box::new(loader))),
            cached: OnceLock::new(),
            shape,
            dtype,
        }
    }

    /// Create from already-loaded data (no lazy loading needed).
    pub fn from_vec(data: Vec<f64>, shape: Vec<usize>, dtype: Dtype) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(data);
        Self {
            loader: Mutex::new(None),
            cached: cell,
            shape,
            dtype,
        }
    }

    /// Load the array data, returning a reference to the cached vector.
    ///
    /// On first call, invokes the loader closure and caches the result.
    /// Subsequent calls return the cached data without any locking overhead.
    pub fn load(&self) -> RosettaResult<&Vec<f64>> {
        if let Some(data) = self.cached.get() {
            return Ok(data);
        }

        let loader = {
            let mut guard = self
                .loader
                .lock()
                .map_err(|e| RosettaError::Config(format!("lock poisoned: {e}")))?;
            guard.take()
        };

        if let Some(loader) = loader {
            let data = loader()?;
            let _ = self.cached.set(data);
        }

        self.cached
            .get()
            .ok_or_else(|| RosettaError::Config("array data not available".into()))
    }

    /// Returns the shape of the array.
    pub fn shape(&self) -> &[usize] {
        &self.shape
    }

    /// Returns the element type of the array.
    pub fn dtype(&self) -> Dtype {
        self.dtype
    }

    /// Whether the array data has already been loaded into memory.
    pub fn is_loaded(&self) -> bool {
        self.cached.get().is_some()
    }
}

impl std::fmt::Debug for LazyArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazyArray")
            .field("shape", &self.shape)
            .field("dtype", &self.dtype)
            .field("loaded", &self.is_loaded())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn lazy_types_are_send_sync() {
        assert_send_sync::<super::LazyImage>();
        assert_send_sync::<super::LazyArray>();
    }
}
