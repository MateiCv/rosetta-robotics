//! `rosetta formats` subcommand -- lists all supported dataset formats.

use anyhow::Result;
use serde::Serialize;

use crate::output::{self, OutputFormat};

/// Serializable representation of a format's capabilities (for JSON output).
#[derive(Serialize)]
struct FormatInfo {
    name: &'static str,
    can_read: bool,
    can_write: bool,
    description: &'static str,
}

/// Run the `formats` subcommand.
///
/// In text mode, prints a fixed-width table matching the original output.
/// In JSON mode, serializes all format capabilities as a JSON array.
pub fn run(output_format: OutputFormat) -> Result<()> {
    let formats = rosetta_formats::all_formats();

    match output_format {
        OutputFormat::Json => {
            let infos: Vec<FormatInfo> = formats
                .iter()
                .map(|fmt| FormatInfo {
                    name: fmt.name,
                    can_read: fmt.can_read,
                    can_write: fmt.can_write,
                    description: fmt.description,
                })
                .collect();
            output::print_json(&infos)?;
        }
        OutputFormat::Text => {
            println!("Supported formats:\n");
            println!(
                "{:<20} {:<8} {:<8} Description",
                "Format", "Read", "Write"
            );
            println!("{}", "-".repeat(65));
            for fmt in formats {
                let read = if fmt.can_read { "yes" } else { "-" };
                let write = if fmt.can_write { "yes" } else { "-" };
                println!(
                    "{:<20} {:<8} {:<8} {}",
                    fmt.name, read, write, fmt.description
                );
            }
        }
    }

    Ok(())
}
