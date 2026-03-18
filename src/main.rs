mod amp;
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
use std::path::Path;
use std::collections::HashSet;

/// Look up the workspace for a thread from the state file, then run
/// `amp threads continue <thread_id>` in that workspace directory,
/// suspending and restoring the TUI around the external process.
fn continue_thread(thread_id: &str) -> Result<()> {
    let state_path = amp::state::state_path()?;
    let state = amp::state::State::load(&state_path)?;
    let workspace = state
        .workspace_for(thread_id)
        .ok_or_else(|| anyhow::anyhow!("no workspace recorded for thread {}", thread_id))?;
    let workspace_path = Path::new(workspace);
    suspend::run_external_command("amp", &["threads", "continue", thread_id], workspace_path)?;
    Ok(())
}

fn load_threads_for_issue(app: &mut App<impl LinearApi>, identifier: &str) {
    let state_path = match amp::state::state_path() {
        Ok(p) => p,
        Err(_) => return,
    };
    let state = match amp::state::State::load(&state_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let thread_ids: Vec<String> = state
        .threads_for_issue(identifier)
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    if thread_ids.is_empty() {
        app.detail_threads.clear();
        return;
    }
    let threads_dir = match amp::thread::amp_threads_dir() {
        Some(d) => d,
        None => return,
    };
    app.detail_threads = amp::thread::load_thread_summaries(&threads_dir, &thread_ids);
    app.detail_thread_selected = 0;
}

/// Open the workspace picker for the current issue.
/// Loads workspace history from state and populates the picker.
fn open_workspace_picker(app: &mut App<impl LinearApi>) {
    let state_path = match amp::state::state_path() {
        Ok(p) => p,
        Err(_) => return,
    };
    let state = match amp::state::State::load(&state_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let workspaces: Vec<String> = state.workspaces().to_vec();
    app.show_workspace_picker(workspaces);
}

/// Execute the new-thread flow:
/// 1. Snapshot thread IDs
/// 2. Suspend TUI and run `amp threads new` in the chosen workspace
/// 3. Diff thread IDs to detect the new thread
/// 4. If found, save thread link and update workspace history
fn start_new_thread(
    issue_identifier: &str,
    workspace: &str,
    before_ids: &HashSet<String>,
) -> Result<()> {
    let workspace_path = Path::new(workspace);
    suspend::run_external_command("amp", &["threads", "new"], workspace_path)?;

    // Diff thread IDs to find the newly created thread
    let threads_dir = match amp::thread::amp_threads_dir() {
        Some(d) => d,
        None => return Ok(()),
    };
    let after_ids = amp::thread::snapshot_thread_ids(&threads_dir);
    let new_ids: Vec<&String> = after_ids.difference(before_ids).collect();

    if let Some(new_thread_id) = new_ids.first() {
        let state_path = amp::state::state_path()?;
        let mut state = amp::state::State::load(&state_path)?;
        state.add_thread_link(new_thread_id, issue_identifier, workspace);
        state.add_workspace(workspace);
        state.save(&state_path)?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = config::resolve_api_key()?;
    let client = LinearClient::new(api_key);
    let mut app = App::new(client);

    // Fetch issues before entering TUI (populates cache)
    app.load_issues().await;

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
                app::View::ProjectList => views::project_list::render(frame, area, &app),
                app::View::ProjectDetail => views::project::render(frame, area, &app),
            }
            if app.show_help {
                views::help::render(frame, area);
            }
        })?;

        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            if app.show_help {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('?') => app.dismiss_help(),
                    _ => {}
                }
            } else if app.error.is_some() {
                match key.code {
                    KeyCode::Esc => app.dismiss_error(),
                    KeyCode::Char('r') => {
                        app.dismiss_error();
                        app.refresh().await;
                    }
                    KeyCode::Char('q') => app.running = false,
                    _ => {}
                }
            } else if app.workspace_picker.is_some() {
                let is_typing = app.workspace_picker.as_ref().is_some_and(|p| p.typing);
                if is_typing {
                    match key.code {
                        KeyCode::Enter => {
                            let typed_path = app
                                .workspace_picker
                                .as_mut()
                                .and_then(|p| p.confirm_typed_path());
                            if let (Some(workspace), Some(issue)) =
                                (typed_path, app.selected_issue())
                            {
                                let issue_id = issue.identifier.clone();
                                app.workspace_picker = None;
                                let before_ids = amp::thread::amp_threads_dir()
                                    .map(|d| amp::thread::snapshot_thread_ids(&d))
                                    .unwrap_or_default();
                                let _ = start_new_thread(&issue_id, &workspace, &before_ids);
                                load_threads_for_issue(&mut app, &issue_id);
                            }
                        }
                        KeyCode::Esc => {
                            if let Some(ref mut picker) = app.workspace_picker {
                                picker.cancel_typing();
                            }
                        }
                        KeyCode::Backspace => {
                            if let Some(ref mut picker) = app.workspace_picker {
                                picker.input.pop();
                            }
                        }
                        KeyCode::Char(c) => {
                            if let Some(ref mut picker) = app.workspace_picker {
                                picker.input.push(c);
                            }
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Enter => {
                            let picker = app.workspace_picker.take();
                            if let (Some(picker), Some(issue)) = (picker, app.selected_issue())
                                && let Some(workspace) = picker.options.get(picker.selected)
                            {
                                let issue_id = issue.identifier.clone();
                                let workspace = workspace.clone();
                                let before_ids = amp::thread::amp_threads_dir()
                                    .map(|d| amp::thread::snapshot_thread_ids(&d))
                                    .unwrap_or_default();
                                let _ = start_new_thread(&issue_id, &workspace, &before_ids);
                                load_threads_for_issue(&mut app, &issue_id);
                            }
                        }
                        KeyCode::Esc => app.cancel_workspace_picker(),
                        KeyCode::Char('/') => {
                            if let Some(ref mut picker) = app.workspace_picker {
                                picker.start_typing();
                            }
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            if let Some(ref mut picker) = app.workspace_picker {
                                picker.move_down();
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            if let Some(ref mut picker) = app.workspace_picker {
                                picker.move_up();
                            }
                        }
                        _ => {}
                    }
                }
            } else if app.awaiting_quit {
                app.awaiting_quit = false;
                match key.code {
                    KeyCode::Char('q') => app.running = false,
                    _ => {}
                }
            } else if app.awaiting_open {
                app.awaiting_open = false;
                match key.code {
                    KeyCode::Char('l') => {
                        if let Some(url) = app.selected_issue_url() {
                            let _ = std::process::Command::new("open")
                                .arg(&url)
                                .spawn();
                        }
                    }
                    KeyCode::Char('g') => {
                        if let Some(issue) = app.selected_issue() {
                            let issue_id = issue.id.clone();
                            if let Ok(Some(url)) = app.api.fetch_pull_request_url(&issue_id).await {
                                let _ = std::process::Command::new("open")
                                    .arg(&url)
                                    .spawn();
                            }
                        }
                    }
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
            } else if matches!(app.view, app::View::ProjectList) {
                if let Some(action) = keys::map_key(key) {
                    match action {
                        keys::Action::Quit => app.awaiting_quit = true,
                        keys::Action::MoveDown => app.project_move_down(),
                        keys::Action::MoveUp => app.project_move_up(),
                        keys::Action::Top => app.project_top(),
                        keys::Action::Bottom => app.project_bottom(),
                        keys::Action::Select => {
                            app.select_project();
                            app.load_project_issues().await;
                        }
                        keys::Action::Back => app.switch_to_my_issues(),
                        keys::Action::Refresh => app.refresh_projects().await,
                        keys::Action::Help => app.toggle_help(),
                        keys::Action::OpenIn => {
                            if let Some(url) = app.selected_project_url() {
                                let _ = std::process::Command::new("open")
                                    .arg(&url)
                                    .spawn();
                            }
                        }
                        _ => {}
                    }
                }
            } else if matches!(app.view, app::View::ProjectDetail) {
                if let Some(action) = keys::map_key(key) {
                    match action {
                        keys::Action::Quit => app.awaiting_quit = true,
                        keys::Action::MoveDown => app.project_issue_move_down(),
                        keys::Action::MoveUp => app.project_issue_move_up(),
                        keys::Action::Top => app.project_issue_top(),
                        keys::Action::Bottom => app.project_issue_bottom(),
                        keys::Action::Select => {
                            let id = app.selected_project_issue().map(|i| i.identifier.clone());
                            app.select_project_issue();
                            if let Some(id) = id {
                                load_threads_for_issue(&mut app, &id);
                            }
                        }
                        keys::Action::Back => app.back_from_project_detail(),
                        keys::Action::Refresh => {
                            app.load_project_issues().await;
                        }
                        keys::Action::Help => app.toggle_help(),
                        keys::Action::OpenIn => app.awaiting_open = true,
                        _ => {}
                    }
                }
            } else if matches!(app.view, app::View::Detail) {
                if let Some(action) = keys::map_key(key) {
                    match app.detail_section {
                        app::DetailSection::Threads => match action {
                            keys::Action::Quit => app.awaiting_quit = true,
                            keys::Action::Back => app.focus_body(),
                            keys::Action::MoveDown => app.thread_move_down(),
                            keys::Action::MoveUp => app.thread_move_up(),
                            keys::Action::Select => {
                                if let Some(thread) = app.selected_thread() {
                                    let thread_id = thread.id.clone();
                                    let _ = continue_thread(&thread_id);
                                }
                            }
                            keys::Action::Tab => app.focus_body(),
                            keys::Action::Refresh => app.refresh().await,
                            keys::Action::NewThread => open_workspace_picker(&mut app),
                            keys::Action::OpenIn => app.awaiting_open = true,
                            keys::Action::Help => app.toggle_help(),
                            _ => {}
                        },
                        app::DetailSection::Body => match action {
                            keys::Action::Quit => app.awaiting_quit = true,
                            keys::Action::Back => app.back_to_list(),
                            keys::Action::MoveDown => app.scroll_detail_down(),
                            keys::Action::MoveUp => app.scroll_detail_up(),
                            keys::Action::Top => app.detail_scroll = 0,
                            keys::Action::Refresh => app.refresh().await,
                            keys::Action::Tab => app.focus_threads(),
                            keys::Action::NewThread => open_workspace_picker(&mut app),
                            keys::Action::OpenIn => app.awaiting_open = true,
                            keys::Action::Help => app.toggle_help(),
                            _ => {}
                        },
                    }
                }
            } else if let Some(action) = keys::map_key(key) {
                match action {
                    keys::Action::Quit => app.awaiting_quit = true,
                    keys::Action::MoveDown => app.move_down(),
                    keys::Action::MoveUp => app.move_up(),
                    keys::Action::Top => app.top(),
                    keys::Action::Bottom => app.bottom(),
                    keys::Action::Select => {
                        let id = app.selected_issue().map(|i| i.identifier.clone());
                        app.select_issue();
                        if let Some(id) = id {
                            load_threads_for_issue(&mut app, &id);
                        }
                    }
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
                    keys::Action::OpenIn => app.awaiting_open = true,
                    keys::Action::Help => app.toggle_help(),
                    keys::Action::Projects => {
                        app.switch_to_projects();
                        app.load_projects().await;
                    }
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
