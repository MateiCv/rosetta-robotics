//! `forge inspect` subcommand -- displays dataset schema, cameras, fields, and metadata.
//!
//! Supports human-readable text output (default), JSON output (`--json`),
//! and YAML config generation (`--generate-config`).

use std::path::Path;

use anyhow::Result;
use forge_core::DatasetInfo;

use crate::output::{self, OutputFormat};

/// Run the `inspect` subcommand.
///
/// Resolves the path, auto-detects format, inspects the dataset, and
/// prints the result in the requested format. Optionally generates a
/// YAML conversion config template.
pub fn run(path: &str, output_format: OutputFormat, generate_config: Option<&Path>) -> Result<()> {
    let (resolved_path, format_name) = crate::output::resolve_and_detect(path)?;

    let reader = forge_formats::get_reader(&format_name)?;
    let info = reader.inspect(&resolved_path)?;

    match output_format {
        OutputFormat::Json => {
            output::print_json(&info)?;
        }
        OutputFormat::Text => {
            print_human_readable(&info, &format_name);
        }
    }

    if let Some(config_path) = generate_config {
        generate_yaml_config(&info, &format_name, config_path)?;
    }

    Ok(())
}

/// Print human-readable dataset summary to stdout.
fn print_human_readable(info: &DatasetInfo, format_name: &str) {
    println!("Dataset: {}", info.path.display());
    println!("Format: {format_name}");
    println!("Episodes: {}", info.total_episodes);
    println!("Total frames: {}", info.total_frames);

    match info.fps {
        Some(fps) => println!("FPS: {fps}"),
        None => println!("FPS: unknown"),
    }

    match &info.robot_type {
        Some(rt) => println!("Robot type: {rt}"),
        None => println!("Robot type: unknown"),
    }

    // Fields
    if !info.fields.is_empty() {
        println!();
        println!("Fields:");
        for field in &info.fields {
            let shape_str = format!("{:?}", field.shape);
            println!("  {:<30} {:<16} {:?}", field.name, shape_str, field.dtype);
        }
    }

    // Cameras
    if !info.cameras.is_empty() {
        println!();
        println!("Cameras:");
        for cam in &info.cameras {
            println!(
                "  {:<30} {}x{}  {}ch  {}",
                cam.name, cam.width, cam.height, cam.channels, cam.encoding
            );
        }
    }

    // Metadata
    if !info.metadata.is_empty() {
        println!();
        println!("Metadata:");
        let mut keys: Vec<&String> = info.metadata.keys().collect();
        keys.sort();
        for key in keys {
            println!("  {}: {}", key, info.metadata[key]);
        }
    }
}

/// Generate a YAML conversion config template from dataset inspection results.
///
/// Builds commented YAML manually (not via serde_yaml) to include helpful
/// comments for users editing the config file.
fn generate_yaml_config(info: &DatasetInfo, format_name: &str, output: &Path) -> Result<()> {
    let mut lines = vec![
        "# Forge conversion configuration".to_string(),
        format!("# Generated from: {}", info.path.display()),
        format!("# Source format: {format_name}"),
        String::new(),
        "target_format: lerobot-v3".to_string(),
        String::new(),
    ];

    // FPS
    match info.fps {
        Some(fps) => lines.push(format!("fps: {fps}")),
        None => lines.push("# fps: 30  # Specify FPS (required if not in source)".to_string()),
    }

    // Robot type
    match &info.robot_type {
        Some(rt) => lines.push(format!("robot_type: {rt}")),
        None => lines.push("# robot_type: unknown  # Specify robot type".to_string()),
    }
    lines.push(String::new());

    // Camera mappings
    if !info.cameras.is_empty() {
        lines.push("cameras:".to_string());
        for cam in &info.cameras {
            lines.push(format!("  {}: {}", cam.name, cam.name));
        }
        lines.push(String::new());
    }

    // Field mappings (commented out as a starting point)
    if !info.fields.is_empty() {
        lines.push("# fields:".to_string());
        for field in &info.fields {
            lines.push(format!("#   {}: {}", field.name, field.name));
        }
        lines.push(String::new());
    }

    // Behavior flags
    lines.push("fail_on_error: false".to_string());
    lines.push("skip_existing: true".to_string());

    std::fs::write(output, lines.join("\n") + "\n")?;
    eprintln!("Config written to: {}", output.display());

    Ok(())
}
