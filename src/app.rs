use crate::api::client::LinearApi;
use crate::api::types::Issue;

#[allow(dead_code)]
pub enum View {
    MyIssues,
    Project,
    Detail,
}

pub struct App<A: LinearApi> {
    pub running: bool,
    pub view: View,
    pub api: A,
    pub issues: Vec<Issue>,
    pub selected: usize,
    pub filter: Option<String>,
    pub filter_input: String,
    pub filtering: bool,
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
            filtering: false,
        }
    }

    pub fn filtered_issues(&self) -> Vec<&Issue> {
        match &self.filter {
            Some(f) => {
                let lower = f.to_lowercase();
                self.issues
                    .iter()
                    .filter(|i| i.title.to_lowercase().contains(&lower))
                    .collect()
            }
            None => self.issues.iter().collect(),
        }
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

    pub fn start_filter(&mut self) {
        self.filtering = true;
        self.filter_input.clear();
    }

    pub fn apply_filter(&mut self) {
        self.filtering = false;
        if self.filter_input.is_empty() {
            self.filter = None;
        } else {
            self.filter = Some(self.filter_input.clone());
        }
        self.selected = 0;
    }

    pub fn cancel_filter(&mut self) {
        self.filtering = false;
        self.filter_input.clear();
    }

    pub fn clear_filter(&mut self) {
        self.filter = None;
        self.filter_input.clear();
        self.selected = 0;
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
            },
            Issue {
                id: "2".into(),
                identifier: "JEM-2".into(),
                title: "Beta task".into(),
                state: None,
                priority: Some(3.0),
                project: None,
            },
            Issue {
                id: "3".into(),
                identifier: "JEM-3".into(),
                title: "Gamma task".into(),
                state: None,
                priority: None,
                project: None,
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
        app.filter = Some("beta".into());
        let filtered = app.filtered_issues();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].identifier, "JEM-2");
    }

    #[test]
    fn filter_is_case_insensitive() {
        let mut app = app_with_issues();
        app.filter = Some("ALPHA".into());
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
        assert_eq!(app.filter.as_deref(), Some("gamma"));
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
}
