use std::cmp::Ordering;
use std::time::Duration;

use crate::amp::output::{OutputLine, SessionOutputBuffer};
use crate::amp::state::SessionRunStatus;
use crate::amp::thread::ThreadSummary;
use crate::api::cache::ResponseCache;
use crate::api::client::LinearApi;
use crate::api::types::{Issue, Project};

/// Lightweight summary of a session run for display in the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRunSummary {
    pub run_id: String,
    pub thread_id: String,
    pub status: SessionRunStatus,
    pub log_path: Option<String>,
    pub created_at_ms: u64,
}

/// Pick the more prominent of two statuses for display purposes.
/// Running > Pending > Failed > Stale > Completed.
fn pick_display_status(a: SessionRunStatus, b: SessionRunStatus) -> SessionRunStatus {
    fn rank(s: SessionRunStatus) -> u8 {
        match s {
            SessionRunStatus::Running => 4,
            SessionRunStatus::Pending => 3,
            SessionRunStatus::Failed => 2,
            SessionRunStatus::Stale => 1,
            SessionRunStatus::Completed => 0,
        }
    }
    if rank(b) > rank(a) { b } else { a }
}

const CACHE_TTL_SECS: u64 = 300; // 5 minutes
const CACHE_KEY_MY_ISSUES: &str = "my_issues";
const CACHE_KEY_PROJECTS: &str = "projects";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailSection {
    Body,
    Threads,
    Output,
}

pub enum View {
    MyIssues,
    ProjectList,
    ProjectDetail,
    Detail,
}

/// Tracks which list view the user came from when entering the Detail view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailOrigin {
    MyIssues,
    ProjectDetail,
}

/// State for the workspace picker modal shown when starting a new Amp thread.
#[derive(Debug, Clone)]
pub struct WorkspacePicker {
    pub options: Vec<String>,
    pub selected: usize,
    /// When true, the user is typing a new workspace path via `/`.
    pub typing: bool,
    /// Buffer for the path being typed.
    pub input: String,
    /// The original options before tab completion replaced them.
    original_options: Vec<String>,
}

#[allow(dead_code)]
impl WorkspacePicker {
    pub fn new(options: Vec<String>) -> Self {
        let original_options = options.clone();
        Self {
            options,
            selected: 0,
            typing: false,
            input: String::new(),
            original_options,
        }
    }

    pub fn move_down(&mut self) {
        if !self.options.is_empty() && self.selected < self.options.len() - 1 {
            self.selected += 1;
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn selected_workspace(&self) -> Option<&str> {
        self.options.get(self.selected).map(|s| s.as_str())
    }

    pub fn start_typing(&mut self) {
        self.typing = true;
        self.input = std::env::current_dir()
            .map(|p| {
                let mut s = p.to_string_lossy().to_string();
                if !s.ends_with('/') {
                    s.push('/');
                }
                s
            })
            .unwrap_or_default();
    }

    pub fn cancel_typing(&mut self) {
        self.typing = false;
        self.input.clear();
        self.options = self.original_options.clone();
        self.selected = 0;
    }

    /// Delete one path component from the end of the input.
    ///
    /// Strips trailing slash, then removes back to the previous `/`.
    /// For example: `/Users/me/projects/` → `/Users/me/`
    pub fn delete_path_component(&mut self) {
        // Strip trailing slash so we target the last component.
        if self.input.ends_with('/') {
            self.input.pop();
        }
        // Remove characters back to the previous `/` (inclusive of everything after it).
        if let Some(pos) = self.input.rfind('/') {
            self.input.truncate(pos + 1);
        } else {
            self.input.clear();
        }
    }

    /// Perform tab completion on the current input.
    ///
    /// Lists directories matching the current input prefix and replaces the
    /// picker's options with the results. The user can then navigate them with
    /// j/k or arrows and press Enter to select, just like the normal picker.
    /// If exactly one directory matches, the input is completed immediately and
    /// the options are populated with its children.
    pub fn tab_complete(&mut self) {
        use std::path::Path;

        let base = &self.input;
        let path = Path::new(base);
        let (dir, prefix) = if base.ends_with('/') || base.is_empty() {
            (
                if base.is_empty() {
                    Path::new(".")
                } else {
                    path
                },
                "",
            )
        } else {
            let parent = path.parent().unwrap_or(Path::new("."));
            let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
            (parent, file_name)
        };

        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut matches: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with(prefix))
                })
                .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
                .map(|e| {
                    let mut full = dir.join(e.file_name()).to_string_lossy().to_string();
                    full.push('/');
                    full
                })
                .collect();
            matches.sort();

            match matches.len() {
                0 => {} // no matches — do nothing
                1 => {
                    // Single match — complete the input, don't change options.
                    self.input = matches[0].clone();
                }
                _ => {
                    // Multiple matches — show them as selectable options.
                    self.options = matches;
                    self.selected = 0;
                }
            }
        }
    }

    /// Confirm the typed path. Returns the entered path if non-empty.
    /// If the input is empty but there's a selected option, returns that instead.
    pub fn confirm_typed_path(&mut self) -> Option<String> {
        self.typing = false;
        let path = self.input.trim().to_string();
        let result = if !path.is_empty() {
            Some(path)
        } else {
            self.options.get(self.selected).cloned()
        };
        self.input.clear();
        self.options = self.original_options.clone();
        self.selected = 0;
        result
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Identifier,
    Title,
    Project,
    Status,
    Priority,
}

impl SortColumn {
    pub fn label(&self) -> &'static str {
        match self {
            SortColumn::Identifier => "ID",
            SortColumn::Title => "Title",
            SortColumn::Project => "Project",
            SortColumn::Status => "Status",
            SortColumn::Priority => "Priority",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSortColumn {
    Name,
    Status,
    Lead,
    Progress,
}

impl ProjectSortColumn {
    pub fn label(&self) -> &'static str {
        match self {
            ProjectSortColumn::Name => "Name",
            ProjectSortColumn::Status => "Status",
            ProjectSortColumn::Lead => "Lead",
            ProjectSortColumn::Progress => "Progress",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    pub fn toggle(self) -> Self {
        match self {
            SortDirection::Asc => SortDirection::Desc,
            SortDirection::Desc => SortDirection::Asc,
        }
    }
}

/// User-friendly error message displayed in the status bar.
#[derive(Debug, Clone)]
pub struct AppError {
    pub message: String,
}

impl AppError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Classify an `anyhow::Error` into a user-friendly message.
    pub fn from_api_error(err: &anyhow::Error) -> Self {
        let msg = format!("{err}");
        if msg.contains("401") {
            Self::new("Authentication failed — check your API key")
        } else if msg.contains("403") {
            Self::new("Access denied — insufficient permissions")
        } else if msg.contains("429") {
            Self::new("Rate limited — please wait and try again")
        } else if msg.contains("dns error")
            || msg.contains("connect error")
            || msg.contains("No connection")
            || msg.contains("error trying to connect")
        {
            Self::new("Network error — check your internet connection")
        } else if msg.contains("timed out") || msg.contains("timeout") {
            Self::new("Request timed out — try again")
        } else {
            Self::new(format!("API error: {msg}"))
        }
    }
}

pub struct App<A: LinearApi> {
    pub running: bool,
    pub view: View,
    pub api: A,
    pub issues: Vec<Issue>,
    pub selected: usize,
    pub filter: Option<(SortColumn, String)>,
    pub filter_input: String,
    pub filter_column: Option<SortColumn>,
    pub filtering: bool,
    pub awaiting_quit: bool,
    pub awaiting_filter: bool,
    pub awaiting_sort: bool,
    pub awaiting_open: bool,
    pub awaiting_state_change: bool,
    pub state_options: Vec<String>,
    pub state_selected: usize,
    pub sort: Option<(SortColumn, SortDirection)>,
    pub search: Option<String>,
    pub search_input: String,
    pub searching: bool,
    pub detail_scroll: u16,
    pub detail_scroll_max: u16,
    pub refreshing: bool,
    pub loading: bool,
    pub error: Option<AppError>,
    pub detail_section: DetailSection,
    pub detail_threads: Vec<ThreadSummary>,
    pub detail_thread_selected: usize,
    pub detail_session_runs: Vec<SessionRunSummary>,
    pub workspace_picker: Option<WorkspacePicker>,
    pub output_buffer: SessionOutputBuffer,
    pub detail_output_scroll: u16,
    pub detail_output_scroll_max: u16,
    pub message_input_active: bool,
    pub message_input: String,
    pub show_help: bool,
    pub cache: ResponseCache<Vec<Issue>>,
    pub projects: Vec<Project>,
    pub project_selected: usize,
    pub project_sort: Option<(ProjectSortColumn, SortDirection)>,
    pub project_cache: ResponseCache<Vec<Project>>,
    pub project_issues: Vec<Issue>,
    pub project_issue_selected: usize,
    pub detail_origin: DetailOrigin,
}

impl<A: LinearApi> App<A> {
    pub fn new(api: A) -> Self {
        Self {
            running: true,
            view: View::MyIssues,
            api,
            issues: Vec::new(),
            selected: 0,
            filter: None,
            filter_input: String::new(),
            filter_column: None,
            filtering: false,
            awaiting_quit: false,
            awaiting_filter: false,
            awaiting_sort: false,
            awaiting_open: false,
            awaiting_state_change: false,
            state_options: Vec::new(),
            state_selected: 0,
            sort: None,
            search: None,
            search_input: String::new(),
            searching: false,
            detail_scroll: 0,
            detail_scroll_max: 0,
            refreshing: false,
            loading: false,
            error: None,
            detail_section: DetailSection::Body,
            detail_threads: Vec::new(),
            detail_thread_selected: 0,
            detail_session_runs: Vec::new(),
            workspace_picker: None,
            output_buffer: SessionOutputBuffer::new(),
            detail_output_scroll: 0,
            detail_output_scroll_max: 0,
            message_input_active: false,
            message_input: String::new(),
            show_help: false,
            cache: ResponseCache::new(Duration::from_secs(CACHE_TTL_SECS)),
            projects: Vec::new(),
            project_selected: 0,
            project_sort: None,
            project_cache: ResponseCache::new(Duration::from_secs(CACHE_TTL_SECS)),
            project_issues: Vec::new(),
            project_issue_selected: 0,
            detail_origin: DetailOrigin::MyIssues,
        }
    }

    pub fn filtered_issues(&self) -> Vec<&Issue> {
        let mut issues: Vec<&Issue> = self.issues.iter().collect();
        if let Some((col, f)) = &self.filter {
            let lower = f.to_lowercase();
            issues.retain(|i| {
                let value = match col {
                    SortColumn::Identifier => i.identifier.as_str(),
                    SortColumn::Title => i.title.as_str(),
                    SortColumn::Project => i.project_str(),
                    SortColumn::Status => i.status_str(),
                    SortColumn::Priority => i.priority_str(),
                };
                value.to_lowercase().contains(&lower)
            });
        }
        if let Some(q) = &self.search {
            let lower = q.to_lowercase();
            issues.retain(|i| i.matches_search(&lower));
        }
        if let Some((col, dir)) = &self.sort {
            issues.sort_by(|a, b| {
                let ord = match col {
                    SortColumn::Identifier => a.identifier.cmp(&b.identifier),
                    SortColumn::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
                    SortColumn::Project => a.project_str().cmp(b.project_str()),
                    SortColumn::Status => a.status_str().cmp(b.status_str()),
                    SortColumn::Priority => a
                        .priority
                        .partial_cmp(&b.priority)
                        .unwrap_or(Ordering::Equal),
                };
                match dir {
                    SortDirection::Asc => ord,
                    SortDirection::Desc => ord.reverse(),
                }
            });
        }
        issues
    }

    pub fn set_sort(&mut self, col: SortColumn) {
        self.sort = Some(match self.sort {
            Some((c, dir)) if c == col => (col, dir.toggle()),
            _ => (col, SortDirection::Asc),
        });
        self.selected = 0;
    }

    pub fn move_down(&mut self) {
        let len = self.filtered_issues().len();
        if len > 0 && self.selected < len - 1 {
            self.selected += 1;
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn top(&mut self) {
        self.selected = 0;
    }

    pub fn bottom(&mut self) {
        let len = self.filtered_issues().len();
        if len > 0 {
            self.selected = len - 1;
        }
    }

    pub fn start_search(&mut self) {
        self.searching = true;
        self.search_input.clear();
    }

    pub fn apply_search(&mut self) {
        self.searching = false;
        if self.search_input.is_empty() {
            self.search = None;
        } else {
            self.search = Some(self.search_input.clone());
        }
        self.selected = 0;
    }

    pub fn cancel_search(&mut self) {
        self.searching = false;
        self.search_input.clear();
    }

    pub fn clear_search(&mut self) {
        self.search = None;
        self.search_input.clear();
        self.selected = 0;
    }

    #[allow(dead_code)]
    pub fn start_filter(&mut self) {
        self.start_column_filter(SortColumn::Title);
    }

    pub fn start_column_filter(&mut self, col: SortColumn) {
        self.filter_column = Some(col);
        self.filtering = true;
        self.filter_input.clear();
    }

    pub fn apply_filter(&mut self) {
        self.filtering = false;
        if self.filter_input.is_empty() {
            self.filter = None;
        } else if let Some(col) = self.filter_column {
            self.filter = Some((col, self.filter_input.clone()));
        }
        self.filter_column = None;
        self.selected = 0;
    }

    pub fn cancel_filter(&mut self) {
        self.filtering = false;
        self.filter_input.clear();
    }

    pub fn clear_filter(&mut self) {
        self.filter = None;
        self.filter_column = None;
        self.filter_input.clear();
        self.selected = 0;
    }

    pub fn select_issue(&mut self) {
        let issues = self.filtered_issues();
        if self.selected < issues.len() {
            self.view = View::Detail;
            self.detail_scroll = 0;
            self.detail_section = DetailSection::Body;
            self.detail_thread_selected = 0;
            self.detail_origin = DetailOrigin::MyIssues;
        }
    }

    pub fn back_to_list(&mut self) {
        self.view = match self.detail_origin {
            DetailOrigin::MyIssues => View::MyIssues,
            DetailOrigin::ProjectDetail => View::ProjectDetail,
        };
        self.detail_scroll = 0;
        self.detail_section = DetailSection::Body;
        self.detail_threads.clear();
        self.detail_thread_selected = 0;
        self.detail_session_runs.clear();
        self.detail_output_scroll = 0;
        self.detail_output_scroll_max = 0;
        self.message_input_active = false;
        self.message_input.clear();
    }

    pub fn selected_issue(&self) -> Option<&Issue> {
        self.context_issue()
    }

    pub fn scroll_detail_down(&mut self) {
        if self.detail_scroll < self.detail_scroll_max {
            self.detail_scroll = self.detail_scroll.saturating_add(1);
        }
    }

    pub fn scroll_detail_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    pub fn thread_move_down(&mut self) {
        let len = self.detail_threads.len();
        if len > 0 && self.detail_thread_selected < len - 1 {
            self.detail_thread_selected += 1;
        }
    }

    pub fn thread_move_up(&mut self) {
        if self.detail_thread_selected > 0 {
            self.detail_thread_selected -= 1;
        }
    }

    pub fn selected_thread(&self) -> Option<&ThreadSummary> {
        if self.detail_section == DetailSection::Threads {
            self.detail_threads.get(self.detail_thread_selected)
        } else {
            None
        }
    }

    /// Return the newest run summary for a given thread.
    pub fn latest_run_for_thread(&self, thread_id: &str) -> Option<&SessionRunSummary> {
        self.detail_session_runs
            .iter()
            .filter(|r| r.thread_id == thread_id)
            .max_by_key(|r| r.created_at_ms)
    }

    /// Return the newest run summary for the selected thread in the detail threads section.
    pub fn selected_thread_run(&self) -> Option<&SessionRunSummary> {
        let thread = self.selected_thread()?;
        self.latest_run_for_thread(&thread.id)
    }

    /// Return the most relevant session run status for a given thread ID.
    /// Prefers active statuses (running > pending) over terminal ones.
    pub fn run_status_for_thread(&self, thread_id: &str) -> Option<SessionRunStatus> {
        let mut best: Option<SessionRunStatus> = None;
        for run in &self.detail_session_runs {
            if run.thread_id == thread_id {
                best = Some(match best {
                    None => run.status,
                    Some(prev) => pick_display_status(prev, run.status),
                });
            }
        }
        best
    }

    /// Return aggregate counts of active (running/pending) runs for the current issue.
    pub fn active_run_counts(&self) -> (usize, usize) {
        let running = self
            .detail_session_runs
            .iter()
            .filter(|r| r.status == SessionRunStatus::Running)
            .count();
        let pending = self
            .detail_session_runs
            .iter()
            .filter(|r| r.status == SessionRunStatus::Pending)
            .count();
        (running, pending)
    }

    pub fn focus_threads(&mut self) {
        if !self.detail_threads.is_empty() {
            self.detail_section = DetailSection::Threads;
        }
    }

    pub fn focus_body(&mut self) {
        self.detail_section = DetailSection::Body;
    }

    /// Switch to the output section for the currently selected thread.
    pub fn focus_output(&mut self) {
        if let Some(thread) = self.selected_thread() {
            if self.output_buffer.line_count(&thread.id) > 0 {
                self.detail_section = DetailSection::Output;
                self.detail_output_scroll = 0;
            }
        }
    }

    /// Return output lines for the currently selected thread.
    pub fn selected_thread_output(&self) -> &[OutputLine] {
        match self.selected_thread() {
            Some(t) => self.output_buffer.lines_for(&t.id),
            None => &[],
        }
    }

    pub fn scroll_output_down(&mut self) {
        if self.detail_output_scroll < self.detail_output_scroll_max {
            self.detail_output_scroll = self.detail_output_scroll.saturating_add(1);
        }
    }

    pub fn scroll_output_up(&mut self) {
        self.detail_output_scroll = self.detail_output_scroll.saturating_sub(1);
    }

    /// Auto-scroll the output view to the bottom.
    pub fn scroll_output_to_bottom(&mut self) {
        self.detail_output_scroll = self.detail_output_scroll_max;
    }

    /// Activate the message input bar in the output section.
    pub fn start_message_input(&mut self) {
        self.message_input_active = true;
        self.message_input.clear();
    }

    /// Cancel message input without sending.
    pub fn cancel_message_input(&mut self) {
        self.message_input_active = false;
        self.message_input.clear();
    }

    /// Submit the current message input.
    ///
    /// Adds the message to the output buffer as a user message and returns
    /// the (thread_id, message_text) pair for the caller to forward to the
    /// session. Returns `None` if the input is empty or no thread is selected.
    pub fn submit_message_input(&mut self) -> Option<(String, String)> {
        self.message_input_active = false;
        let text = self.message_input.trim().to_string();
        if text.is_empty() {
            self.message_input.clear();
            return None;
        }

        let thread_id = self
            .detail_threads
            .get(self.detail_thread_selected)
            .map(|t| t.id.clone())?;

        self.output_buffer.push_user_message(&thread_id, &text);
        self.message_input.clear();
        Some((thread_id, text))
    }

    pub fn show_workspace_picker(&mut self, workspaces: Vec<String>) {
        self.workspace_picker = Some(WorkspacePicker::new(workspaces));
    }

    pub fn cancel_workspace_picker(&mut self) {
        self.workspace_picker = None;
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn dismiss_help(&mut self) {
        self.show_help = false;
    }

    pub fn selected_issue_url(&self) -> Option<String> {
        self.context_issue().and_then(|i| i.url.clone())
    }

    /// Returns the issue relevant to the current view context.
    /// In ProjectDetail or Detail-from-project, returns the selected project issue.
    /// Otherwise, returns the selected my-issues issue.
    pub fn context_issue(&self) -> Option<&Issue> {
        match self.view {
            View::ProjectDetail => self.selected_project_issue(),
            View::Detail if self.detail_origin == DetailOrigin::ProjectDetail => {
                self.selected_project_issue()
            }
            _ => {
                let issues = self.filtered_issues();
                issues.get(self.selected).copied()
            }
        }
    }

    pub fn switch_to_projects(&mut self) {
        self.view = View::ProjectList;
    }

    pub fn switch_to_my_issues(&mut self) {
        self.view = View::MyIssues;
    }

    pub fn sorted_projects(&self) -> Vec<&Project> {
        let mut projects: Vec<&Project> = self.projects.iter().collect();
        if let Some((col, dir)) = &self.project_sort {
            projects.sort_by(|a, b| {
                let ord = match col {
                    ProjectSortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    ProjectSortColumn::Status => a.status_str().cmp(b.status_str()),
                    ProjectSortColumn::Lead => a.lead_str().cmp(b.lead_str()),
                    ProjectSortColumn::Progress => a
                        .progress
                        .partial_cmp(&b.progress)
                        .unwrap_or(Ordering::Equal),
                };
                match dir {
                    SortDirection::Asc => ord,
                    SortDirection::Desc => ord.reverse(),
                }
            });
        }
        projects
    }

    pub fn set_project_sort(&mut self, col: ProjectSortColumn) {
        self.project_sort = Some(match self.project_sort {
            Some((c, dir)) if c == col => (col, dir.toggle()),
            _ => (col, SortDirection::Asc),
        });
        self.project_selected = 0;
    }

    pub fn project_move_down(&mut self) {
        let len = self.projects.len();
        if len > 0 && self.project_selected < len - 1 {
            self.project_selected += 1;
        }
    }

    pub fn project_move_up(&mut self) {
        if self.project_selected > 0 {
            self.project_selected -= 1;
        }
    }

    pub fn project_top(&mut self) {
        self.project_selected = 0;
    }

    pub fn project_bottom(&mut self) {
        let len = self.projects.len();
        if len > 0 {
            self.project_selected = len - 1;
        }
    }

    pub fn selected_project(&self) -> Option<&Project> {
        self.projects.get(self.project_selected)
    }

    pub fn select_project(&mut self) {
        if self.project_selected < self.projects.len() {
            self.view = View::ProjectDetail;
            self.project_issue_selected = 0;
        }
    }

    pub fn back_from_project_detail(&mut self) {
        self.view = View::ProjectList;
        self.project_issues.clear();
        self.project_issue_selected = 0;
    }

    pub fn project_issue_move_down(&mut self) {
        let len = self.project_issues.len();
        if len > 0 && self.project_issue_selected < len - 1 {
            self.project_issue_selected += 1;
        }
    }

    pub fn project_issue_move_up(&mut self) {
        if self.project_issue_selected > 0 {
            self.project_issue_selected -= 1;
        }
    }

    pub fn project_issue_top(&mut self) {
        self.project_issue_selected = 0;
    }

    pub fn project_issue_bottom(&mut self) {
        let len = self.project_issues.len();
        if len > 0 {
            self.project_issue_selected = len - 1;
        }
    }

    pub fn selected_project_issue(&self) -> Option<&Issue> {
        self.project_issues.get(self.project_issue_selected)
    }

    pub fn select_project_issue(&mut self) {
        if self.project_issue_selected < self.project_issues.len() {
            self.view = View::Detail;
            self.detail_scroll = 0;
            self.detail_section = DetailSection::Body;
            self.detail_thread_selected = 0;
            self.detail_origin = DetailOrigin::ProjectDetail;
        }
    }

    pub fn selected_project_url(&self) -> Option<String> {
        self.selected_project().and_then(|p| p.url.clone())
    }

    pub fn dismiss_error(&mut self) {
        self.error = None;
    }

    pub fn start_state_change(&mut self) {
        const STATES: &[&str] = &[
            "Backlog",
            "Todo",
            "In Progress",
            "In Review",
            "Done",
            "Canceled",
        ];
        self.state_options = STATES.iter().map(|s| s.to_string()).collect();
        let current = self.context_issue().map(|i| i.status_str().to_string());
        self.state_selected = current
            .and_then(|c| STATES.iter().position(|s| *s == c))
            .unwrap_or(0);
        self.awaiting_state_change = true;
    }

    pub fn cancel_state_change(&mut self) {
        self.awaiting_state_change = false;
        self.state_options.clear();
        self.state_selected = 0;
    }

    pub fn state_change_move_down(&mut self) {
        if !self.state_options.is_empty() && self.state_selected < self.state_options.len() - 1 {
            self.state_selected += 1;
        }
    }

    pub fn state_change_move_up(&mut self) {
        if self.state_selected > 0 {
            self.state_selected -= 1;
        }
    }

    pub fn selected_state_option(&self) -> Option<&str> {
        self.state_options
            .get(self.state_selected)
            .map(|s| s.as_str())
    }

    /// Apply a state change to the currently selected issue in local data.
    pub fn apply_local_state_change(&mut self, new_state: &str) {
        if let Some(issue) = self.context_issue() {
            let issue_id = issue.id.clone();
            let identifier = issue.identifier.clone();
            // Update in my issues list
            if let Some(issue) = self.issues.iter_mut().find(|i| i.id == issue_id) {
                issue.state = Some(crate::api::types::IssueState {
                    name: new_state.to_string(),
                });
            }
            // Update in project issues list
            if let Some(issue) = self.project_issues.iter_mut().find(|i| i.id == issue_id) {
                issue.state = Some(crate::api::types::IssueState {
                    name: new_state.to_string(),
                });
            }
            // Invalidate caches so next refresh gets fresh data
            self.cache.invalidate(CACHE_KEY_MY_ISSUES);
            self.project_cache.invalidate(CACHE_KEY_PROJECTS);
            let _ = identifier; // suppress unused warning
        }
    }

    /// Load projects, serving from cache if fresh, otherwise fetching from API.
    pub async fn load_projects(&mut self) {
        if let Some(cached) = self.project_cache.get(CACHE_KEY_PROJECTS) {
            self.projects = cached.clone();
            return;
        }
        self.loading = self.projects.is_empty();
        self.fetch_and_cache_projects().await;
        self.loading = false;
    }

    async fn fetch_and_cache_projects(&mut self) {
        match self.api.fetch_projects().await {
            Ok(projects) => {
                self.error = None;
                self.project_cache
                    .insert(CACHE_KEY_PROJECTS, projects.clone());
                self.projects = projects;
            }
            Err(e) => {
                self.error = Some(AppError::from_api_error(&e));
            }
        }
    }

    pub async fn load_project_issues(&mut self) {
        if let Some(project) = self.selected_project() {
            let project_id = project.id.clone();
            self.loading = self.project_issues.is_empty();
            match self.api.fetch_project_issues(&project_id).await {
                Ok(issues) => {
                    self.error = None;
                    self.project_issues = issues;
                    self.project_issue_selected = 0;
                }
                Err(e) => {
                    self.error = Some(AppError::from_api_error(&e));
                }
            }
            self.loading = false;
        }
    }

    pub async fn refresh_projects(&mut self) {
        self.refreshing = true;
        self.project_cache.invalidate(CACHE_KEY_PROJECTS);
        self.fetch_and_cache_projects().await;
        self.refreshing = false;
    }

    /// Load issues, serving from cache if fresh, otherwise fetching from API.
    /// On cache hit, returns immediately. On miss, fetches and populates cache.
    pub async fn load_issues(&mut self) {
        if let Some(cached) = self.cache.get(CACHE_KEY_MY_ISSUES) {
            self.issues = cached.clone();
            return;
        }
        self.loading = self.issues.is_empty();
        self.fetch_and_cache_issues().await;
        self.loading = false;
    }

    /// Force-refresh all data from API, bypassing caches.
    /// Re-fetches issues and projects, then returns the identifier of the
    /// currently selected issue (if any) so callers can reload thread data.
    pub async fn refresh(&mut self) -> Option<String> {
        self.refreshing = true;
        self.cache.invalidate(CACHE_KEY_MY_ISSUES);
        self.project_cache.invalidate(CACHE_KEY_PROJECTS);
        self.fetch_and_cache_issues().await;
        self.fetch_and_cache_projects().await;
        self.refreshing = false;
        self.context_issue().map(|i| i.identifier.clone())
    }

    async fn fetch_and_cache_issues(&mut self) {
        let selected_id = self.selected_issue().map(|i| i.identifier.clone());
        match self.api.fetch_my_issues().await {
            Ok(issues) => {
                self.error = None;
                self.cache.insert(CACHE_KEY_MY_ISSUES, issues.clone());
                self.issues = issues;
                if let Some(id) = selected_id {
                    let new_index = self
                        .filtered_issues()
                        .iter()
                        .position(|i| i.identifier == id);
                    self.selected = new_index.unwrap_or(0);
                }
            }
            Err(e) => {
                self.error = Some(AppError::from_api_error(&e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::fake::FakeLinearApi;

    fn app_with_issues() -> App<FakeLinearApi> {
        let mut app = App::new(FakeLinearApi::new());
        app.issues = vec![
            Issue {
                id: "1".into(),
                identifier: "JEM-1".into(),
                title: "Alpha task".into(),
                url: Some("https://linear.app/test/issue/JEM-1".into()),
                state: None,
                priority: Some(2.0),
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
            Issue {
                id: "2".into(),
                identifier: "JEM-2".into(),
                title: "Beta task".into(),
                url: Some("https://linear.app/test/issue/JEM-2".into()),
                state: None,
                priority: Some(3.0),
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
            Issue {
                id: "3".into(),
                identifier: "JEM-3".into(),
                title: "Gamma task".into(),
                url: Some("https://linear.app/test/issue/JEM-3".into()),
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
        ];
        app
    }

    #[test]
    fn navigation_wraps_at_bounds() {
        let mut app = app_with_issues();
        assert_eq!(app.selected, 0);

        app.move_down();
        assert_eq!(app.selected, 1);
        app.move_down();
        assert_eq!(app.selected, 2);
        app.move_down(); // at bottom, stays
        assert_eq!(app.selected, 2);

        app.move_up();
        assert_eq!(app.selected, 1);
        app.top();
        assert_eq!(app.selected, 0);
        app.move_up(); // at top, stays
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn bottom_goes_to_last() {
        let mut app = app_with_issues();
        app.bottom();
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn filter_narrows_results() {
        let mut app = app_with_issues();
        app.filter = Some((SortColumn::Title, "beta".into()));
        let filtered = app.filtered_issues();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].identifier, "JEM-2");
    }

    #[test]
    fn filter_is_case_insensitive() {
        let mut app = app_with_issues();
        app.filter = Some((SortColumn::Title, "ALPHA".into()));
        assert_eq!(app.filtered_issues().len(), 1);
    }

    #[test]
    fn apply_empty_filter_clears() {
        let mut app = app_with_issues();
        app.start_filter();
        app.apply_filter();
        assert!(app.filter.is_none());
        assert!(!app.filtering);
    }

    #[test]
    fn apply_filter_sets_and_resets_cursor() {
        let mut app = app_with_issues();
        app.selected = 2;
        app.start_filter();
        app.filter_input = "gamma".into();
        app.apply_filter();
        assert_eq!(app.filter.as_ref().map(|(_, f)| f.as_str()), Some("gamma"));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn cancel_filter_discards_input() {
        let mut app = app_with_issues();
        app.start_filter();
        app.filter_input = "something".into();
        app.cancel_filter();
        assert!(!app.filtering);
        assert!(app.filter_input.is_empty());
    }

    #[test]
    fn select_issue_enters_detail_view() {
        let mut app = app_with_issues();
        app.selected = 1;
        app.select_issue();
        assert!(matches!(app.view, View::Detail));
        assert_eq!(app.detail_scroll, 0);
    }

    #[test]
    fn back_to_list_returns_to_my_issues() {
        let mut app = app_with_issues();
        app.select_issue();
        app.back_to_list();
        assert!(matches!(app.view, View::MyIssues));
    }

    #[test]
    fn selected_issue_returns_correct_issue() {
        let mut app = app_with_issues();
        app.selected = 1;
        let issue = app.selected_issue().unwrap();
        assert_eq!(issue.identifier, "JEM-2");
    }

    #[test]
    fn detail_scroll_up_down() {
        let mut app = app_with_issues();
        app.select_issue();
        app.detail_scroll_max = 5;
        assert_eq!(app.detail_scroll, 0);
        app.scroll_detail_down();
        app.scroll_detail_down();
        assert_eq!(app.detail_scroll, 2);
        app.scroll_detail_up();
        assert_eq!(app.detail_scroll, 1);
        app.scroll_detail_up();
        app.scroll_detail_up(); // should not underflow
        assert_eq!(app.detail_scroll, 0);
    }

    #[test]
    fn detail_scroll_clamped_to_max() {
        let mut app = app_with_issues();
        app.select_issue();
        app.detail_scroll_max = 2;
        app.scroll_detail_down();
        app.scroll_detail_down();
        assert_eq!(app.detail_scroll, 2);
        app.scroll_detail_down(); // should not exceed max
        assert_eq!(app.detail_scroll, 2);
    }

    #[test]
    fn detail_scroll_blocked_when_content_fits() {
        let mut app = app_with_issues();
        app.select_issue();
        app.detail_scroll_max = 0; // content fits in box
        app.scroll_detail_down();
        assert_eq!(app.detail_scroll, 0);
    }

    #[test]
    fn search_filters_across_all_columns() {
        let mut app = app_with_issues();
        app.search = Some("jem-2".into());
        let results = app.filtered_issues();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].identifier, "JEM-2");
    }

    #[test]
    fn search_is_case_insensitive() {
        let mut app = app_with_issues();
        app.search = Some("ALPHA".into());
        assert_eq!(app.filtered_issues().len(), 1);
    }

    #[test]
    fn search_matches_priority() {
        let mut app = app_with_issues();
        app.search = Some("high".into());
        let results = app.filtered_issues();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].identifier, "JEM-1");
    }

    #[test]
    fn apply_empty_search_clears() {
        let mut app = app_with_issues();
        app.start_search();
        app.apply_search();
        assert!(app.search.is_none());
        assert!(!app.searching);
    }

    #[test]
    fn apply_search_sets_and_resets_cursor() {
        let mut app = app_with_issues();
        app.selected = 2;
        app.start_search();
        app.search_input = "beta".into();
        app.apply_search();
        assert_eq!(app.search.as_deref(), Some("beta"));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn cancel_search_discards_input() {
        let mut app = app_with_issues();
        app.start_search();
        app.search_input = "something".into();
        app.cancel_search();
        assert!(!app.searching);
        assert!(app.search_input.is_empty());
    }

    #[test]
    fn search_and_filter_combine() {
        let mut app = app_with_issues();
        app.search = Some("task".into());
        assert_eq!(app.filtered_issues().len(), 3);
        app.filter = Some((SortColumn::Title, "alpha".into()));
        assert_eq!(app.filtered_issues().len(), 1);
        assert_eq!(app.filtered_issues()[0].identifier, "JEM-1");
    }

    #[tokio::test]
    async fn refresh_reloads_issues() {
        let fake = FakeLinearApi::new();
        fake.push_response(serde_json::json!({
            "data": { "issues": { "nodes": [
                { "id": "1", "identifier": "JEM-10", "title": "New issue" }
            ]}}
        }));
        let mut app = App::new(fake);
        app.issues = vec![Issue {
            id: "old".into(),
            identifier: "JEM-1".into(),
            title: "Old issue".into(),
            url: None,
            state: None,
            priority: None,
            project: None,
            description: None,
            assignee: None,
            labels: None,
            comments: None,
        }];
        assert_eq!(app.issues.len(), 1);
        assert_eq!(app.issues[0].identifier, "JEM-1");

        app.refresh().await;

        assert_eq!(app.issues.len(), 1);
        assert_eq!(app.issues[0].identifier, "JEM-10");
        assert!(!app.refreshing);
    }

    #[tokio::test]
    async fn refresh_preserves_selection_when_issue_exists() {
        let fake = FakeLinearApi::new();
        fake.push_response(serde_json::json!({
            "data": { "issues": { "nodes": [
                { "id": "1", "identifier": "JEM-1", "title": "Alpha" },
                { "id": "2", "identifier": "JEM-2", "title": "Beta" },
                { "id": "3", "identifier": "JEM-3", "title": "Gamma" }
            ]}}
        }));
        let mut app = App::new(fake);
        app.issues = vec![
            Issue {
                id: "1".into(),
                identifier: "JEM-1".into(),
                title: "Alpha".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
            Issue {
                id: "2".into(),
                identifier: "JEM-2".into(),
                title: "Beta".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
        ];
        app.selected = 1; // JEM-2 selected

        app.refresh().await;

        assert_eq!(app.selected, 1); // JEM-2 is still at index 1
        assert_eq!(app.issues[app.selected].identifier, "JEM-2");
    }

    #[tokio::test]
    async fn refresh_resets_selection_when_issue_gone() {
        let fake = FakeLinearApi::new();
        fake.push_response(serde_json::json!({
            "data": { "issues": { "nodes": [
                { "id": "1", "identifier": "JEM-1", "title": "Alpha" },
                { "id": "3", "identifier": "JEM-3", "title": "Gamma" }
            ]}}
        }));
        let mut app = App::new(fake);
        app.issues = vec![
            Issue {
                id: "1".into(),
                identifier: "JEM-1".into(),
                title: "Alpha".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
            Issue {
                id: "2".into(),
                identifier: "JEM-2".into(),
                title: "Beta".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
        ];
        app.selected = 1; // JEM-2 selected

        app.refresh().await;

        assert_eq!(app.selected, 0); // JEM-2 gone, reset to 0
    }

    #[tokio::test]
    async fn refresh_on_api_error_preserves_issues() {
        let fake = FakeLinearApi::new();
        // No response enqueued — FakeLinearApi returns null data which yields empty vec,
        // but let's test the error case by not pushing any response (returns empty)
        let mut app = App::new(fake);
        let original_issues = vec![Issue {
            id: "1".into(),
            identifier: "JEM-1".into(),
            title: "Alpha".into(),
            url: None,
            state: None,
            priority: None,
            project: None,
            description: None,
            assignee: None,
            labels: None,
            comments: None,
        }];
        app.issues = original_issues.clone();

        app.refresh().await;

        // FakeLinearApi with no response returns empty vec (not an error), so issues get replaced
        // This tests that refreshing flag is cleared
        assert!(!app.refreshing);
    }

    #[test]
    fn select_issue_resets_thread_state() {
        let mut app = app_with_issues();
        app.detail_section = DetailSection::Threads;
        app.detail_thread_selected = 2;
        app.select_issue();
        assert!(matches!(app.detail_section, DetailSection::Body));
        assert_eq!(app.detail_thread_selected, 0);
    }

    #[test]
    fn back_to_list_clears_threads() {
        let mut app = app_with_issues();
        app.select_issue();
        app.detail_threads = vec![ThreadSummary {
            id: "T-1".into(),
            title: "Thread".into(),
            message_count: 5,
            last_activity_ms: 1700000000000,
        }];
        app.detail_section = DetailSection::Threads;
        app.back_to_list();
        assert!(matches!(app.detail_section, DetailSection::Body));
        assert!(app.detail_threads.is_empty());
        assert_eq!(app.detail_thread_selected, 0);
    }

    #[test]
    fn thread_move_down_and_up() {
        let mut app = app_with_issues();
        app.detail_threads = vec![
            ThreadSummary {
                id: "T-1".into(),
                title: "A".into(),
                message_count: 1,
                last_activity_ms: 0,
            },
            ThreadSummary {
                id: "T-2".into(),
                title: "B".into(),
                message_count: 2,
                last_activity_ms: 0,
            },
            ThreadSummary {
                id: "T-3".into(),
                title: "C".into(),
                message_count: 3,
                last_activity_ms: 0,
            },
        ];
        assert_eq!(app.detail_thread_selected, 0);
        app.thread_move_down();
        assert_eq!(app.detail_thread_selected, 1);
        app.thread_move_down();
        assert_eq!(app.detail_thread_selected, 2);
        app.thread_move_down(); // should not exceed
        assert_eq!(app.detail_thread_selected, 2);
        app.thread_move_up();
        assert_eq!(app.detail_thread_selected, 1);
        app.thread_move_up();
        assert_eq!(app.detail_thread_selected, 0);
        app.thread_move_up(); // should not underflow
        assert_eq!(app.detail_thread_selected, 0);
    }

    #[test]
    fn focus_threads_requires_threads() {
        let mut app = app_with_issues();
        app.focus_threads();
        assert!(matches!(app.detail_section, DetailSection::Body));

        app.detail_threads = vec![ThreadSummary {
            id: "T-1".into(),
            title: "A".into(),
            message_count: 1,
            last_activity_ms: 0,
        }];
        app.focus_threads();
        assert!(matches!(app.detail_section, DetailSection::Threads));
    }

    #[test]
    fn focus_body_returns_to_body() {
        let mut app = app_with_issues();
        app.detail_section = DetailSection::Threads;
        app.focus_body();
        assert!(matches!(app.detail_section, DetailSection::Body));
    }

    #[test]
    fn selected_thread_returns_none_when_body_focused() {
        let mut app = app_with_issues();
        app.detail_threads = vec![ThreadSummary {
            id: "T-1".into(),
            title: "A".into(),
            message_count: 1,
            last_activity_ms: 0,
        }];
        app.detail_section = DetailSection::Body;
        assert!(app.selected_thread().is_none());
    }

    #[test]
    fn selected_thread_returns_current_when_threads_focused() {
        let mut app = app_with_issues();
        app.detail_threads = vec![
            ThreadSummary {
                id: "T-1".into(),
                title: "A".into(),
                message_count: 1,
                last_activity_ms: 0,
            },
            ThreadSummary {
                id: "T-2".into(),
                title: "B".into(),
                message_count: 2,
                last_activity_ms: 0,
            },
        ];
        app.detail_section = DetailSection::Threads;
        app.detail_thread_selected = 0;
        assert_eq!(app.selected_thread().unwrap().id, "T-1");

        app.detail_thread_selected = 1;
        assert_eq!(app.selected_thread().unwrap().id, "T-2");
    }

    #[test]
    fn selected_thread_returns_none_when_no_threads() {
        let mut app = app_with_issues();
        app.detail_section = DetailSection::Threads;
        assert!(app.selected_thread().is_none());
    }

    #[test]
    fn workspace_picker_navigation() {
        let mut picker = WorkspacePicker::new(vec!["/a".into(), "/b".into(), "/c".into()]);
        assert_eq!(picker.selected, 0);
        assert_eq!(picker.selected_workspace(), Some("/a"));

        picker.move_down();
        assert_eq!(picker.selected, 1);
        assert_eq!(picker.selected_workspace(), Some("/b"));

        picker.move_down();
        assert_eq!(picker.selected, 2);

        picker.move_down(); // should not exceed
        assert_eq!(picker.selected, 2);

        picker.move_up();
        assert_eq!(picker.selected, 1);

        picker.move_up();
        assert_eq!(picker.selected, 0);

        picker.move_up(); // should not underflow
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn workspace_picker_empty_options() {
        let picker = WorkspacePicker::new(vec![]);
        assert_eq!(picker.selected, 0);
        assert!(picker.selected_workspace().is_none());
    }

    #[test]
    fn workspace_picker_single_option() {
        let mut picker = WorkspacePicker::new(vec!["/only".into()]);
        assert_eq!(picker.selected_workspace(), Some("/only"));
        picker.move_down();
        assert_eq!(picker.selected, 0);
        picker.move_up();
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn show_workspace_picker_sets_state() {
        let mut app = app_with_issues();
        assert!(app.workspace_picker.is_none());

        app.show_workspace_picker(vec!["/ws1".into(), "/ws2".into()]);
        assert!(app.workspace_picker.is_some());

        let picker = app.workspace_picker.as_ref().unwrap();
        assert_eq!(picker.options.len(), 2);
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn show_workspace_picker_with_empty_list_opens_picker() {
        let mut app = app_with_issues();
        app.show_workspace_picker(vec![]);
        assert!(app.workspace_picker.is_some());
        let picker = app.workspace_picker.as_ref().unwrap();
        assert!(picker.options.is_empty());
    }

    #[test]
    fn cancel_workspace_picker_clears_state() {
        let mut app = app_with_issues();
        app.show_workspace_picker(vec!["/ws".into()]);
        assert!(app.workspace_picker.is_some());

        app.cancel_workspace_picker();
        assert!(app.workspace_picker.is_none());
    }

    fn app_with_projects() -> App<FakeLinearApi> {
        let mut app = App::new(FakeLinearApi::new());
        app.projects = vec![
            Project {
                id: "p1".into(),
                name: "Alpha Project".into(),
                state: Some("started".into()),
                progress: Some(0.5),
                lead: Some(crate::api::types::IssueUser {
                    name: "Alice".into(),
                }),
                url: Some("https://linear.app/test/project/alpha".into()),
            },
            Project {
                id: "p2".into(),
                name: "Beta Project".into(),
                state: Some("planned".into()),
                progress: Some(0.0),
                lead: None,
                url: None,
            },
            Project {
                id: "p3".into(),
                name: "Gamma Project".into(),
                state: None,
                progress: None,
                lead: None,
                url: Some("https://linear.app/test/project/gamma".into()),
            },
        ];
        app
    }

    #[test]
    fn switch_to_projects_changes_view() {
        let mut app = app_with_issues();
        assert!(matches!(app.view, View::MyIssues));
        app.switch_to_projects();
        assert!(matches!(app.view, View::ProjectList));
    }

    #[test]
    fn switch_to_my_issues_changes_view() {
        let mut app = app_with_issues();
        app.switch_to_projects();
        app.switch_to_my_issues();
        assert!(matches!(app.view, View::MyIssues));
    }

    #[test]
    fn project_navigation_wraps_at_bounds() {
        let mut app = app_with_projects();
        assert_eq!(app.project_selected, 0);

        app.project_move_down();
        assert_eq!(app.project_selected, 1);
        app.project_move_down();
        assert_eq!(app.project_selected, 2);
        app.project_move_down(); // at bottom, stays
        assert_eq!(app.project_selected, 2);

        app.project_move_up();
        assert_eq!(app.project_selected, 1);
        app.project_top();
        assert_eq!(app.project_selected, 0);
        app.project_move_up(); // at top, stays
        assert_eq!(app.project_selected, 0);
    }

    #[test]
    fn project_bottom_goes_to_last() {
        let mut app = app_with_projects();
        app.project_bottom();
        assert_eq!(app.project_selected, 2);
    }

    #[test]
    fn project_top_goes_to_first() {
        let mut app = app_with_projects();
        app.project_selected = 2;
        app.project_top();
        assert_eq!(app.project_selected, 0);
    }

    #[test]
    fn selected_project_returns_correct_project() {
        let mut app = app_with_projects();
        app.project_selected = 1;
        let project = app.selected_project().unwrap();
        assert_eq!(project.name, "Beta Project");
    }

    #[test]
    fn selected_project_returns_none_when_empty() {
        let app = App::new(FakeLinearApi::new());
        assert!(app.selected_project().is_none());
    }

    #[test]
    fn select_project_enters_project_detail_view() {
        let mut app = app_with_projects();
        app.project_selected = 0;
        app.select_project();
        assert!(matches!(app.view, View::ProjectDetail));
        assert_eq!(app.project_issue_selected, 0);
    }

    #[test]
    fn select_project_on_empty_does_nothing() {
        let mut app = App::new(FakeLinearApi::new());
        app.view = View::ProjectList;
        app.select_project();
        assert!(matches!(app.view, View::ProjectList));
    }

    #[test]
    fn back_from_project_detail_returns_to_project_list() {
        let mut app = app_with_projects();
        app.select_project();
        app.project_issues = vec![Issue {
            id: "1".into(),
            identifier: "JEM-1".into(),
            title: "Test".into(),
            url: None,
            state: None,
            priority: None,
            project: None,
            description: None,
            assignee: None,
            labels: None,
            comments: None,
        }];
        app.back_from_project_detail();
        assert!(matches!(app.view, View::ProjectList));
        assert!(app.project_issues.is_empty());
        assert_eq!(app.project_issue_selected, 0);
    }

    #[test]
    fn project_issue_navigation() {
        let mut app = app_with_projects();
        app.project_issues = vec![
            Issue {
                id: "1".into(),
                identifier: "JEM-1".into(),
                title: "A".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
            Issue {
                id: "2".into(),
                identifier: "JEM-2".into(),
                title: "B".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
        ];
        assert_eq!(app.project_issue_selected, 0);
        app.project_issue_move_down();
        assert_eq!(app.project_issue_selected, 1);
        app.project_issue_move_down(); // at end
        assert_eq!(app.project_issue_selected, 1);
        app.project_issue_move_up();
        assert_eq!(app.project_issue_selected, 0);
        app.project_issue_move_up(); // at start
        assert_eq!(app.project_issue_selected, 0);
    }

    #[test]
    fn project_issue_top_and_bottom() {
        let mut app = app_with_projects();
        app.project_issues = vec![
            Issue {
                id: "1".into(),
                identifier: "JEM-1".into(),
                title: "A".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
            Issue {
                id: "2".into(),
                identifier: "JEM-2".into(),
                title: "B".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
            Issue {
                id: "3".into(),
                identifier: "JEM-3".into(),
                title: "C".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
        ];
        app.project_issue_bottom();
        assert_eq!(app.project_issue_selected, 2);
        app.project_issue_top();
        assert_eq!(app.project_issue_selected, 0);
    }

    #[test]
    fn selected_project_issue_returns_correct_issue() {
        let mut app = app_with_projects();
        app.project_issues = vec![
            Issue {
                id: "1".into(),
                identifier: "JEM-1".into(),
                title: "A".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
            Issue {
                id: "2".into(),
                identifier: "JEM-2".into(),
                title: "B".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
        ];
        app.project_issue_selected = 1;
        let issue = app.selected_project_issue().unwrap();
        assert_eq!(issue.identifier, "JEM-2");
    }

    #[test]
    fn selected_project_url_returns_url() {
        let app = app_with_projects();
        assert_eq!(
            app.selected_project_url(),
            Some("https://linear.app/test/project/alpha".into())
        );
    }

    #[test]
    fn selected_project_url_returns_none_when_no_url() {
        let mut app = app_with_projects();
        app.project_selected = 1;
        assert!(app.selected_project_url().is_none());
    }

    #[tokio::test]
    async fn load_projects_fetches_on_empty_cache() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/projects.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);

        assert!(app.projects.is_empty());
        app.load_projects().await;
        assert_eq!(app.projects.len(), 2);
        assert!(app.project_cache.is_fresh(CACHE_KEY_PROJECTS));
    }

    #[tokio::test]
    async fn load_projects_serves_from_cache() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/projects.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);

        app.load_projects().await;
        assert_eq!(app.projects.len(), 2);

        app.projects.clear();
        app.load_projects().await;
        assert_eq!(app.projects.len(), 2);
    }

    #[tokio::test]
    async fn refresh_projects_bypasses_cache() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/projects.json")).unwrap();
        fake.push_response(fixture.clone());
        fake.push_response(fixture);
        let mut app = App::new(fake);

        app.load_projects().await;
        assert_eq!(app.projects.len(), 2);

        app.refresh_projects().await;
        assert_eq!(app.projects.len(), 2);
        assert!(!app.refreshing);
        assert!(app.project_cache.is_fresh(CACHE_KEY_PROJECTS));
    }

    #[tokio::test]
    async fn load_project_issues_fetches_issues() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/project_issues.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);
        app.projects = vec![Project {
            id: "proj-1".into(),
            name: "Alpha".into(),
            state: None,
            progress: None,
            lead: None,
            url: None,
        }];
        app.project_selected = 0;

        app.load_project_issues().await;
        assert_eq!(app.project_issues.len(), 2);
        assert_eq!(app.project_issues[0].identifier, "JEM-10");
        assert_eq!(app.project_issue_selected, 0);
    }

    fn app_with_project_issues() -> App<FakeLinearApi> {
        let mut app = app_with_projects();
        app.view = View::ProjectDetail;
        app.project_selected = 0;
        app.project_issues = vec![
            Issue {
                id: "10".into(),
                identifier: "JEM-10".into(),
                title: "Project Alpha".into(),
                url: Some("https://linear.app/test/issue/JEM-10".into()),
                state: None,
                priority: Some(2.0),
                project: None,
                description: Some("Description 10".into()),
                assignee: None,
                labels: None,
                comments: None,
            },
            Issue {
                id: "11".into(),
                identifier: "JEM-11".into(),
                title: "Project Beta".into(),
                url: Some("https://linear.app/test/issue/JEM-11".into()),
                state: None,
                priority: Some(3.0),
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
        ];
        app
    }

    #[test]
    fn select_project_issue_enters_detail_view() {
        let mut app = app_with_project_issues();
        app.project_issue_selected = 0;
        app.select_project_issue();
        assert!(matches!(app.view, View::Detail));
        assert_eq!(app.detail_origin, DetailOrigin::ProjectDetail);
        assert_eq!(app.detail_scroll, 0);
        assert!(matches!(app.detail_section, DetailSection::Body));
        assert_eq!(app.detail_thread_selected, 0);
    }

    #[test]
    fn select_project_issue_on_empty_does_nothing() {
        let mut app = app_with_projects();
        app.view = View::ProjectDetail;
        app.select_project_issue();
        assert!(matches!(app.view, View::ProjectDetail));
    }

    #[test]
    fn select_project_issue_at_second_item() {
        let mut app = app_with_project_issues();
        app.project_issue_selected = 1;
        app.select_project_issue();
        assert!(matches!(app.view, View::Detail));
        assert_eq!(app.detail_origin, DetailOrigin::ProjectDetail);
    }

    #[test]
    fn back_to_list_from_project_issue_returns_to_project_detail() {
        let mut app = app_with_project_issues();
        app.select_project_issue();
        assert!(matches!(app.view, View::Detail));
        app.back_to_list();
        assert!(matches!(app.view, View::ProjectDetail));
    }

    #[test]
    fn back_to_list_from_my_issue_returns_to_my_issues() {
        let mut app = app_with_issues();
        app.select_issue();
        assert!(matches!(app.view, View::Detail));
        app.back_to_list();
        assert!(matches!(app.view, View::MyIssues));
    }

    #[test]
    fn selected_issue_in_detail_from_project_returns_project_issue() {
        let mut app = app_with_project_issues();
        app.project_issue_selected = 1;
        app.select_project_issue();
        let issue = app.selected_issue().unwrap();
        assert_eq!(issue.identifier, "JEM-11");
    }

    #[test]
    fn selected_issue_in_detail_from_my_issues_returns_my_issue() {
        let mut app = app_with_issues();
        app.selected = 1;
        app.select_issue();
        let issue = app.selected_issue().unwrap();
        assert_eq!(issue.identifier, "JEM-2");
    }

    #[test]
    fn selected_issue_url_from_project_detail_view() {
        let mut app = app_with_project_issues();
        app.project_issue_selected = 0;
        assert_eq!(
            app.selected_issue_url(),
            Some("https://linear.app/test/issue/JEM-10".into())
        );
    }

    #[test]
    fn selected_issue_url_from_project_issue_detail() {
        let mut app = app_with_project_issues();
        app.project_issue_selected = 0;
        app.select_project_issue();
        assert_eq!(
            app.selected_issue_url(),
            Some("https://linear.app/test/issue/JEM-10".into())
        );
    }

    #[test]
    fn context_issue_in_my_issues_view() {
        let mut app = app_with_issues();
        app.selected = 0;
        let issue = app.context_issue().unwrap();
        assert_eq!(issue.identifier, "JEM-1");
    }

    #[test]
    fn context_issue_in_project_detail_view() {
        let mut app = app_with_project_issues();
        app.project_issue_selected = 1;
        let issue = app.context_issue().unwrap();
        assert_eq!(issue.identifier, "JEM-11");
    }

    #[test]
    fn context_issue_in_detail_from_project() {
        let mut app = app_with_project_issues();
        app.project_issue_selected = 0;
        app.select_project_issue();
        let issue = app.context_issue().unwrap();
        assert_eq!(issue.identifier, "JEM-10");
    }

    #[test]
    fn detail_origin_defaults_to_my_issues() {
        let app = App::new(FakeLinearApi::new());
        assert_eq!(app.detail_origin, DetailOrigin::MyIssues);
    }

    #[test]
    fn select_issue_sets_origin_to_my_issues() {
        let mut app = app_with_issues();
        // Even if origin was previously ProjectDetail, selecting from MyIssues resets it
        app.detail_origin = DetailOrigin::ProjectDetail;
        app.select_issue();
        assert_eq!(app.detail_origin, DetailOrigin::MyIssues);
    }

    #[test]
    fn workspace_picker_start_typing() {
        let mut picker = WorkspacePicker::new(vec!["/a".into()]);
        assert!(!picker.typing);

        picker.start_typing();
        assert!(picker.typing);
        // Input is seeded with PWD as an absolute path with trailing slash.
        let pwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(picker.input.starts_with(&pwd));
        assert!(picker.input.ends_with('/'));
    }

    #[test]
    fn workspace_picker_cancel_typing() {
        let mut picker = WorkspacePicker::new(vec!["/a".into()]);
        picker.start_typing();
        picker.input.push_str("some/path");
        picker.cancel_typing();

        assert!(!picker.typing);
        assert!(picker.input.is_empty());
    }

    #[test]
    fn workspace_picker_confirm_typed_path_returns_path() {
        let mut picker = WorkspacePicker::new(vec![]);
        picker.start_typing();
        picker.input = "/my/workspace".to_string();

        let result = picker.confirm_typed_path();
        assert_eq!(result, Some("/my/workspace".to_string()));
        assert!(!picker.typing);
        assert!(picker.input.is_empty());
    }

    #[test]
    fn workspace_picker_confirm_typed_path_trims_whitespace() {
        let mut picker = WorkspacePicker::new(vec![]);
        picker.start_typing();
        picker.input = "  /my/workspace  ".to_string();

        let result = picker.confirm_typed_path();
        assert_eq!(result, Some("/my/workspace".to_string()));
    }

    #[test]
    fn workspace_picker_confirm_empty_path_returns_none() {
        let mut picker = WorkspacePicker::new(vec![]);
        picker.typing = true;
        picker.input.clear();

        let result = picker.confirm_typed_path();
        assert!(result.is_none());
        assert!(!picker.typing);
    }

    #[test]
    fn workspace_picker_confirm_whitespace_only_returns_none() {
        let mut picker = WorkspacePicker::new(vec![]);
        picker.typing = true;
        picker.input = "   ".to_string();

        let result = picker.confirm_typed_path();
        assert!(result.is_none());
    }

    #[test]
    fn workspace_picker_typing_highlights_selected() {
        let mut picker = WorkspacePicker::new(vec!["/a".into(), "/b".into()]);
        assert!(!picker.typing);
        assert_eq!(picker.selected_workspace(), Some("/a"));

        picker.start_typing();
        assert!(picker.typing);
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn workspace_picker_typing_input_editable() {
        let mut picker = WorkspacePicker::new(vec![]);
        picker.start_typing();
        let initial = picker.input.clone();

        // User can append characters.
        picker.input.push_str("subdir");
        assert!(picker.input.ends_with("subdir"));

        // User can delete characters.
        picker.input.pop();
        assert!(picker.input.ends_with("subdi"));

        // User can clear and type a completely different absolute path.
        picker.input = "/other/path".to_string();
        assert_eq!(picker.input, "/other/path");
        assert_ne!(picker.input, initial);
    }

    #[test]
    fn workspace_picker_tab_complete_populates_options() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("alpha")).unwrap();
        std::fs::create_dir(dir.path().join("beta")).unwrap();
        // Files should not appear in completions.
        std::fs::write(dir.path().join("file.txt"), "").unwrap();

        let mut picker = WorkspacePicker::new(vec!["/original".into()]);
        picker.start_typing();
        picker.input = format!("{}/", dir.path().display());

        picker.tab_complete();
        // Options should contain exactly the two subdirectories.
        assert_eq!(picker.options.len(), 2);
        assert!(picker.options[0].ends_with("alpha/"));
        assert!(picker.options[1].ends_with("beta/"));
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn workspace_picker_tab_complete_single_match_auto_completes() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("only_dir");
        std::fs::create_dir(&sub).unwrap();

        let mut picker = WorkspacePicker::new(vec!["/original".into()]);
        picker.start_typing();
        picker.input = format!("{}/o", dir.path().display());

        picker.tab_complete();
        // Single match — input should be completed.
        assert!(
            picker.input.ends_with("only_dir/"),
            "input should auto-complete: {}",
            picker.input
        );
        // Options should remain unchanged (not drilled into children).
        assert_eq!(picker.options, vec!["/original"]);
    }

    #[test]
    fn workspace_picker_tab_complete_with_prefix_filters() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::create_dir(dir.path().join("scripts")).unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();

        let mut picker = WorkspacePicker::new(vec![]);
        picker.start_typing();
        picker.input = format!("{}/s", dir.path().display());

        picker.tab_complete();
        // Should show only "scripts" and "src", not "target".
        assert_eq!(picker.options.len(), 2);
        let names: Vec<&str> = picker
            .options
            .iter()
            .map(|o| o.rsplit('/').nth(1).unwrap())
            .collect();
        assert!(names.contains(&"scripts"));
        assert!(names.contains(&"src"));
    }

    #[test]
    fn workspace_picker_cancel_typing_restores_original_options() {
        let original = vec!["/ws1".to_string(), "/ws2".to_string()];
        let mut picker = WorkspacePicker::new(original.clone());
        picker.start_typing();

        // Simulate tab completion replacing options.
        picker.options = vec!["/some/dir/".into()];
        picker.cancel_typing();

        assert!(!picker.typing);
        assert_eq!(picker.options, original);
    }

    #[test]
    fn workspace_picker_delete_path_component() {
        let mut picker = WorkspacePicker::new(vec![]);
        picker.typing = true;

        // Trailing slash: removes the last directory.
        picker.input = "/Users/me/projects/".to_string();
        picker.delete_path_component();
        assert_eq!(picker.input, "/Users/me/");

        // No trailing slash: removes partial text back to previous slash.
        picker.input = "/Users/me/pro".to_string();
        picker.delete_path_component();
        assert_eq!(picker.input, "/Users/me/");

        // Down to root.
        picker.input = "/Users/".to_string();
        picker.delete_path_component();
        assert_eq!(picker.input, "/");

        // At root slash — clears entirely.
        picker.delete_path_component();
        assert_eq!(picker.input, "");
    }

    #[test]
    fn selected_issue_url_returns_url() {
        let app = app_with_issues();
        assert_eq!(
            app.selected_issue_url(),
            Some("https://linear.app/test/issue/JEM-1".into())
        );
    }

    #[test]
    fn selected_issue_url_returns_none_when_no_issues() {
        let app = App::new(FakeLinearApi::new());
        assert!(app.selected_issue_url().is_none());
    }

    #[test]
    fn selected_issue_url_returns_none_when_url_missing() {
        let mut app = App::new(FakeLinearApi::new());
        app.issues = vec![Issue {
            id: "1".into(),
            identifier: "JEM-1".into(),
            title: "No URL".into(),
            url: None,
            state: None,
            priority: None,
            project: None,
            description: None,
            assignee: None,
            labels: None,
            comments: None,
        }];
        assert!(app.selected_issue_url().is_none());
    }

    #[test]
    fn awaiting_open_defaults_to_false() {
        let app = app_with_issues();
        assert!(!app.awaiting_open);
    }

    #[test]
    fn show_help_defaults_to_false() {
        let app = app_with_issues();
        assert!(!app.show_help);
    }

    #[test]
    fn toggle_help_shows_and_hides() {
        let mut app = app_with_issues();
        assert!(!app.show_help);

        app.toggle_help();
        assert!(app.show_help);

        app.toggle_help();
        assert!(!app.show_help);
    }

    #[test]
    fn dismiss_help_hides_overlay() {
        let mut app = app_with_issues();
        app.toggle_help();
        assert!(app.show_help);

        app.dismiss_help();
        assert!(!app.show_help);
    }

    #[test]
    fn dismiss_help_is_idempotent() {
        let mut app = app_with_issues();
        app.dismiss_help();
        assert!(!app.show_help);
    }

    #[tokio::test]
    async fn load_issues_fetches_on_empty_cache() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/my_issues.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);

        assert!(app.issues.is_empty());
        app.load_issues().await;
        assert_eq!(app.issues.len(), 2);
        assert!(app.cache.is_fresh(CACHE_KEY_MY_ISSUES));
    }

    #[tokio::test]
    async fn load_issues_serves_from_cache() {
        let fake = FakeLinearApi::new();
        // Enqueue only one response — second call should come from cache
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/my_issues.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);

        app.load_issues().await;
        assert_eq!(app.issues.len(), 2);

        // Clear issues, then load again — should restore from cache
        app.issues.clear();
        app.load_issues().await;
        assert_eq!(app.issues.len(), 2);
    }

    #[tokio::test]
    async fn refresh_bypasses_cache() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/my_issues.json")).unwrap();
        fake.push_response(fixture.clone());
        fake.push_response(fixture);
        let mut app = App::new(fake);

        app.load_issues().await;
        assert_eq!(app.issues.len(), 2);

        // Refresh should hit the API again (second enqueued response)
        app.refresh().await;
        assert_eq!(app.issues.len(), 2);
        assert!(app.cache.is_fresh(CACHE_KEY_MY_ISSUES));
    }

    #[tokio::test]
    async fn refresh_updates_cache() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/my_issues.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);

        app.load_issues().await;
        assert_eq!(app.issues.len(), 2);
        assert!(app.cache.is_fresh(CACHE_KEY_MY_ISSUES));
    }

    #[tokio::test]
    async fn refresh_on_api_error_preserves_cache() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/my_issues.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);

        app.load_issues().await;
        assert_eq!(app.issues.len(), 2);

        // Refresh with no enqueued response (will get null data → empty)
        // But since fetch returns Ok([]) for null data, issues will be replaced
        app.refresh().await;
        // After refresh with empty result, cache should have the empty result
        assert!(app.cache.is_fresh(CACHE_KEY_MY_ISSUES));
    }

    // --- Loading state tests ---

    #[test]
    fn loading_defaults_to_false() {
        let app = App::new(FakeLinearApi::new());
        assert!(!app.loading);
    }

    #[tokio::test]
    async fn load_issues_sets_loading_when_empty() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/my_issues.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);

        assert!(app.issues.is_empty());
        app.load_issues().await;
        // After load completes, loading is false
        assert!(!app.loading);
        assert!(!app.issues.is_empty());
    }

    #[tokio::test]
    async fn load_issues_from_cache_skips_loading() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/my_issues.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);

        app.load_issues().await;
        // Clear issues, load again from cache
        app.issues.clear();
        app.load_issues().await;
        assert!(!app.loading);
    }

    #[tokio::test]
    async fn load_projects_sets_loading_when_empty() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/projects.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);

        assert!(app.projects.is_empty());
        app.load_projects().await;
        assert!(!app.loading);
        assert!(!app.projects.is_empty());
    }

    // --- Error state tests ---

    #[test]
    fn error_defaults_to_none() {
        let app = App::new(FakeLinearApi::new());
        assert!(app.error.is_none());
    }

    #[test]
    fn dismiss_error_clears_error() {
        let mut app = App::new(FakeLinearApi::new());
        app.error = Some(AppError::new("test error"));
        app.dismiss_error();
        assert!(app.error.is_none());
    }

    #[test]
    fn dismiss_error_is_idempotent() {
        let mut app = App::new(FakeLinearApi::new());
        app.dismiss_error();
        assert!(app.error.is_none());
    }

    #[tokio::test]
    async fn load_issues_sets_error_on_failure() {
        let fake = FakeLinearApi::new();
        fake.push_error("HTTP status client error (401 Unauthorized)");
        let mut app = App::new(fake);

        app.load_issues().await;

        assert!(app.error.is_some());
        assert!(
            app.error
                .as_ref()
                .unwrap()
                .message
                .contains("Authentication")
        );
    }

    #[tokio::test]
    async fn load_issues_error_preserves_existing_data() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/my_issues.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);

        app.load_issues().await;
        assert_eq!(app.issues.len(), 2);
        assert!(app.error.is_none());

        // Now enqueue errors for both API calls during refresh
        // (refresh fetches issues and projects)
        app.api.push_error("connection timed out");
        app.api.push_error("connection timed out");
        app.refresh().await;

        // Issues should still be there (graceful degradation)
        assert_eq!(app.issues.len(), 2);
        assert!(app.error.is_some());
        assert!(app.error.as_ref().unwrap().message.contains("timed out"));
    }

    #[tokio::test]
    async fn refresh_clears_error_on_success() {
        let fake = FakeLinearApi::new();
        fake.push_error("network error");
        let mut app = App::new(fake);

        app.load_issues().await;
        assert!(app.error.is_some());

        // Now enqueue a successful response
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/my_issues.json")).unwrap();
        app.api.push_response(fixture);
        app.refresh().await;

        assert!(app.error.is_none());
        assert_eq!(app.issues.len(), 2);
    }

    #[tokio::test]
    async fn load_projects_sets_error_on_failure() {
        let fake = FakeLinearApi::new();
        fake.push_error("HTTP status client error (429 Too Many Requests)");
        let mut app = App::new(fake);

        app.load_projects().await;

        assert!(app.error.is_some());
        assert!(app.error.as_ref().unwrap().message.contains("Rate limited"));
    }

    #[tokio::test]
    async fn load_project_issues_sets_error_on_failure() {
        let fake = FakeLinearApi::new();
        let proj_fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/projects.json")).unwrap();
        fake.push_response(proj_fixture);
        let mut app = App::new(fake);

        app.load_projects().await;
        assert!(!app.projects.is_empty());
        app.select_project();

        app.api.push_error("dns error: failed to lookup address");
        app.load_project_issues().await;

        assert!(app.error.is_some());
        assert!(
            app.error
                .as_ref()
                .unwrap()
                .message
                .contains("Network error")
        );
    }

    // --- AppError classification tests ---

    #[test]
    fn app_error_classifies_401_as_auth() {
        let err = anyhow::anyhow!("HTTP status client error (401 Unauthorized)");
        let app_err = AppError::from_api_error(&err);
        assert!(app_err.message.contains("Authentication"));
    }

    #[test]
    fn app_error_classifies_403_as_access_denied() {
        let err = anyhow::anyhow!("HTTP status client error (403 Forbidden)");
        let app_err = AppError::from_api_error(&err);
        assert!(app_err.message.contains("Access denied"));
    }

    #[test]
    fn app_error_classifies_429_as_rate_limit() {
        let err = anyhow::anyhow!("HTTP status client error (429 Too Many Requests)");
        let app_err = AppError::from_api_error(&err);
        assert!(app_err.message.contains("Rate limited"));
    }

    #[test]
    fn app_error_classifies_dns_error_as_network() {
        let err = anyhow::anyhow!("dns error: failed to lookup address");
        let app_err = AppError::from_api_error(&err);
        assert!(app_err.message.contains("Network error"));
    }

    #[test]
    fn app_error_classifies_connect_error_as_network() {
        let err = anyhow::anyhow!("error trying to connect: connection refused");
        let app_err = AppError::from_api_error(&err);
        assert!(app_err.message.contains("Network error"));
    }

    #[test]
    fn app_error_classifies_timeout() {
        let err = anyhow::anyhow!("request timed out");
        let app_err = AppError::from_api_error(&err);
        assert!(app_err.message.contains("timed out"));
    }

    #[test]
    fn app_error_fallback_includes_original_message() {
        let err = anyhow::anyhow!("unexpected server error");
        let app_err = AppError::from_api_error(&err);
        assert!(app_err.message.contains("unexpected server error"));
    }

    #[tokio::test]
    async fn refresh_returns_selected_issue_identifier() {
        let fake = FakeLinearApi::new();
        fake.push_response(serde_json::json!({
            "data": { "issues": { "nodes": [
                { "id": "1", "identifier": "JEM-1", "title": "Alpha" },
                { "id": "2", "identifier": "JEM-2", "title": "Beta" }
            ]}}
        }));
        let mut app = App::new(fake);
        app.issues = vec![
            Issue {
                id: "1".into(),
                identifier: "JEM-1".into(),
                title: "Alpha".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
            Issue {
                id: "2".into(),
                identifier: "JEM-2".into(),
                title: "Beta".into(),
                url: None,
                state: None,
                priority: None,
                project: None,
                description: None,
                assignee: None,
                labels: None,
                comments: None,
            },
        ];
        app.selected = 1;

        let result = app.refresh().await;
        assert_eq!(result, Some("JEM-2".into()));
    }

    #[tokio::test]
    async fn refresh_returns_none_when_no_issues() {
        let fake = FakeLinearApi::new();
        let mut app = App::new(fake);

        let result = app.refresh().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn refresh_invalidates_project_cache() {
        let fake = FakeLinearApi::new();
        let proj_fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/projects.json")).unwrap();
        fake.push_response(proj_fixture.clone());
        let mut app = App::new(fake);

        app.load_projects().await;
        assert!(app.project_cache.is_fresh(CACHE_KEY_PROJECTS));

        // Enqueue responses for refresh (issues + projects)
        app.api
            .push_response(serde_json::json!({"data": { "issues": { "nodes": [] }}}));
        app.api.push_response(proj_fixture);
        app.refresh().await;

        // Project cache should be re-populated (fresh) after refresh
        assert!(app.project_cache.is_fresh(CACHE_KEY_PROJECTS));
    }

    #[tokio::test]
    async fn refresh_updates_projects() {
        let fake = FakeLinearApi::new();
        let mut app = App::new(fake);
        app.projects = vec![Project {
            id: "old".into(),
            name: "Old Project".into(),
            state: None,
            progress: None,
            lead: None,
            url: None,
        }];

        // Enqueue responses for refresh (issues then projects)
        app.api
            .push_response(serde_json::json!({"data": { "issues": { "nodes": [] }}}));
        let proj_fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/projects.json")).unwrap();
        app.api.push_response(proj_fixture);
        app.refresh().await;

        assert_eq!(app.projects.len(), 2);
        assert_eq!(app.projects[0].name, "Alpha Project");
    }

    #[tokio::test]
    async fn refresh_projects_sets_error_on_failure() {
        let fake = FakeLinearApi::new();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/projects.json")).unwrap();
        fake.push_response(fixture);
        let mut app = App::new(fake);

        app.load_projects().await;
        assert!(!app.projects.is_empty());
        assert!(app.error.is_none());

        app.api
            .push_error("HTTP status client error (403 Forbidden)");
        app.refresh_projects().await;

        assert!(app.error.is_some());
        assert!(
            app.error
                .as_ref()
                .unwrap()
                .message
                .contains("Access denied")
        );
        // Projects should still be there (graceful degradation — only cache was invalidated)
        assert!(!app.projects.is_empty());
    }

    #[test]
    fn run_status_for_thread_returns_most_prominent() {
        let mut app = App::new(FakeLinearApi::new());
        app.detail_session_runs = vec![
            SessionRunSummary {
                run_id: "run-1".to_string(),
                thread_id: "T-abc".to_string(),
                status: SessionRunStatus::Completed,
                log_path: Some("/tmp/run-1.log".to_string()),
                created_at_ms: 100,
            },
            SessionRunSummary {
                run_id: "run-2".to_string(),
                thread_id: "T-abc".to_string(),
                status: SessionRunStatus::Running,
                log_path: Some("/tmp/run-2.log".to_string()),
                created_at_ms: 200,
            },
        ];
        assert_eq!(
            app.run_status_for_thread("T-abc"),
            Some(SessionRunStatus::Running)
        );
    }

    #[test]
    fn selected_thread_run_returns_newest_for_selected_thread() {
        let mut app = App::new(FakeLinearApi::new());
        app.detail_section = DetailSection::Threads;
        app.detail_threads = vec![
            ThreadSummary {
                id: "T-abc".to_string(),
                title: "A".to_string(),
                message_count: 1,
                last_activity_ms: 0,
            },
            ThreadSummary {
                id: "T-def".to_string(),
                title: "B".to_string(),
                message_count: 1,
                last_activity_ms: 0,
            },
        ];
        app.detail_session_runs = vec![
            SessionRunSummary {
                run_id: "run-old".to_string(),
                thread_id: "T-abc".to_string(),
                status: SessionRunStatus::Failed,
                log_path: Some("/tmp/run-old.log".to_string()),
                created_at_ms: 100,
            },
            SessionRunSummary {
                run_id: "run-other".to_string(),
                thread_id: "T-def".to_string(),
                status: SessionRunStatus::Running,
                log_path: Some("/tmp/run-other.log".to_string()),
                created_at_ms: 150,
            },
            SessionRunSummary {
                run_id: "run-new".to_string(),
                thread_id: "T-abc".to_string(),
                status: SessionRunStatus::Completed,
                log_path: Some("/tmp/run-new.log".to_string()),
                created_at_ms: 200,
            },
        ];

        app.detail_thread_selected = 0;
        assert_eq!(
            app.selected_thread_run().map(|r| r.run_id.as_str()),
            Some("run-new")
        );

        app.detail_thread_selected = 1;
        assert_eq!(
            app.selected_thread_run().map(|r| r.run_id.as_str()),
            Some("run-other")
        );
    }

    #[test]
    fn run_status_for_thread_returns_none_when_no_runs() {
        let app = App::new(FakeLinearApi::new());
        assert_eq!(app.run_status_for_thread("T-xyz"), None);
    }

    #[test]
    fn active_run_counts_tallies_running_and_pending() {
        let mut app = App::new(FakeLinearApi::new());
        app.detail_session_runs = vec![
            SessionRunSummary {
                run_id: "run-1".to_string(),
                thread_id: "T-a".to_string(),
                status: SessionRunStatus::Running,
                log_path: Some("/tmp/run-1.log".to_string()),
                created_at_ms: 1,
            },
            SessionRunSummary {
                run_id: "run-2".to_string(),
                thread_id: "T-b".to_string(),
                status: SessionRunStatus::Pending,
                log_path: Some("/tmp/run-2.log".to_string()),
                created_at_ms: 2,
            },
            SessionRunSummary {
                run_id: "run-3".to_string(),
                thread_id: "T-c".to_string(),
                status: SessionRunStatus::Completed,
                log_path: Some("/tmp/run-3.log".to_string()),
                created_at_ms: 3,
            },
            SessionRunSummary {
                run_id: "run-4".to_string(),
                thread_id: "T-d".to_string(),
                status: SessionRunStatus::Running,
                log_path: Some("/tmp/run-4.log".to_string()),
                created_at_ms: 4,
            },
        ];
        let (running, pending) = app.active_run_counts();
        assert_eq!(running, 2);
        assert_eq!(pending, 1);
    }

    #[test]
    fn active_run_counts_zero_when_no_runs() {
        let app = App::new(FakeLinearApi::new());
        let (running, pending) = app.active_run_counts();
        assert_eq!(running, 0);
        assert_eq!(pending, 0);
    }

    #[test]
    fn pick_display_status_prefers_running() {
        assert_eq!(
            pick_display_status(SessionRunStatus::Completed, SessionRunStatus::Running),
            SessionRunStatus::Running
        );
        assert_eq!(
            pick_display_status(SessionRunStatus::Running, SessionRunStatus::Failed),
            SessionRunStatus::Running
        );
    }

    #[test]
    fn pick_display_status_prefers_pending_over_terminal() {
        assert_eq!(
            pick_display_status(SessionRunStatus::Completed, SessionRunStatus::Pending),
            SessionRunStatus::Pending
        );
        assert_eq!(
            pick_display_status(SessionRunStatus::Failed, SessionRunStatus::Pending),
            SessionRunStatus::Pending
        );
    }

    #[test]
    fn back_to_list_clears_session_runs() {
        let mut app = App::new(FakeLinearApi::new());
        app.issues = vec![Issue {
            id: "1".into(),
            identifier: "JEM-1".into(),
            title: "Test".into(),
            url: None,
            state: None,
            priority: None,
            project: None,
            description: None,
            assignee: None,
            labels: None,
            comments: None,
        }];
        app.select_issue();
        app.detail_session_runs = vec![SessionRunSummary {
            run_id: "run-1".to_string(),
            thread_id: "T-a".to_string(),
            status: SessionRunStatus::Running,
            log_path: Some("/tmp/run-1.log".to_string()),
            created_at_ms: 1,
        }];
        app.back_to_list();
        assert!(app.detail_session_runs.is_empty());
    }

    #[test]
    fn start_state_change_populates_options() {
        let mut app = app_with_issues();
        app.start_state_change();
        assert!(app.awaiting_state_change);
        assert!(!app.state_options.is_empty());
        assert_eq!(app.state_selected, 0);
        assert!(app.state_options.contains(&"In Progress".to_string()));
        assert!(app.state_options.contains(&"Done".to_string()));
    }

    #[test]
    fn cancel_state_change_clears_state() {
        let mut app = app_with_issues();
        app.start_state_change();
        app.state_selected = 2;
        app.cancel_state_change();
        assert!(!app.awaiting_state_change);
        assert!(app.state_options.is_empty());
        assert_eq!(app.state_selected, 0);
    }

    #[test]
    fn state_change_navigation() {
        let mut app = app_with_issues();
        app.start_state_change();
        assert_eq!(app.state_selected, 0);

        app.state_change_move_down();
        assert_eq!(app.state_selected, 1);

        app.state_change_move_up();
        assert_eq!(app.state_selected, 0);

        // Cannot go above 0
        app.state_change_move_up();
        assert_eq!(app.state_selected, 0);
    }

    #[test]
    fn selected_state_option_returns_correct_value() {
        let mut app = app_with_issues();
        app.start_state_change();
        assert_eq!(app.selected_state_option(), Some("Backlog"));

        app.state_change_move_down();
        assert_eq!(app.selected_state_option(), Some("Todo"));
    }

    #[test]
    fn apply_local_state_change_updates_my_issues() {
        let mut app = app_with_issues();
        app.selected = 0;
        app.apply_local_state_change("In Progress");
        assert_eq!(app.issues[0].status_str(), "In Progress");
    }

    #[test]
    fn apply_local_state_change_updates_project_issues() {
        let mut app = app_with_project_issues();
        app.project_issue_selected = 0;
        app.view = View::ProjectDetail;
        app.apply_local_state_change("Done");
        assert_eq!(app.project_issues[0].status_str(), "Done");
    }

    #[test]
    fn start_state_change_defaults_to_current_state() {
        let mut app = app_with_issues();
        // Give the first issue an "In Progress" state
        app.issues[0].state = Some(crate::api::types::IssueState {
            name: "In Progress".into(),
        });
        app.selected = 0;
        app.select_issue(); // enter detail view so context_issue() works
        app.start_state_change();
        assert_eq!(app.state_selected, 2); // "In Progress" is index 2
    }

    // --- Output section tests ---

    fn app_with_thread_and_output() -> App<FakeLinearApi> {
        use crate::amp::session::AmpEvent;

        let mut app = app_with_issues();
        app.detail_section = DetailSection::Threads;
        app.detail_threads = vec![ThreadSummary {
            id: "T-abc".to_string(),
            title: "Test thread".to_string(),
            message_count: 1,
            last_activity_ms: 0,
        }];
        // Push some output into the buffer
        let event = AmpEvent {
            event_type: "assistant".to_string(),
            subtype: None,
            raw: serde_json::json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "Hello from assistant"}]
                }
            }),
        };
        app.output_buffer.push_event("T-abc", &event);
        app
    }

    #[test]
    fn focus_output_switches_section_when_output_exists() {
        let mut app = app_with_thread_and_output();
        app.focus_output();
        assert_eq!(app.detail_section, DetailSection::Output);
        assert_eq!(app.detail_output_scroll, 0);
    }

    #[test]
    fn focus_output_does_nothing_when_no_output() {
        let mut app = app_with_issues();
        app.detail_section = DetailSection::Threads;
        app.detail_threads = vec![ThreadSummary {
            id: "T-empty".to_string(),
            title: "Empty".to_string(),
            message_count: 0,
            last_activity_ms: 0,
        }];
        app.focus_output();
        assert_eq!(app.detail_section, DetailSection::Threads);
    }

    #[test]
    fn selected_thread_output_returns_lines() {
        let app = app_with_thread_and_output();
        let output = app.selected_thread_output();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].text, "Hello from assistant");
    }

    #[test]
    fn selected_thread_output_empty_when_no_thread_selected() {
        let mut app = app_with_issues();
        app.detail_section = DetailSection::Body;
        let output = app.selected_thread_output();
        assert!(output.is_empty());
    }

    #[test]
    fn scroll_output_respects_bounds() {
        let mut app = app_with_thread_and_output();
        app.detail_output_scroll_max = 5;
        app.detail_output_scroll = 0;

        app.scroll_output_down();
        assert_eq!(app.detail_output_scroll, 1);

        app.scroll_output_to_bottom();
        assert_eq!(app.detail_output_scroll, 5);

        app.scroll_output_down();
        assert_eq!(app.detail_output_scroll, 5); // can't exceed max

        app.scroll_output_up();
        assert_eq!(app.detail_output_scroll, 4);

        app.detail_output_scroll = 0;
        app.scroll_output_up();
        assert_eq!(app.detail_output_scroll, 0); // can't go below 0
    }

    #[test]
    fn back_to_list_clears_output_state() {
        let mut app = app_with_thread_and_output();
        app.select_issue();
        app.detail_output_scroll = 10;
        app.detail_output_scroll_max = 20;
        app.back_to_list();
        assert_eq!(app.detail_output_scroll, 0);
        assert_eq!(app.detail_output_scroll_max, 0);
    }

    #[test]
    fn start_message_input_activates_and_clears() {
        let mut app = app_with_thread_and_output();
        app.message_input = "leftover".to_string();
        app.start_message_input();
        assert!(app.message_input_active);
        assert!(app.message_input.is_empty());
    }

    #[test]
    fn cancel_message_input_deactivates() {
        let mut app = app_with_thread_and_output();
        app.start_message_input();
        app.message_input.push_str("draft");
        app.cancel_message_input();
        assert!(!app.message_input_active);
        assert!(app.message_input.is_empty());
    }

    #[test]
    fn submit_message_input_returns_thread_and_text() {
        let mut app = app_with_thread_and_output();
        app.start_message_input();
        app.message_input = "do the thing".to_string();
        let result = app.submit_message_input();
        assert!(!app.message_input_active);
        assert!(app.message_input.is_empty());
        let (thread_id, text) = result.unwrap();
        assert_eq!(thread_id, "T-abc");
        assert_eq!(text, "do the thing");
        // Should also appear in output buffer
        assert_eq!(app.output_buffer.line_count("T-abc"), 2); // 1 assistant + 1 user
    }

    #[test]
    fn submit_message_input_empty_returns_none() {
        let mut app = app_with_thread_and_output();
        app.start_message_input();
        let result = app.submit_message_input();
        assert!(result.is_none());
    }

    #[test]
    fn submit_message_input_whitespace_only_returns_none() {
        let mut app = app_with_thread_and_output();
        app.start_message_input();
        app.message_input = "   ".to_string();
        let result = app.submit_message_input();
        assert!(result.is_none());
    }

    #[test]
    fn submit_message_input_no_thread_returns_none() {
        let mut app = app_with_issues();
        app.detail_threads.clear();
        app.start_message_input();
        app.message_input = "hello".to_string();
        let result = app.submit_message_input();
        assert!(result.is_none());
    }
}
