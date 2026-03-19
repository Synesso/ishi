mod amp;
mod api;
mod app;
mod config;
mod keys;
mod suspend;
mod views;

use anyhow::{Context, Result};
use api::client::{LinearApi, LinearClient};
use app::App;
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, prelude::CrosstermBackend};
use std::collections::HashSet;
use std::io::stdout;
use std::path::Path;
use tokio::sync::mpsc;

enum ThreadRunModeAction {
    Foreground {
        thread_id: String,
    },
    Background {
        thread_id: String,
        issue: api::types::Issue,
    },
}

enum RunManagementAction {
    OpenLog {
        log_path: String,
    },
    Retry {
        thread_id: String,
        issue: api::types::Issue,
    },
    MarkStale {
        run_id: String,
    },
}

fn resolve_thread_run_mode_action(
    app: &App<impl LinearApi>,
    key_code: KeyCode,
) -> Option<ThreadRunModeAction> {
    match key_code {
        KeyCode::Char('f') => app
            .selected_thread()
            .map(|thread| ThreadRunModeAction::Foreground {
                thread_id: thread.id.clone(),
            }),
        KeyCode::Char('b') => {
            let thread = app.selected_thread()?;
            let issue = app.selected_issue()?;
            Some(ThreadRunModeAction::Background {
                thread_id: thread.id.clone(),
                issue: issue.clone(),
            })
        }
        _ => None,
    }
}

fn resolve_run_management_action(
    app: &App<impl LinearApi>,
    action: keys::Action,
) -> Option<RunManagementAction> {
    match action {
        keys::Action::OpenRunLog => {
            let run = app.selected_thread_run()?;
            let log_path = run.log_path.clone()?;
            Some(RunManagementAction::OpenLog { log_path })
        }
        keys::Action::RetryRun => {
            // Retry is intentionally tied to an existing run for the selected thread.
            app.selected_thread_run()?;
            let thread = app.selected_thread()?;
            let issue = app.selected_issue()?;
            Some(RunManagementAction::Retry {
                thread_id: thread.id.clone(),
                issue: issue.clone(),
            })
        }
        keys::Action::MarkRunStale => {
            let run = app.selected_thread_run()?;
            Some(RunManagementAction::MarkStale {
                run_id: run.run_id.clone(),
            })
        }
        _ => None,
    }
}

fn mark_session_run_stale(run_id: &str) -> Result<bool> {
    let state_path = amp::state::state_path()?;
    let mut state = amp::state::State::load(&state_path)?;
    let changed = state.mark_session_run_stale(run_id, now_ms());
    if changed {
        state.save(&state_path)?;
    }
    Ok(changed)
}

async fn execute_run_management_action(
    app: &mut App<impl LinearApi>,
    action: RunManagementAction,
) -> Result<()> {
    match action {
        RunManagementAction::OpenLog { log_path } => {
            std::process::Command::new("open")
                .arg(&log_path)
                .spawn()
                .with_context(|| format!("failed to open run log at {log_path}"))?;
        }
        RunManagementAction::Retry { thread_id, issue } => {
            launch_background_thread_run(&issue, &thread_id)?;
            refresh_all(app).await;
        }
        RunManagementAction::MarkStale { run_id } => {
            if mark_session_run_stale(&run_id)? {
                refresh_all(app).await;
            }
        }
    }
    Ok(())
}

fn reconcile_session_runs() -> Result<usize> {
    let state_path = amp::state::state_path()?;
    amp::reconcile::reconcile_state_file(&state_path)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Perform a full refresh and reload thread data for the selected issue.
async fn refresh_all(app: &mut App<impl LinearApi>) {
    if let Err(err) = reconcile_session_runs() {
        app.error = Some(app::AppError::new(format!(
            "Failed to reconcile background runs: {err}"
        )));
        return;
    }

    if let Some(id) = app.refresh().await {
        load_threads_for_issue(app, &id);
    }
}

/// Build the default non-interactive prompt used for background thread runs.
fn build_continue_prompt(issue: &api::types::Issue) -> String {
    format!(
        "Continue work on this Linear issue in the existing thread.\n\n{}",
        build_issue_context(issue)
    )
}

/// Launch a background run for an existing thread and persist its lifecycle
/// metadata to state.
fn launch_background_thread_run(issue: &api::types::Issue, thread_id: &str) -> Result<()> {
    let state_path = amp::state::state_path()?;
    let mut state = amp::state::State::load(&state_path)?;
    let workspace = state
        .workspace_for(thread_id)
        .ok_or_else(|| anyhow::anyhow!("no workspace recorded for thread {}", thread_id))?;
    let workspace_path = Path::new(workspace);

    let prompt = build_continue_prompt(issue);
    let launched = amp::run::launch_thread_continue_background(
        thread_id,
        &issue.identifier,
        workspace_path,
        &prompt,
    )?;

    state.add_session_run(&launched.run_id, launched.run);
    state.save(&state_path)?;
    Ok(())
}

/// Continue an existing thread in the foreground (interactive) path.
fn continue_thread_in_foreground(thread_id: &str) -> Result<()> {
    let state_path = amp::state::state_path()?;
    let state = amp::state::State::load(&state_path)?;
    let workspace = state
        .workspace_for(thread_id)
        .ok_or_else(|| anyhow::anyhow!("no workspace recorded for thread {}", thread_id))?;
    let workspace_path = Path::new(workspace);
    let args = ["threads", "continue", thread_id];
    let _ = suspend::run_external_command("amp", &args, workspace_path)?;
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

    // Load session run summaries for this issue
    app.detail_session_runs = state
        .runs_for_issue(identifier)
        .into_iter()
        .map(|(run_id, run)| app::SessionRunSummary {
            run_id: run_id.to_string(),
            thread_id: run.thread_id.clone(),
            status: run.status,
            log_path: run.log_path.clone(),
            created_at_ms: run.created_at_ms,
        })
        .collect();

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
    let mut workspaces: Vec<String> = state.workspaces().to_vec();
    if let Ok(pwd) = std::env::current_dir() {
        let pwd = pwd.to_string_lossy().to_string();
        workspaces.retain(|w| w != &pwd);
        workspaces.insert(0, pwd);
    }
    app.show_workspace_picker(workspaces);
}

/// Build a context prompt from a Linear issue to seed a new Amp thread.
fn build_issue_context(issue: &api::types::Issue) -> String {
    let mut ctx = format!("# {} {}\n", issue.identifier, issue.title);
    if let Some(url) = &issue.url {
        ctx.push_str(&format!("{}\n", url));
    }
    ctx.push('\n');
    if let Some(state) = &issue.state {
        ctx.push_str(&format!("**Status**: {}\n", state.name));
    }
    if let Some(project) = &issue.project {
        ctx.push_str(&format!("**Project**: {}\n", project.name));
    }
    if let Some(labels) = &issue.labels {
        if !labels.nodes.is_empty() {
            let names: Vec<&str> = labels.nodes.iter().map(|l| l.name.as_str()).collect();
            ctx.push_str(&format!("**Labels**: {}\n", names.join(", ")));
        }
    }
    if let Some(desc) = &issue.description {
        if !desc.is_empty() {
            ctx.push_str(&format!("\n## Description\n\n{}\n", desc));
        }
    }
    ctx
}

/// Execute the new-thread flow:
/// 1. Snapshot thread IDs
/// 2. Suspend TUI, open `$EDITOR` with pre-filled issue context for the user to
///    add instructions, then launch `amp` piping the edited context
/// 3. Diff thread IDs to detect the new thread
/// 4. If found, save thread link and update workspace history
fn start_new_thread(
    issue_identifier: &str,
    workspace: &str,
    before_ids: &HashSet<String>,
    issue_context: Option<&str>,
) -> Result<()> {
    let workspace_path = Path::new(workspace);

    suspend::run_with_editor_then_command(workspace_path, issue_context)?;

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
    if let Err(err) = reconcile_session_runs() {
        app.error = Some(app::AppError::new(format!(
            "Failed to reconcile background runs: {err}"
        )));
    }

    // Channel for background tasks to report errors back to the UI.
    let (bg_error_tx, mut bg_error_rx) = mpsc::unbounded_channel::<String>();

    // Terminal setup
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Main loop
    while app.running {
        // Check for errors from background tasks.
        while let Ok(msg) = bg_error_rx.try_recv() {
            app.error = Some(app::AppError::new(msg));
        }
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
                        refresh_all(&mut app).await;
                    }
                    KeyCode::Char('q') => app.running = false,
                    _ => {}
                }
            } else if app.workspace_picker.is_some() {
                let is_typing = app.workspace_picker.as_ref().is_some_and(|p| p.typing);
                if is_typing {
                    match key.code {
                        KeyCode::Enter => {
                            // If the input is empty, adopt the selected option
                            // as the input path before confirming.
                            if let Some(ref mut picker) = app.workspace_picker {
                                if picker.input.is_empty() {
                                    if let Some(ws) = picker.options.get(picker.selected) {
                                        picker.input = ws.clone();
                                    }
                                }
                            }
                            let typed_path = app
                                .workspace_picker
                                .as_mut()
                                .and_then(|p| p.confirm_typed_path());
                            if let (Some(workspace), Some(issue)) =
                                (typed_path, app.selected_issue())
                            {
                                let issue_id = issue.identifier.clone();
                                let context = build_issue_context(issue);
                                app.workspace_picker = None;
                                let before_ids = amp::thread::amp_threads_dir()
                                    .map(|d| amp::thread::snapshot_thread_ids(&d))
                                    .unwrap_or_default();
                                if let Err(err) = start_new_thread(
                                    &issue_id,
                                    &workspace,
                                    &before_ids,
                                    Some(&context),
                                ) {
                                    app.error = Some(app::AppError::new(format!(
                                        "Failed to start thread: {err}"
                                    )));
                                } else {
                                    terminal.clear()?;
                                    refresh_all(&mut app).await;
                                }
                            }
                        }
                        KeyCode::Esc => {
                            if let Some(ref mut picker) = app.workspace_picker {
                                picker.cancel_typing();
                            }
                        }
                        KeyCode::Tab | KeyCode::BackTab => {
                            if let Some(ref mut picker) = app.workspace_picker {
                                picker.tab_complete();
                            }
                        }
                        KeyCode::Down => {
                            if let Some(ref mut picker) = app.workspace_picker {
                                picker.move_down();
                            }
                        }
                        KeyCode::Up => {
                            if let Some(ref mut picker) = app.workspace_picker {
                                picker.move_up();
                            }
                        }
                        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::ALT) => {
                            if let Some(ref mut picker) = app.workspace_picker {
                                picker.delete_path_component();
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
                                let context = build_issue_context(issue);
                                let workspace = workspace.clone();
                                let before_ids = amp::thread::amp_threads_dir()
                                    .map(|d| amp::thread::snapshot_thread_ids(&d))
                                    .unwrap_or_default();
                                if let Err(err) = start_new_thread(
                                    &issue_id,
                                    &workspace,
                                    &before_ids,
                                    Some(&context),
                                ) {
                                    app.error = Some(app::AppError::new(format!(
                                        "Failed to start thread: {err}"
                                    )));
                                } else {
                                    terminal.clear()?;
                                    refresh_all(&mut app).await;
                                }
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
                            let _ = std::process::Command::new("open").arg(&url).spawn();
                        }
                    }
                    KeyCode::Char('g') => {
                        if let Some(issue) = app.selected_issue() {
                            let issue_id = issue.id.clone();
                            if let Ok(Some(url)) = app.api.fetch_pull_request_url(&issue_id).await {
                                let _ = std::process::Command::new("open").arg(&url).spawn();
                            }
                        }
                    }
                    _ => {}
                }
            } else if app.awaiting_thread_run_mode {
                app.awaiting_thread_run_mode = false;
                match resolve_thread_run_mode_action(&app, key.code) {
                    Some(ThreadRunModeAction::Foreground { thread_id }) => {
                        if let Err(err) = continue_thread_in_foreground(&thread_id) {
                            app.error = Some(app::AppError::new(format!(
                                "Failed to run thread in foreground: {err}"
                            )));
                        } else {
                            terminal.clear()?;
                            refresh_all(&mut app).await;
                        }
                    }
                    Some(ThreadRunModeAction::Background { thread_id, issue }) => {
                        if let Err(err) = launch_background_thread_run(&issue, &thread_id) {
                            app.error = Some(app::AppError::new(format!(
                                "Failed to launch background run: {err}"
                            )));
                        }
                    }
                    None => {}
                }
            } else if app.awaiting_state_change {
                match key.code {
                    KeyCode::Enter => {
                        let state_name = app.selected_state_option().map(|s| s.to_string());
                        let issue = app
                            .context_issue()
                            .map(|i| (i.id.clone(), i.identifier.clone()));
                        app.cancel_state_change();
                        if let (Some(state_name), Some((issue_id, identifier))) =
                            (state_name, issue)
                        {
                            // Optimistic update: apply locally first for instant feedback.
                            app.apply_local_state_change(&state_name);
                            // Fire-and-forget: spawn the API call in the background.
                            let api = app.api.clone();
                            let tx = bg_error_tx.clone();
                            tokio::spawn(async move {
                                if let Err(err) =
                                    api.update_issue_state(&issue_id, &state_name).await
                                {
                                    let _ = tx.send(format!(
                                        "Failed to update state for {}: {}",
                                        identifier, err
                                    ));
                                }
                            });
                        }
                    }
                    KeyCode::Esc => app.cancel_state_change(),
                    KeyCode::Char('j') | KeyCode::Down | KeyCode::Right => {
                        app.state_change_move_down()
                    }
                    KeyCode::Char('k') | KeyCode::Up | KeyCode::Left => app.state_change_move_up(),
                    _ => {}
                }
            } else if app.awaiting_sort {
                app.awaiting_sort = false;
                if matches!(app.view, app::View::ProjectList) {
                    match key.code {
                        KeyCode::Char('n') => app.set_project_sort(app::ProjectSortColumn::Name),
                        KeyCode::Char('s') => app.set_project_sort(app::ProjectSortColumn::Status),
                        KeyCode::Char('l') => app.set_project_sort(app::ProjectSortColumn::Lead),
                        KeyCode::Char('p') => app.set_project_sort(app::ProjectSortColumn::Progress),
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('i') => app.set_sort(app::SortColumn::Identifier),
                        KeyCode::Char('t') => app.set_sort(app::SortColumn::Title),
                        KeyCode::Char('p') => app.set_sort(app::SortColumn::Project),
                        KeyCode::Char('s') => app.set_sort(app::SortColumn::Status),
                        KeyCode::Char('r') => app.set_sort(app::SortColumn::Priority),
                        _ => {}
                    }
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
                        keys::Action::OrderBy => app.awaiting_sort = true,
                        keys::Action::OpenIn => {
                            if let Some(url) = app.selected_project_url() {
                                let _ = std::process::Command::new("open").arg(&url).spawn();
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
                        keys::Action::ChangeState => {
                            if app.context_issue().is_some() {
                                app.start_state_change();
                            }
                        }
                        _ => {}
                    }
                }
            } else if matches!(app.view, app::View::Detail) {
                if let Some(action) = keys::map_key(key) {
                    match app.detail_section {
                        app::DetailSection::Output => match action {
                            keys::Action::Quit => app.awaiting_quit = true,
                            keys::Action::Back => {
                                app.detail_section = app::DetailSection::Threads;
                                app.detail_output_scroll = 0;
                            }
                            keys::Action::MoveDown => app.scroll_output_down(),
                            keys::Action::MoveUp => app.scroll_output_up(),
                            keys::Action::Top => app.detail_output_scroll = 0,
                            keys::Action::Bottom => app.scroll_output_to_bottom(),
                            keys::Action::Help => app.toggle_help(),
                            _ => {}
                        },
                        app::DetailSection::Threads => match action {
                            keys::Action::Quit => app.awaiting_quit = true,
                            keys::Action::Back => app.focus_body(),
                            keys::Action::MoveDown => app.thread_move_down(),
                            keys::Action::MoveUp => app.thread_move_up(),
                            keys::Action::Select => {
                                if app.selected_thread().is_some() {
                                    app.awaiting_thread_run_mode = true;
                                }
                            }
                            keys::Action::Tab => app.focus_body(),
                            keys::Action::Refresh => refresh_all(&mut app).await,
                            keys::Action::NewThread => open_workspace_picker(&mut app),
                            keys::Action::OpenIn => {
                                // In threads context, 'o' opens output view if output exists
                                if app
                                    .selected_thread()
                                    .is_some_and(|t| app.output_buffer.line_count(&t.id) > 0)
                                {
                                    app.focus_output();
                                } else {
                                    app.awaiting_open = true;
                                }
                            }
                            keys::Action::OpenRunLog => {
                                if let Some(action) =
                                    resolve_run_management_action(&app, keys::Action::OpenRunLog)
                                    && let Err(err) =
                                        execute_run_management_action(&mut app, action).await
                                {
                                    app.error = Some(app::AppError::new(format!(
                                        "Failed to open run log: {err}"
                                    )));
                                }
                            }
                            keys::Action::RetryRun => {
                                if let Some(action) =
                                    resolve_run_management_action(&app, keys::Action::RetryRun)
                                    && let Err(err) =
                                        execute_run_management_action(&mut app, action).await
                                {
                                    app.error = Some(app::AppError::new(format!(
                                        "Failed to retry run: {err}"
                                    )));
                                }
                            }
                            keys::Action::MarkRunStale => {
                                if let Some(action) =
                                    resolve_run_management_action(&app, keys::Action::MarkRunStale)
                                    && let Err(err) =
                                        execute_run_management_action(&mut app, action).await
                                {
                                    app.error = Some(app::AppError::new(format!(
                                        "Failed to mark run stale: {err}"
                                    )));
                                }
                            }
                            keys::Action::Help => app.toggle_help(),
                            _ => {}
                        },
                        app::DetailSection::Body => match action {
                            keys::Action::Quit => app.awaiting_quit = true,
                            keys::Action::Back => app.back_to_list(),
                            keys::Action::MoveDown => app.scroll_detail_down(),
                            keys::Action::MoveUp => app.scroll_detail_up(),
                            keys::Action::Top => app.detail_scroll = 0,
                            keys::Action::Refresh => refresh_all(&mut app).await,
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
                    keys::Action::Refresh => refresh_all(&mut app).await,
                    keys::Action::OpenIn => app.awaiting_open = true,
                    keys::Action::Help => app.toggle_help(),
                    keys::Action::Projects => {
                        app.switch_to_projects();
                        app.load_projects().await;
                    }
                    keys::Action::ChangeState => {
                        if app.context_issue().is_some() {
                            app.start_state_change();
                        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amp::state::SessionRunStatus;
    use crate::amp::thread::ThreadSummary;
    use crate::api::fake::FakeLinearApi;
    use crate::api::types::Issue;
    use crate::app::DetailSection;

    fn app_with_issue_and_thread() -> App<FakeLinearApi> {
        let mut app = App::new(FakeLinearApi::new());
        app.issues = vec![Issue {
            id: "1".to_string(),
            identifier: "JEM-1".to_string(),
            title: "Test issue".to_string(),
            url: None,
            state: None,
            priority: None,
            project: None,
            description: None,
            assignee: None,
            labels: None,
            comments: None,
        }];
        app.detail_section = DetailSection::Threads;
        app.detail_threads = vec![ThreadSummary {
            id: "T-abc".to_string(),
            title: "Test thread".to_string(),
            message_count: 1,
            last_activity_ms: 0,
        }];
        app.detail_session_runs = vec![app::SessionRunSummary {
            run_id: "run-1".to_string(),
            thread_id: "T-abc".to_string(),
            status: SessionRunStatus::Failed,
            log_path: Some("/tmp/run-1.log".to_string()),
            created_at_ms: 100,
        }];
        app
    }

    #[test]
    fn resolves_foreground_action_for_selected_thread() {
        let app = app_with_issue_and_thread();

        let action = resolve_thread_run_mode_action(&app, KeyCode::Char('f'));

        match action {
            Some(ThreadRunModeAction::Foreground { thread_id }) => {
                assert_eq!(thread_id, "T-abc");
            }
            _ => panic!("expected foreground action"),
        }
    }

    #[test]
    fn resolves_background_action_for_selected_thread_and_issue() {
        let app = app_with_issue_and_thread();

        let action = resolve_thread_run_mode_action(&app, KeyCode::Char('b'));

        match action {
            Some(ThreadRunModeAction::Background { thread_id, issue }) => {
                assert_eq!(thread_id, "T-abc");
                assert_eq!(issue.identifier, "JEM-1");
            }
            _ => panic!("expected background action"),
        }
    }

    #[test]
    fn background_action_requires_selected_issue() {
        let mut app = app_with_issue_and_thread();
        app.issues.clear();

        let action = resolve_thread_run_mode_action(&app, KeyCode::Char('b'));

        assert!(action.is_none());
    }

    #[test]
    fn foreground_action_requires_selected_thread() {
        let mut app = app_with_issue_and_thread();
        app.detail_threads.clear();

        let action = resolve_thread_run_mode_action(&app, KeyCode::Char('f'));

        assert!(action.is_none());
    }

    #[test]
    fn non_run_mode_key_returns_no_action() {
        let app = app_with_issue_and_thread();

        let action = resolve_thread_run_mode_action(&app, KeyCode::Enter);

        assert!(action.is_none());
    }

    #[test]
    fn resolves_open_log_management_action() {
        let app = app_with_issue_and_thread();

        let action = resolve_run_management_action(&app, keys::Action::OpenRunLog);

        match action {
            Some(RunManagementAction::OpenLog { log_path }) => {
                assert_eq!(log_path, "/tmp/run-1.log");
            }
            _ => panic!("expected open-log action"),
        }
    }

    #[test]
    fn open_log_management_action_requires_log_path() {
        let mut app = app_with_issue_and_thread();
        app.detail_session_runs[0].log_path = None;

        let action = resolve_run_management_action(&app, keys::Action::OpenRunLog);

        assert!(action.is_none());
    }

    #[test]
    fn retry_management_action_requires_existing_run() {
        let mut app = app_with_issue_and_thread();
        app.detail_session_runs.clear();

        let action = resolve_run_management_action(&app, keys::Action::RetryRun);

        assert!(action.is_none());
    }

    #[test]
    fn resolves_mark_stale_management_action() {
        let app = app_with_issue_and_thread();

        let action = resolve_run_management_action(&app, keys::Action::MarkRunStale);

        match action {
            Some(RunManagementAction::MarkStale { run_id }) => {
                assert_eq!(run_id, "run-1");
            }
            _ => panic!("expected mark-stale action"),
        }
    }
}
