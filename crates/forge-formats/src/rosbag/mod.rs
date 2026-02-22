//! ROS bag format reader supporting both ROS 1 (.bag) and ROS 2 (MCAP) files.
//!
//! ROS bags are the standard recording format for robots using the Robot
//! Operating System. This module supports:
//! - **ROS 1** legacy `.bag` files via the `rosbag` crate (bzip2/lz4 decompression)
//! - **ROS 2** MCAP files via the `mcap` crate (zstd/lz4 decompression)
//!
//! Both sub-formats are unified behind a single [`RosbagReader`] that
//! implements [`FormatReader`](crate::traits::FormatReader).

pub mod reader;
pub mod ros1;
pub mod ros2;
pub mod writer;

pub use reader::{RosbagDetector, RosbagReader};
pub use ros1::read_ros1_bag;
pub use ros2::read_ros2_mcap;
pub use writer::RosbagWriter;
