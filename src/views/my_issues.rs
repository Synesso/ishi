use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::api::client::LinearApi;
use crate::app::App;

pub fn render<A: LinearApi>(frame: &mut Frame, area: Rect, app: &App<A>) {
    let issues = app.filtered_issues();

    let chunks = if app.filtering || app.filter.is_some() {
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area)
    } else {
        Layout::vertical([Constraint::Min(0)]).split(area)
    };

    let header = Row::new(vec![
        Cell::from("ID").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Title").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Priority").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);

    let rows: Vec<Row> = issues
        .iter()
        .map(|issue| {
            Row::new(vec![
                Cell::from(issue.identifier.as_str()),
                Cell::from(issue.title.as_str()),
                Cell::from(issue.status_str()),
                Cell::from(issue.priority_str()),
            ])
        })
        .collect();

    let title = match &app.filter {
        Some(f) => format!("My Issues ({} of {}) [filter: {}]", issues.len(), app.issues.len(), f),
        None => format!("My Issues ({})", issues.len()),
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Min(30),
            Constraint::Length(15),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut table_state = TableState::default();
    if !issues.is_empty() {
        table_state.select(Some(app.selected));
    }

    frame.render_stateful_widget(table, chunks[0], &mut table_state);

    // Filter bar
    if app.filtering {
        let line = Line::from(vec![
            Span::raw("/"),
            Span::styled(&app.filter_input, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("▏"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if app.filter.is_some() {
        let line = Line::from(vec![
            Span::raw("Filter active — press Esc to clear"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    }
}
