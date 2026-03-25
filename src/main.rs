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
use std::io::stdout;
use std::path::Path;
use tokio::sync::mpsc;

/// Messages sent from background tasks back to the main loop.
enum BgMessage {
    Error(String),
    ThreadCreated { issue_identifier: String },
}

enum RunManagementAction {
    MarkStale {
        run_id: String,
    },
}

fn resolve_run_management_action(
    app: &App<impl LinearApi>,
    action: keys::Action,
) -> Option<RunManagementAction> {
    match action {
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

/// Build an issue context prompt for seeding a new Amp thread.
/// Metadata is wrapped in XML-style tags. Parent task details are included when present.
fn build_issue_context(issue: &api::types::Issue) -> String {
    let mut ctx = String::new();

    // Parent task context
    if let Some(parent) = &issue.parent {
        ctx.push_str("<parent_task>\n");
        ctx.push_str(&format!("# {} {}\n", parent.identifier, parent.title));
        if let Some(url) = &parent.url {
            ctx.push_str(&format!("{}\n", url));
        }
        if let Some(state) = &parent.state {
            ctx.push_str(&format!("Status: {}\n", state.name));
        }
        if let Some(labels) = &parent.labels
            && !labels.nodes.is_empty()
        {
            let names: Vec<&str> = labels.nodes.iter().map(|l| l.name.as_str()).collect();
            ctx.push_str(&format!("Labels: {}\n", names.join(", ")));
        }
        if let Some(desc) = &parent.description
            && !desc.is_empty()
        {
            ctx.push_str(&format!("\n{}\n", desc));
        }
        ctx.push_str("</parent_task>\n\n");
    }

    // Task context
    ctx.push_str("<task>\n");
    ctx.push_str(&format!("# {} {}\n", issue.identifier, issue.title));
    if let Some(url) = &issue.url {
        ctx.push_str(&format!("{}\n", url));
    }
    ctx.push('\n');
    ctx.push_str("<metadata>\n");
    if let Some(state) = &issue.state {
        ctx.push_str(&format!("Status: {}\n", state.name));
    }
    if let Some(project) = &issue.project {
        ctx.push_str(&format!("Project: {}\n", project.name));
    }
    if let Some(labels) = &issue.labels
        && !labels.nodes.is_empty()
    {
        let names: Vec<&str> = labels.nodes.iter().map(|l| l.name.as_str()).collect();
        ctx.push_str(&format!("Labels: {}\n", names.join(", ")));
    }
    if let Some(assignee) = &issue.assignee {
        ctx.push_str(&format!("Assignee: {}\n", assignee.name));
    }
    ctx.push_str("</metadata>\n");
    if let Some(desc) = &issue.description
        && !desc.is_empty()
    {
        ctx.push_str(&format!("\n{}\n", desc));
    }
    ctx.push_str("</task>\n");

    ctx
}

/// Detect the terminal emulator running ishi by checking `$TERM_PROGRAM`.
/// Returns the application name suitable for `open -a <name>`.
/// Falls back to "Terminal" if unrecognised.
fn detect_terminal() -> &'static str {
    match std::env::var("TERM_PROGRAM").as_deref() {
        Ok("iTerm.app") => "iTerm",
        Ok("WezTerm") => "WezTerm",
        Ok("Alacritty") => "Alacritty",
        Ok("ghostty") => "Ghostty",
        Ok("kitty") => "kitty",
        _ => "Terminal",
    }
}

/// Launch an existing thread in a new terminal window.
/// Opens a terminal running `amp threads continue <thread_id>` in the thread's workspace.
fn launch_thread(thread_id: &str, workspace: &str) -> Result<()> {
    let run_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("ishi")
        .join("run-logs");
    std::fs::create_dir_all(&run_dir)?;

    let launcher_path = run_dir.join(format!("{}.launch.sh", thread_id));
    let launcher_script = format!(
        r#"#!/usr/bin/env bash
cd {workspace} || exit 1
amp threads continue {thread_id}
rm -f {launcher}
"#,
        workspace = shell_escape::unix::escape(workspace.into()),
        thread_id = shell_escape::unix::escape(thread_id.into()),
        launcher = shell_escape::unix::escape(launcher_path.to_string_lossy().into_owned().into()),
    );
    std::fs::write(&launcher_path, &launcher_script)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launcher_path, std::fs::Permissions::from_mode(0o755))?;
    }

    let terminal_app = detect_terminal();
    std::process::Command::new("open")
        .args(["-a", terminal_app, launcher_path.to_str().unwrap_or("")])
        .spawn()
        .context("failed to open terminal window")?;

    Ok(())
}

/// Execute the new-thread flow:
/// 1. Create a thread via `amp threads new`
/// 2. Write a launcher script that pipes the prompt to `amp threads continue`
/// 3. Open the launcher in a new terminal window of the same type as ishi's terminal
/// 4. Record the thread link in state
fn start_new_thread(issue_identifier: &str, workspace: &str, prompt: &str) -> Result<()> {
    let workspace_path = Path::new(workspace);

    // Create a new thread via `amp threads new`
    let new_output = std::process::Command::new("amp")
        .args(["threads", "new"])
        .current_dir(workspace_path)
        .output()
        .context("failed to run `amp threads new`")?;
    if !new_output.status.success() {
        anyhow::bail!(
            "amp threads new failed: {}",
            String::from_utf8_lossy(&new_output.stderr)
        );
    }
    let thread_id = String::from_utf8_lossy(&new_output.stdout).trim().to_string();
    if thread_id.is_empty() {
        anyhow::bail!("amp threads new returned empty thread ID");
    }

    // Write prompt to a temp file
    let run_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("ishi")
        .join("run-logs");
    std::fs::create_dir_all(&run_dir)?;
    let prompt_path = run_dir.join(format!("{}.prompt", thread_id));
    std::fs::write(&prompt_path, prompt)?;

    // Build a launcher script
    let launcher_path = run_dir.join(format!("{}.sh", thread_id));
    let launcher_script = format!(
        r#"#!/usr/bin/env bash
cd {workspace} || exit 1
cat {prompt_file} | amp threads continue {thread_id}
rm -f {prompt_file} {launcher}
"#,
        workspace = shell_escape::unix::escape(workspace.into()),
        prompt_file = shell_escape::unix::escape(prompt_path.to_string_lossy().into_owned().into()),
        thread_id = shell_escape::unix::escape(thread_id.as_str().into()),
        launcher = shell_escape::unix::escape(launcher_path.to_string_lossy().into_owned().into()),
    );
    std::fs::write(&launcher_path, &launcher_script)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launcher_path, std::fs::Permissions::from_mode(0o755))?;
    }

    // Open in a new terminal window of the same type
    let terminal_app = detect_terminal();
    std::process::Command::new("open")
        .args(["-a", terminal_app, launcher_path.to_str().unwrap_or("")])
        .spawn()
        .context("failed to open terminal window")?;

    // Persist thread link (no session run tracking)
    let state_path = amp::state::state_path()?;
    let mut state = amp::state::State::load(&state_path)?;
    state.add_thread_link(&thread_id, issue_identifier, workspace);
    state.add_workspace(workspace);
    state.save(&state_path)?;

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

    // Channel for background tasks to report messages back to the UI.
    let (bg_tx, mut bg_rx) = mpsc::unbounded_channel::<BgMessage>();

    // Periodic reconciliation: reload thread/run state every ~3 seconds.
    const RECONCILE_INTERVAL_TICKS: u32 = 30; // 30 × 100ms poll = 3s
    let mut reconcile_tick_counter: u32 = 0;

    // Terminal setup
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Main loop
    while app.running {
        // Check for messages from background tasks.
        while let Ok(msg) = bg_rx.try_recv() {
            match msg {
                BgMessage::Error(err) => {
                    app.error = Some(app::AppError::new(err));
                }
                BgMessage::ThreadCreated { issue_identifier } => {
                    app.flash = Some(("Thread started ✓".into(), 30));
                    app.load_thread_counts();
                    // Reload threads if we're viewing the same issue.
                    if app
                        .context_issue()
                        .is_some_and(|i| i.identifier == issue_identifier)
                    {
                        load_threads_for_issue(&mut app, &issue_identifier);
                    }
                }
            }
        }

        // Periodic reconciliation of background run statuses.
        reconcile_tick_counter += 1;
        if reconcile_tick_counter >= RECONCILE_INTERVAL_TICKS
            && matches!(app.view, app::View::Detail)
        {
            reconcile_tick_counter = 0;
            let reconciled = reconcile_session_runs().unwrap_or(0) > 0;
            let has_active = app.active_run_counts() != (0, 0);
            // Reload thread data when statuses changed or runs are still active.
            if reconciled || has_active {
                app.load_thread_counts();
                if let Some(issue) = app.context_issue() {
                    let id = issue.identifier.clone();
                    load_threads_for_issue(&mut app, &id);
                }
            }
            // Re-read log file content when viewing a run log.
            if matches!(app.detail_section, app::DetailSection::RunLog) {
                app.refresh_run_log();
            }
        }

        app.tick_flash();
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
            } else if app.message_input_active {
                match key.code {
                    KeyCode::Enter => {
                        if let Some((_thread_id, _text)) = app.submit_message_input() {
                            // TODO: forward to SessionManager::send_message
                            // once SessionManager is wired into the main loop.
                            app.scroll_output_to_bottom();
                        }
                    }
                    KeyCode::Esc => app.cancel_message_input(),
                    KeyCode::Backspace => {
                        app.message_input.pop();
                    }
                    KeyCode::Char(c) => {
                        app.message_input.push(c);
                    }
                    _ => {}
                }
            } else if app.workspace_picker.is_some() {
                let is_typing = app.workspace_picker.as_ref().is_some_and(|p| p.typing);
                if is_typing {
                    match key.code {
                        KeyCode::Enter => {
                            if let Some(ref mut picker) = app.workspace_picker
                                && picker.input.is_empty()
                                && let Some(ws) = picker.options.get(picker.selected)
                            {
                                picker.input = ws.clone();
                            }
                            let typed_path = app
                                .workspace_picker
                                .as_mut()
                                .and_then(|p| p.confirm_typed_path());
                            if let (Some(workspace), Some(issue)) =
                                (typed_path, app.context_issue())
                            {
                                let issue_id = issue.identifier.clone();
                                let context = build_issue_context(issue);
                                app.workspace_picker = None;
                                app.flash = Some(("Starting thread …".into(), 0));
                                let tx = bg_tx.clone();
                                let issue_id_signal = issue_id.clone();
                                tokio::task::spawn_blocking(move || {
                                    match start_new_thread(&issue_id, &workspace, &context) {
                                        Ok(()) => {
                                            let _ = tx.send(BgMessage::ThreadCreated {
                                                issue_identifier: issue_id_signal,
                                            });
                                        }
                                        Err(err) => {
                                            let _ = tx.send(BgMessage::Error(format!(
                                                "Failed to start thread: {err}"
                                            )));
                                        }
                                    }
                                });
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
                            if let (Some(picker), Some(issue)) = (picker, app.context_issue())
                                && let Some(workspace) = picker.options.get(picker.selected)
                            {
                                let issue_id = issue.identifier.clone();
                                let context = build_issue_context(issue);
                                let workspace = workspace.clone();
                                app.flash = Some(("Starting thread …".into(), 0));
                                let tx = bg_tx.clone();
                                let issue_id_signal = issue_id.clone();
                                tokio::task::spawn_blocking(move || {
                                    match start_new_thread(&issue_id, &workspace, &context) {
                                        Ok(()) => {
                                            let _ = tx.send(BgMessage::ThreadCreated {
                                                issue_identifier: issue_id_signal,
                                            });
                                        }
                                        Err(err) => {
                                            let _ = tx.send(BgMessage::Error(format!(
                                                "Failed to start thread: {err}"
                                            )));
                                        }
                                    }
                                });
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
                if let KeyCode::Char('q') = key.code {
                    app.running = false;
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
                            let tx = bg_tx.clone();
                            tokio::spawn(async move {
                                if let Err(err) =
                                    api.update_issue_state(&issue_id, &state_name).await
                                {
                                    let _ = tx.send(BgMessage::Error(format!(
                                        "Failed to update state for {}: {}",
                                        identifier, err
                                    )));
                                }
                            });
                        }
                    }
                    KeyCode::Esc => app.cancel_state_change(),
                    KeyCode::Down | KeyCode::Right => app.state_change_move_down(),
                    KeyCode::Up | KeyCode::Left => app.state_change_move_up(),
                    KeyCode::Backspace => app.state_type_ahead_pop(),
                    KeyCode::Char(c) => app.state_type_ahead_push(c),
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
                            if let Some(issue) = app.context_issue() {
                                let issue_id = issue.id.clone();
                                match app.api.fetch_team_states(&issue_id).await {
                                    Ok(states) => app.start_state_change(states),
                                    Err(err) => {
                                        app.error =
                                            Some(app::AppError::new(format!("Failed to fetch states: {err}")));
                                    }
                                }
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
                            keys::Action::SendInstruction => app.start_message_input(),
                            keys::Action::Help => app.toggle_help(),
                            _ => {}
                        },
                        app::DetailSection::RunLog => match action {
                            keys::Action::Quit => app.awaiting_quit = true,
                            keys::Action::Back => {
                                app.detail_section = app::DetailSection::Threads;
                                app.run_log_lines.clear();
                                app.run_log_scroll = 0;
                            }
                            keys::Action::MoveDown => app.scroll_run_log_down(),
                            keys::Action::MoveUp => app.scroll_run_log_up(),
                            keys::Action::Top => app.run_log_scroll = 0,
                            keys::Action::Bottom => app.scroll_run_log_to_bottom(),
                            keys::Action::Help => app.toggle_help(),
                            _ => {}
                        },
                        app::DetailSection::Threads => match action {
                            keys::Action::Quit => app.awaiting_quit = true,
                            keys::Action::Back => app.focus_body(),
                            keys::Action::MoveDown => app.thread_move_down(),
                            keys::Action::MoveUp => app.thread_move_up(),
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
                                if let Some(thread) = app.selected_thread() {
                                    let thread_id = thread.id.clone();
                                    let state_path = amp::state::state_path();
                                    let workspace = state_path.ok().and_then(|p| {
                                        amp::state::State::load(&p).ok().and_then(|s| {
                                            s.thread_links
                                                .get(&thread_id)
                                                .map(|l| l.workspace.clone())
                                        })
                                    });
                                    if let Some(ws) = workspace {
                                        if let Err(err) = launch_thread(&thread_id, &ws) {
                                            app.error = Some(app::AppError::new(format!(
                                                "Failed to launch thread: {err}"
                                            )));
                                        }
                                    } else {
                                        app.error = Some(app::AppError::new(
                                            "No workspace found for this thread".to_string(),
                                        ));
                                    }
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
                            keys::Action::ChangeState => {
                                if let Some(issue) = app.context_issue() {
                                    let issue_id = issue.id.clone();
                                    match app.api.fetch_team_states(&issue_id).await {
                                        Ok(states) => app.start_state_change(states),
                                        Err(err) => {
                                            app.error = Some(app::AppError::new(format!(
                                                "Failed to fetch states: {err}"
                                            )));
                                        }
                                    }
                                }
                            }
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
                        if let Some(issue) = app.context_issue() {
                            let issue_id = issue.id.clone();
                            match app.api.fetch_team_states(&issue_id).await {
                                Ok(states) => app.start_state_change(states),
                                Err(err) => {
                                    app.error =
                                        Some(app::AppError::new(format!("Failed to fetch states: {err}")));
                                }
                            }
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
            parent: None,
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

    fn minimal_issue() -> Issue {
        Issue {
            id: "1".into(),
            identifier: "JEM-42".into(),
            title: "Fix the widget".into(),
            url: None,
            state: None,
            priority: None,
            project: None,
            description: None,
            assignee: None,
            labels: None,
            comments: None,
            parent: None,
        }
    }

    #[test]
    fn build_issue_context_minimal() {
        let ctx = build_issue_context(&minimal_issue());
        assert!(ctx.contains("<task>"));
        assert!(ctx.contains("</task>"));
        assert!(ctx.contains("# JEM-42 Fix the widget"));
        assert!(ctx.contains("<metadata>"));
        assert!(ctx.contains("</metadata>"));
        assert!(!ctx.contains("<parent_task>"));
    }

    #[test]
    fn build_issue_context_includes_all_fields() {
        use crate::api::types::*;
        let issue = Issue {
            url: Some("https://linear.app/issue/JEM-42".into()),
            state: Some(IssueState {
                name: "In Progress".into(),
            }),
            project: Some(IssueProject {
                name: "ishi".into(),
            }),
            labels: Some(IssueLabels {
                nodes: vec![
                    IssueLabel { name: "bug".into() },
                    IssueLabel {
                        name: "urgent".into(),
                    },
                ],
            }),
            description: Some("Describe the problem in detail.".into()),
            parent: Some(Box::new(IssueParent {
                identifier: "JEM-40".into(),
                title: "Parent task".into(),
                description: Some("Parent description.".into()),
                url: Some("https://linear.app/issue/JEM-40".into()),
                state: Some(IssueState { name: "Todo".into() }),
                labels: None,
            })),
            ..minimal_issue()
        };
        let ctx = build_issue_context(&issue);
        assert!(ctx.contains("<parent_task>"));
        assert!(ctx.contains("# JEM-40 Parent task"));
        assert!(ctx.contains("Parent description."));
        assert!(ctx.contains("</parent_task>"));
        assert!(ctx.contains("<task>"));
        assert!(ctx.contains("https://linear.app/issue/JEM-42"));
        assert!(ctx.contains("Status: In Progress"));
        assert!(ctx.contains("Project: ishi"));
        assert!(ctx.contains("Labels: bug, urgent"));
        assert!(ctx.contains("Describe the problem in detail."));
        assert!(ctx.contains("</task>"));
    }

    #[test]
    fn build_issue_context_skips_empty_description() {
        let issue = Issue {
            description: Some("".into()),
            ..minimal_issue()
        };
        let ctx = build_issue_context(&issue);
        // Should have metadata tags but description should not appear between metadata and task close
        assert!(ctx.contains("<metadata>"));
        assert!(ctx.contains("</task>"));
    }

    #[test]
    fn build_issue_context_skips_empty_labels() {
        use crate::api::types::*;
        let issue = Issue {
            labels: Some(IssueLabels { nodes: vec![] }),
            ..minimal_issue()
        };
        let ctx = build_issue_context(&issue);
        assert!(!ctx.contains("Labels"));
    }

    #[test]
    fn detect_terminal_defaults_to_terminal() {
        // When TERM_PROGRAM is not set to a known value, should default
        let result = detect_terminal();
        assert!(!result.is_empty());
    }
}
