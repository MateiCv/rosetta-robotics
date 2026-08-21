//! End-to-end `verlet` -> `prismax-mcap` conversion against a real episode.
//!
//! Skipped unless `VERLET_TEST_EPISODE` points at a station episode directory,
//! since the fixture is a few hundred MB of real video and can't live in the repo.
//!
//! ```bash
//! VERLET_TEST_EPISODE=/path/to/ep_000000 \
//!   cargo test -p rosetta-formats --test verlet_prismax_e2e -- --nocapture
//! ```

#![cfg(feature = "prismax-format")]

use std::path::PathBuf;

use rosetta_formats::prismax::PrismaxWriter;
use rosetta_formats::verlet::VerletReader;
use rosetta_formats::{FormatReader, FormatWriter};

fn fixture() -> Option<PathBuf> {
    std::env::var("VERLET_TEST_EPISODE").ok().map(PathBuf::from)
}

#[test]
fn converts_a_real_station_episode() {
    let Some(src) = fixture() else {
        eprintln!("skipping: set VERLET_TEST_EPISODE to a station episode directory");
        return;
    };

    // --- detection -------------------------------------------------------
    let detected = rosetta_formats::detect_format(&src).expect("detect format");
    assert_eq!(
        detected, "verlet",
        "a station episode with camera_info must outrank plain lerobot-v2"
    );

    // --- inspect ---------------------------------------------------------
    let reader = VerletReader;
    let info = reader.inspect(&src).expect("inspect");
    println!(
        "episodes={} frames={} fps={:?} cameras={}",
        info.total_episodes,
        info.total_frames,
        info.fps,
        info.cameras.len()
    );
    assert_eq!(info.format, "verlet");
    let calib = info.metadata.get("_calibration").expect("calibration present");
    assert!(
        calib.as_object().is_some_and(|m| !m.is_empty()),
        "station episode must carry camera_info calibration"
    );
    println!(
        "calibrated cameras: {:?}",
        calib.as_object().unwrap().keys().collect::<Vec<_>>()
    );

    // --- read ------------------------------------------------------------
    // skip_images is the production setting for this path: the PrismaX writer
    // ships the recorded mp4s by packet copy, so decoding nine 720p streams to
    // RGB would only burn memory. Without it a multi-episode info.json OOMs.
    let options = rosetta_formats::ReadOptions { skip_images: true };
    let output = reader
        .read_episodes_with_options(&src, &options)
        .expect("read episodes");
    for w in &output.warnings {
        println!("warning: {}", w.message);
    }
    let episodes = output.data;
    assert!(!episodes.is_empty(), "no episodes read");
    let ep = &episodes[0];
    println!("episode {} frames={}", ep.episode_id, ep.frames.len());

    let videos = ep.metadata.get("_source_videos").expect("source videos");
    let videos = videos.as_object().unwrap();
    println!("source streams: {:?}", videos.keys().collect::<Vec<_>>());
    for cam in ["high", "left_wrist", "right_wrist"] {
        assert!(videos.contains_key(cam), "missing colour camera {cam}");
    }
    println!("task: {:?}", ep.metadata.get("_task"));
    println!("gripper limits: {:?}", ep.metadata.get("_gripper_limits"));

    // state/action must be the 14-dim bimanual vectors
    let state = ep.frames[0].state.as_ref().expect("state").load().unwrap();
    let action = ep.frames[0].action.as_ref().expect("action").load().unwrap();
    assert_eq!(state.len(), 14, "state dim");
    assert_eq!(action.len(), 14, "action dim");

    // --- write -----------------------------------------------------------
    let out = tempfile::TempDir::new().unwrap();
    let mut writer = PrismaxWriter::new();
    writer
        .write_episode(ep, out.path(), 0)
        .expect("write prismax episode");
    writer.finalize(out.path(), &info).expect("finalize");

    // --- verify the delivered layout --------------------------------------
    let ep_dir = out.path().join("episode_000000");
    let mcap = ep_dir.join("episode_000000.mcap");
    assert!(mcap.exists(), "mcap missing");
    let mcap_len = std::fs::metadata(&mcap).unwrap().len();
    println!("mcap: {mcap_len} bytes");
    assert!(mcap_len > 1024, "mcap suspiciously small");

    for cam in ["cam_high", "cam_left", "cam_right"] {
        let v = ep_dir.join("episode_000000").join(format!("{cam}.mp4"));
        assert!(v.exists(), "missing {cam}.mp4");
        println!("{cam}.mp4: {} bytes", std::fs::metadata(&v).unwrap().len());
    }

    // Nothing but the mcap and the same-named subfolder may sit at an episode
    // root -- the platform validator rejects stray files.
    let entries: Vec<String> = std::fs::read_dir(&ep_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(entries.len(), 2, "unexpected files at episode root: {entries:?}");

    // --- depth backfill ---------------------------------------------------
    // Synthesise a grayscale disparity video with exactly the episode's frame
    // count; the real one comes from the out-of-process stereo2depth matcher.
    let frames = rosetta_video::frame_count(&ep_dir.join("episode_000000").join("cam_high.mp4"))
        .expect("count frames");
    let depth_src = out.path().join("synthetic_depth.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y", "-loglevel", "error", "-f", "lavfi", "-i",
            &format!("color=c=gray:s=1280x720:r=30"),
            "-frames:v", &frames.to_string(), "-pix_fmt", "yuv420p",
        ])
        .arg(&depth_src)
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "ffmpeg failed");

    let stream = rosetta_formats::prismax::DepthStream {
        camera_key: "high".to_string(),
        video: depth_src,
        fxb: 48.0,
        p: [600.0, 0.0, 320.0, 0.0, 0.0, 600.0, 240.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        backend: "synthetic".to_string(),
    };
    let before = std::fs::metadata(&mcap).unwrap().len();
    rosetta_formats::prismax::add_depth(&ep_dir, &[stream.clone()]).expect("add depth");
    let after = std::fs::metadata(&mcap).unwrap().len();
    println!("mcap {before} -> {after} bytes after depth backfill ({frames} frames)");
    assert!(after > before, "depth channel should grow the mcap");
    assert!(
        ep_dir.join("episode_000000").join("cam_high_depth.mp4").exists(),
        "depth video not placed"
    );

    // Idempotent: a re-run replaces the topic rather than duplicating it.
    rosetta_formats::prismax::add_depth(&ep_dir, &[stream]).expect("re-add depth");
    let twice = std::fs::metadata(&mcap).unwrap().len();
    assert_eq!(after, twice, "re-running the backfill must not duplicate channels");

    // --- the real producer's output --------------------------------------
    // The synthetic stream above proves the muxer; this proves the *contract*.
    // `rosetta-depth` writes `<cam>_depth.mp4` + `<cam>_depth.json` and this is
    // the consumer that has to read them back -- different language, different
    // process, no shared types. Point VERLET_DEPTH_SIDECARS at a directory of
    // them (produced with the same VERLET_TEST_EPISODE and the same frame
    // count) and the whole chain runs end to end:
    //
    //   PYTHONPATH=. python -c 'rosetta_depth.sidecar.produce(...)' -> dir
    //   VERLET_DEPTH_SIDECARS=dir cargo test --test verlet_prismax_e2e
    if let Ok(dir) = std::env::var("VERLET_DEPTH_SIDECARS") {
        let streams =
            rosetta_formats::prismax::sidecars_in(std::path::Path::new(&dir)).expect("read sidecars");
        assert!(!streams.is_empty(), "no *_depth.json in {dir}");
        for s in &streams {
            println!(
                "sidecar: {} backend={} fxb={:.3} fx={:.1}",
                s.camera_key, s.backend, s.fxb, s.p[0]
            );
            assert!(s.fxb > 0.0, "{}: sidecar must carry an invertible fxb", s.camera_key);
        }
        let before = std::fs::metadata(&mcap).unwrap().len();
        rosetta_formats::prismax::add_depth(&ep_dir, &streams).expect("add real depth");
        let after = std::fs::metadata(&mcap).unwrap().len();
        println!("mcap {before} -> {after} bytes after {} real stream(s)", streams.len());
        for s in &streams {
            // `high` -> `cam_high_depth.mp4`, per DEPTH_CAM_SPEC.
            let short = match s.camera_key.as_str() {
                "high" => "cam_high",
                "left_wrist" => "cam_left",
                "right_wrist" => "cam_right",
                other => panic!("unexpected camera_key {other}"),
            };
            let v = ep_dir
                .join("episode_000000")
                .join(format!("{short}_depth.mp4"));
            assert!(v.exists(), "real depth video not placed for {}", s.camera_key);
            assert_eq!(
                rosetta_video::frame_count(&v).unwrap(),
                frames,
                "{}: depth stream must match the episode frame count",
                s.camera_key
            );
            // The synthetic stream above has the same frame count, so the count
            // alone cannot tell whether the real one actually replaced it.
            assert_eq!(
                std::fs::metadata(&v).unwrap().len(),
                std::fs::metadata(&s.video).unwrap().len(),
                "{}: the placed video is not the one the sidecar names",
                s.camera_key
            );
        }
    }

    println!("OUTPUT_DIR={}", out.keep().display());
}
