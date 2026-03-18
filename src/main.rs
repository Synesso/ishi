mod api;
mod app;
mod config;
mod keys;
mod suspend;
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
                app::View::Detail => views::detail::render(frame, area, &mut app),
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
            if app.awaiting_quit {
                app.awaiting_quit = false;
                match key.code {
                    KeyCode::Char('q') => app.running = false,
                    _ => {}
                }
            } else if app.awaiting_sort {
                app.awaiting_sort = false;
                match key.code {
                    KeyCode::Char('i') => app.set_sort(app::SortColumn::Identifier),
                    KeyCode::Char('t') => app.set_sort(app::SortColumn::Title),
                    KeyCode::Char('p') => app.set_sort(app::SortColumn::Project),
                    KeyCode::Char('s') => app.set_sort(app::SortColumn::Status),
                    KeyCode::Char('r') => app.set_sort(app::SortColumn::Priority),
                    _ => {}
                }
            } else if app.awaiting_filter {
                app.awaiting_filter = false;
                match key.code {
                    KeyCode::Char('i') => app.start_column_filter(app::SortColumn::Identifier),
                    KeyCode::Char('t') => app.start_column_filter(app::SortColumn::Title),
                    KeyCode::Char('p') => app.start_column_filter(app::SortColumn::Project),
                    KeyCode::Char('s') => app.start_column_filter(app::SortColumn::Status),
                    KeyCode::Char('r') => app.start_column_filter(app::SortColumn::Priority),
                    _ => {}
                }
            } else if app.searching {
                match key.code {
                    KeyCode::Enter => app.apply_search(),
                    KeyCode::Esc => app.cancel_search(),
                    KeyCode::Backspace => {
                        app.search_input.pop();
                    }
                    KeyCode::Char(c) => {
                        app.search_input.push(c);
                    }
                    _ => {}
                }
            } else if app.filtering {
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
            } else if matches!(app.view, app::View::Detail) {
                if let Some(action) = keys::map_key(key) {
                    match action {
                        keys::Action::Quit => app.awaiting_quit = true,
                        keys::Action::Back => app.back_to_list(),
                        keys::Action::MoveDown => app.scroll_detail_down(),
                        keys::Action::MoveUp => app.scroll_detail_up(),
                        keys::Action::Top => app.detail_scroll = 0,
                        keys::Action::Refresh => app.refresh().await,
                        _ => {}
                    }
                }
            } else if let Some(action) = keys::map_key(key) {
                match action {
                    keys::Action::Quit => app.awaiting_quit = true,
                    keys::Action::MoveDown => app.move_down(),
                    keys::Action::MoveUp => app.move_up(),
                    keys::Action::Top => app.top(),
                    keys::Action::Bottom => app.bottom(),
                    keys::Action::Select => app.select_issue(),
                    keys::Action::Search => app.start_search(),
                    keys::Action::Back => {
                        if app.search.is_some() {
                            app.clear_search();
                        } else {
                            app.clear_filter();
                        }
                    }
                    keys::Action::OrderBy => app.awaiting_sort = true,
                    keys::Action::FilterBy => app.awaiting_filter = true,
                    keys::Action::Refresh => app.refresh().await,
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
