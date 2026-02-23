//! Interactive TUI dataset viewer.
//!
//! Provides a terminal-based viewer for exploring robotics datasets with
//! camera frame display, action/state timeseries charts, and episode navigation.

pub mod viewer;

use std::io;
use std::path::Path;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

use forge_core::{DatasetInfo, Episode, Frame};

/// Application state for the TUI viewer.
pub struct App {
    /// All episodes loaded from the dataset.
    episodes: Vec<Episode>,
    /// Dataset metadata (retained for future FPS/robot-type display).
    #[allow(dead_code)]
    info: DatasetInfo,
    /// Current episode index.
    current_episode: usize,
    /// Current frame index within the episode.
    current_frame: usize,
    /// Names of cameras to display (from DatasetInfo).
    camera_names: Vec<String>,
    /// Currently selected camera index.
    current_camera: usize,
    /// Whether the app should exit.
    should_quit: bool,
}

impl App {
    /// Create a new App from loaded episodes and dataset info.
    fn new(episodes: Vec<Episode>, info: DatasetInfo) -> Self {
        let camera_names: Vec<String> = info.cameras.iter().map(|c| c.name.clone()).collect();
        Self {
            episodes,
            info,
            current_episode: 0,
            current_frame: 0,
            camera_names,
            current_camera: 0,
            should_quit: false,
        }
    }

    /// Advance to the next episode (wrapping), reset frame to 0.
    fn next_episode(&mut self) {
        if !self.episodes.is_empty() {
            self.current_episode = (self.current_episode + 1) % self.episodes.len();
            self.current_frame = 0;
        }
    }

    /// Go to the previous episode (wrapping), reset frame to 0.
    fn prev_episode(&mut self) {
        if !self.episodes.is_empty() {
            self.current_episode = if self.current_episode == 0 {
                self.episodes.len() - 1
            } else {
                self.current_episode - 1
            };
            self.current_frame = 0;
        }
    }

    /// Advance to the next frame (clamped to episode length - 1).
    fn next_frame(&mut self) {
        let max_frame = self.frame_count().saturating_sub(1);
        if self.current_frame < max_frame {
            self.current_frame += 1;
        }
    }

    /// Go to the previous frame (clamped to 0).
    fn prev_frame(&mut self) {
        self.current_frame = self.current_frame.saturating_sub(1);
    }

    /// Cycle to the next camera.
    fn next_camera(&mut self) {
        if !self.camera_names.is_empty() {
            self.current_camera = (self.current_camera + 1) % self.camera_names.len();
        }
    }

    /// Reference to the current episode.
    pub fn current_episode_ref(&self) -> &Episode {
        &self.episodes[self.current_episode]
    }

    /// Reference to the current frame.
    pub fn current_frame_ref(&self) -> &Frame {
        &self.episodes[self.current_episode].frames[self.current_frame]
    }

    /// Total number of episodes.
    pub fn episode_count(&self) -> usize {
        self.episodes.len()
    }

    /// Number of frames in the current episode.
    pub fn frame_count(&self) -> usize {
        self.episodes[self.current_episode].frames.len()
    }
}

/// Launch the interactive TUI dataset viewer.
///
/// Loads all episodes from the dataset at `path` using the given format reader,
/// sets up the terminal in raw/alternate-screen mode, and runs the event loop.
/// The terminal is always restored on exit, including on panic.
pub fn run_viewer(path: &Path, format_name: &str) -> Result<()> {
    // Get reader and load data.
    let reader = forge_formats::get_reader(format_name)
        .with_context(|| format!("no reader for format '{format_name}'"))?;
    let info = reader
        .inspect(path)
        .with_context(|| format!("failed to inspect {}", path.display()))?;
    let read_output = reader
        .read_episodes(path)
        .with_context(|| format!("failed to read episodes from {}", path.display()))?;

    if read_output.data.is_empty() {
        anyhow::bail!("Dataset has no episodes to visualize");
    }

    eprintln!(
        "Loaded {} episodes from {} ({})",
        read_output.data.len(),
        path.display(),
        format_name
    );
    eprintln!("Starting viewer... Press 'q' to quit, arrows to navigate.");

    let mut app = App::new(read_output.data, info);

    // Setup terminal.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Install panic hook to restore terminal on panic.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    // Event loop.
    let result = run_event_loop(&mut terminal, &mut app);

    // Restore terminal (always, even on error).
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

/// Main event loop: draw the UI and handle keyboard input.
fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|frame| viewer::render_ui(frame, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                KeyCode::Right => app.next_episode(),
                KeyCode::Left => app.prev_episode(),
                KeyCode::Down => app.next_frame(),
                KeyCode::Up => app.prev_frame(),
                KeyCode::Tab => app.next_camera(),
                KeyCode::Char('h') => {} // could show help overlay later
                _ => {}
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
