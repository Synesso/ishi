mod api;
mod app;
mod config;
mod keys;
mod views;

use anyhow::Result;
use api::client::{LinearApi, LinearClient};
use app::App;
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use std::io::stdout;

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = config::resolve_api_key()?;
    let client = LinearClient::new(api_key);
    let mut app = App::new(client);

    // Fetch issues before entering TUI
    match app.api.fetch_my_issues().await {
        Ok(issues) => app.issues = issues,
        Err(e) => eprintln!("Failed to fetch issues: {e}"),
    }

    // Terminal setup
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Main loop
    while app.running {
        terminal.draw(|frame| {
            let area = frame.area();
            match app.view {
                app::View::MyIssues => views::my_issues::render(frame, area, &app),
                _ => {
                    frame.render_widget(
                        ratatui::widgets::Paragraph::new("ishi — (view not implemented)"),
                        area,
                    );
                }
            }
        })?;

        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            if app.filtering {
                match key.code {
                    KeyCode::Enter => app.apply_filter(),
                    KeyCode::Esc => app.cancel_filter(),
                    KeyCode::Backspace => {
                        app.filter_input.pop();
                    }
                    KeyCode::Char(c) => {
                        app.filter_input.push(c);
                    }
                    _ => {}
                }
            } else if let Some(action) = keys::map_key(key) {
                match action {
                    keys::Action::Quit => app.running = false,
                    keys::Action::MoveDown => app.move_down(),
                    keys::Action::MoveUp => app.move_up(),
                    keys::Action::Top => app.top(),
                    keys::Action::Bottom => app.bottom(),
                    keys::Action::Search => app.start_filter(),
                    keys::Action::Back => app.clear_filter(),
                    _ => {}
                }
            }
        }
    }

    // Teardown
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
