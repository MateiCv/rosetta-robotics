//! RLDS/TFRecord format reader for Open-X Embodiment datasets.
//!
//! Reads RLDS datasets stored as TFRecord files with protobuf-encoded
//! `tf.train.Example` messages. Eliminates the TensorFlow dependency
//! by using the `tfrecord` crate for binary parsing and `prost` for
//! protobuf decoding.
//!
//! Supports:
//! - Raw and GZIP/ZLIB-compressed `.tfrecord` shard files
//! - Episode boundary detection from `is_first`/`is_last` markers
//! - Preservation of all RLDS features (images, actions, rewards, language)
//! - TFDS `dataset_info.json` metadata parsing

pub mod reader;
pub mod tfrecord;

pub use reader::{RLDSDetector, RLDSReader};
pub use self::tfrecord::{decode_example, open_tfrecord_file, FeatureValue};
