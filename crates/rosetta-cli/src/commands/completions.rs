//! `rosetta completions` subcommand -- generate shell completion scripts.
//!
//! Outputs a completion script for the specified shell to stdout.
//! Users pipe the output to the appropriate file for their shell.
//!
//! # Usage
//!
//! ```sh
//! rosetta completions bash > ~/.bash_completions/rosetta
//! rosetta completions zsh > ~/.zfunc/_forge
//! rosetta completions fish > ~/.config/fish/completions/rosetta.fish
//! ```

use anyhow::Result;

/// Generate and print a shell completion script to stdout.
///
/// Uses `clap_complete::aot::generate` to produce completions for the
/// specified shell based on the CLI's `clap::Command` definition.
pub fn run(shell: clap_complete::aot::Shell, cmd: &mut clap::Command) -> Result<()> {
    clap_complete::aot::generate(shell, cmd, cmd.get_name().to_string(), &mut std::io::stdout());
    Ok(())
}
