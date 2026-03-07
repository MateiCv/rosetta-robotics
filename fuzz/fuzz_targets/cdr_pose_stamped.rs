#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = forge_formats::rosbag::parse_cdr_pose_stamped(data);
});
