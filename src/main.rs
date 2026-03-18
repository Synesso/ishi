mod api;
mod app;
mod config;
mod keys;
mod views;

use anyhow::Result;
use app::App;
use crossterm::{
    event::{self, Event},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use std::io::stdout;

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = config::resolve_api_key()?;
    let mut app = App::new(api_key);

    // Terminal setup
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Main loop
    while app.running {
        terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(
                ratatui::widgets::Paragraph::new("ishi — Linear TUI (press q to quit)"),
                area,
            );
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if let Some(action) = keys::map_key(key) {
                    match action {
                        keys::Action::Quit => app.running = false,
                        _ => {}
                    }
                }
            }
        }
    }

    // Teardown
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
