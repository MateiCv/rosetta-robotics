//! PrismaX delivery format: ROS 2 / CDR messages in MCAP, video alongside.
//!
//! One episode is a nested folder -- the `.mcap` carries joint states and
//! camera intrinsics only, and the video sits in a same-named subfolder:
//!
//! ```text
//! episode_000000/
//!   episode_000000.mcap
//!   episode_000000/
//!     cam_high.mp4  cam_left.mp4  cam_right.mp4
//! ```
//!
//! Nothing else may appear at an episode root -- no `meta/`, no parquet, no
//! sidecars. The platform validator rejects stray files outright, which is why
//! the depth backfill re-reads the MCAP rather than leaving state beside it.
//!
//! Port of `verlet_to_prismax`, parity target `worktree-wrist-depth-hybrid`.

pub mod depth;
pub mod schemas;
pub mod writer;

pub use depth::{add_depth, DepthStream};
pub use writer::PrismaxWriter;
