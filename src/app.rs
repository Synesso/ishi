use std::cmp::Ordering;

use crate::api::client::LinearApi;
use crate::api::types::Issue;

#[allow(dead_code)]
pub enum View {
    MyIssues,
    Project,
    Detail,
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
    pub sort: Option<(SortColumn, SortDirection)>,
    pub search: Option<String>,
    pub search_input: String,
    pub searching: bool,
    pub detail_scroll: u16,
    pub detail_scroll_max: u16,
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
            sort: None,
            search: None,
            search_input: String::new(),
            searching: false,
            detail_scroll: 0,
            detail_scroll_max: 0,
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
                    SortColumn::Priority => a.priority.partial_cmp(&b.priority).unwrap_or(Ordering::Equal),
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
        }
    }

    pub fn back_to_list(&mut self) {
        self.view = View::MyIssues;
        self.detail_scroll = 0;
    }

    pub fn selected_issue(&self) -> Option<&Issue> {
        let issues = self.filtered_issues();
        issues.get(self.selected).copied()
    }

    pub fn scroll_detail_down(&mut self) {
        if self.detail_scroll < self.detail_scroll_max {
            self.detail_scroll = self.detail_scroll.saturating_add(1);
        }
    }

    pub fn scroll_detail_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
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
}
