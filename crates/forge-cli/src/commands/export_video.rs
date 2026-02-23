//! `forge export-video` subcommand.
//!
//! Exports episodes from a dataset as standalone MP4 video files.
//! Each camera stream in each episode produces one MP4 file.

use std::path::Path;

use anyhow::{Context, Result};
use forge_core::lazy::ImageEncoding;

/// Run the export-video subcommand.
///
/// Reads episodes from `path`, encodes each camera stream as a standalone
/// MP4 file, and writes them to `output_dir`. Supports episode selection
/// (e.g., "0,1,5" or "0-5"), camera filtering, and FPS override.
pub fn run(
    path: &str,
    output_dir: &str,
    episodes: Option<&str>,
    camera: Option<&str>,
    fps: Option<f64>,
) -> Result<()> {
    // Resolve path and auto-detect format (handles hf:// URLs transparently).
    let (resolved_path, format_name) = crate::output::resolve_and_detect(path)?;

    // Get reader and inspect dataset for camera info and FPS.
    let reader = forge_formats::get_reader(&format_name)
        .with_context(|| format!("failed to get reader for format '{format_name}'"))?;
    let info = reader
        .inspect(&resolved_path)
        .with_context(|| format!("failed to inspect dataset at {}", resolved_path.display()))?;

    // Use dataset FPS or CLI override.
    let video_fps = fps.unwrap_or(info.fps.unwrap_or(30.0));

    // Read episodes.
    let read_output = reader
        .read_episodes(&resolved_path)
        .with_context(|| format!("failed to read episodes from {}", resolved_path.display()))?;
    let mut all_episodes = read_output.data;

    // Filter episodes if selection specified.
    if let Some(spec) = episodes {
        let selected = parse_episode_selection(spec)?;
        all_episodes = all_episodes
            .into_iter()
            .enumerate()
            .filter(|(i, _)| selected.contains(i))
            .map(|(_, ep)| ep)
            .collect();
        if all_episodes.is_empty() {
            anyhow::bail!("no episodes matched selection '{spec}'");
        }
    }

    // Create output directory.
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create output directory '{output_dir}'"))?;

    let mut total_count = 0usize;

    for episode in &all_episodes {
        // Determine which cameras to export.
        let camera_names: Vec<String> = if let Some(cam) = camera {
            vec![cam.to_string()]
        } else if let Some(first_frame) = episode.frames.first() {
            let mut names: Vec<String> = first_frame.images.keys().cloned().collect();
            names.sort(); // Deterministic ordering.
            names
        } else {
            eprintln!(
                "Skipping episode '{}': no frames",
                episode.episode_id
            );
            continue;
        };

        for camera_name in &camera_names {
            // Get image dimensions from the first frame's LazyImage.
            let first_frame = match episode.frames.first() {
                Some(f) => f,
                None => continue,
            };
            let lazy_img = match first_frame.images.get(camera_name) {
                Some(img) => img,
                None => {
                    eprintln!(
                        "Skipping camera '{}' in episode '{}': not found in first frame",
                        camera_name, episode.episode_id
                    );
                    continue;
                }
            };

            let width = lazy_img.width;
            let height = lazy_img.height;
            let encoding = lazy_img.encoding;

            // Create output filename: {episode_id}_{camera_name}.mp4
            let output_filename = format!("{}_{}.mp4", episode.episode_id, camera_name);
            let output_path = Path::new(output_dir).join(&output_filename);

            // Create encoder.
            let config = forge_video::VideoEncoderConfig::new(width, height, video_fps);
            let mut encoder = forge_video::H264VideoEncoder::new(&output_path, &config)
                .with_context(|| {
                    format!("failed to create encoder for {}", output_path.display())
                })?;

            // Encode each frame.
            for frame in &episode.frames {
                let img = match frame.images.get(camera_name) {
                    Some(img) => img,
                    None => continue,
                };

                let bytes = img
                    .load()
                    .with_context(|| {
                        format!(
                            "failed to load image for camera '{}' at timestep {}",
                            camera_name, frame.timestep
                        )
                    })?;

                // Handle image encoding: decode to raw RGB if needed.
                let rgb_data: Vec<u8> = match encoding {
                    ImageEncoding::Raw if img.channels == 3 => bytes.to_vec(),
                    ImageEncoding::Jpeg | ImageEncoding::Png => {
                        let decoded = image::load_from_memory(bytes)
                            .with_context(|| {
                                format!(
                                    "failed to decode {:?} image at timestep {}",
                                    encoding, frame.timestep
                                )
                            })?
                            .to_rgb8();
                        decoded.into_raw()
                    }
                    _ => {
                        // For Raw with channels != 3, or CompressedBytes, try decoding via image crate.
                        let decoded = image::load_from_memory(bytes)
                            .with_context(|| {
                                format!(
                                    "failed to decode image with encoding {:?} at timestep {}",
                                    encoding, frame.timestep
                                )
                            })?
                            .to_rgb8();
                        decoded.into_raw()
                    }
                };

                encoder.encode_frame(&rgb_data).with_context(|| {
                    format!(
                        "failed to encode frame at timestep {} for camera '{}'",
                        frame.timestep, camera_name
                    )
                })?;
            }

            // Finalize: flush buffered frames, write trailer, get frame count.
            let count = encoder.finish().with_context(|| {
                format!(
                    "failed to finalize video for camera '{}' in episode '{}'",
                    camera_name, episode.episode_id
                )
            })?;

            eprintln!("Exported: {} ({} frames)", output_filename, count);
            total_count += 1;
        }
    }

    eprintln!("Exported {} videos to {}", total_count, output_dir);
    Ok(())
}

/// Parse an episode selection specification.
///
/// Supports comma-separated indices and ranges:
/// - "0,1,5" -> [0, 1, 5]
/// - "0-5" -> [0, 1, 2, 3, 4, 5]
/// - "0,3-5,9" -> [0, 3, 4, 5, 9]
pub fn parse_episode_selection(spec: &str) -> Result<Vec<usize>> {
    let mut indices = Vec::new();

    for part in spec.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let bounds: Vec<&str> = part.splitn(2, '-').collect();
            if bounds.len() != 2 {
                anyhow::bail!("invalid range in episode selection: '{part}'");
            }
            let start: usize = bounds[0]
                .trim()
                .parse()
                .with_context(|| format!("invalid start in range '{part}'"))?;
            let end: usize = bounds[1]
                .trim()
                .parse()
                .with_context(|| format!("invalid end in range '{part}'"))?;
            if start > end {
                anyhow::bail!("invalid range: start ({start}) > end ({end}) in '{part}'");
            }
            for i in start..=end {
                indices.push(i);
            }
        } else {
            let idx: usize = part
                .parse()
                .with_context(|| format!("invalid episode index: '{part}'"))?;
            indices.push(idx);
        }
    }

    if indices.is_empty() {
        anyhow::bail!("empty episode selection");
    }

    Ok(indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_index() {
        assert_eq!(parse_episode_selection("0").unwrap(), vec![0]);
        assert_eq!(parse_episode_selection("5").unwrap(), vec![5]);
    }

    #[test]
    fn parse_comma_separated() {
        assert_eq!(parse_episode_selection("0,1,5").unwrap(), vec![0, 1, 5]);
    }

    #[test]
    fn parse_range() {
        assert_eq!(
            parse_episode_selection("0-5").unwrap(),
            vec![0, 1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn parse_mixed() {
        assert_eq!(
            parse_episode_selection("0,3-5,9").unwrap(),
            vec![0, 3, 4, 5, 9]
        );
    }

    #[test]
    fn parse_invalid_range() {
        assert!(parse_episode_selection("5-2").is_err());
    }

    #[test]
    fn parse_invalid_index() {
        assert!(parse_episode_selection("abc").is_err());
    }

    #[test]
    fn parse_empty() {
        assert!(parse_episode_selection("").is_err());
    }
}
