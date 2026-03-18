use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::api::client::LinearApi;
use crate::app::{App, SortDirection};

pub fn render<A: LinearApi>(frame: &mut Frame, area: Rect, app: &App<A>) {
    let issues = app.filtered_issues();

    let show_bar = app.awaiting_quit || app.filtering || app.filter.is_some() || app.awaiting_sort || app.awaiting_filter;
    let chunks = if show_bar {
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area)
    } else {
        Layout::vertical([Constraint::Min(0)]).split(area)
    };

    let header = Row::new(vec![
        Cell::from("ID").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Title").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Project").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Priority").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);

    let rows: Vec<Row> = issues
        .iter()
        .map(|issue| {
            Row::new(vec![
                Cell::from(issue.identifier.as_str()),
                Cell::from(issue.title.as_str()),
                Cell::from(issue.project_str()),
                Cell::from(Span::styled(issue.status_str(), status_style(issue.status_str()))),
                Cell::from(Span::styled(issue.priority_str(), priority_style(issue.priority_str()))),
            ])
        })
        .collect();

    let sort_indicator = match &app.sort {
        Some((col, dir)) => {
            let arrow = match dir {
                SortDirection::Asc => "↑",
                SortDirection::Desc => "↓",
            };
            format!(" [sort: {} {}]", col.label(), arrow)
        }
        None => String::new(),
    };

    let title = match &app.filter {
        Some((col, f)) => format!("My Issues ({} of {}){} [filter: {} = {}]", issues.len(), app.issues.len(), sort_indicator, col.label(), f),
        None => format!("My Issues ({}){}", issues.len(), sort_indicator),
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Min(30),
            Constraint::Length(20),
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

    let key_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);

    let column_hints = vec![
        Span::styled("i", key_style),
        Span::raw("d  "),
        Span::styled("t", key_style),
        Span::raw("itle  "),
        Span::styled("p", key_style),
        Span::raw("roject  "),
        Span::styled("s", key_style),
        Span::raw("tatus  p"),
        Span::styled("r", key_style),
        Span::raw("iority"),
    ];

    // Status bar
    if app.awaiting_quit {
        let line = Line::from(vec![
            Span::raw("Press "),
            Span::styled("q", key_style),
            Span::raw(" again to quit"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if app.awaiting_sort {
        let mut spans = vec![Span::raw("order by: ")];
        spans.extend(column_hints);
        frame.render_widget(Paragraph::new(Line::from(spans)), chunks[1]);
    } else if app.awaiting_filter {
        let mut spans = vec![Span::raw("filter by: ")];
        spans.extend(column_hints);
        frame.render_widget(Paragraph::new(Line::from(spans)), chunks[1]);
    } else if app.filtering {
        let prefix = match app.filter_column {
            Some(ref col) => format!("[{}] /", col.label()),
            None => "/".into(),
        };
        let line = Line::from(vec![
            Span::raw(prefix),
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

fn status_style(status: &str) -> Style {
    match status {
        "In Progress" => Style::default().fg(Color::Yellow),
        "Todo" => Style::default().fg(Color::Cyan),
        "Done" => Style::default().fg(Color::Green),
        "Canceled" | "Cancelled" => Style::default().fg(Color::DarkGray),
        "In Review" => Style::default().fg(Color::Magenta),
        "Backlog" | "Triage" => Style::default().fg(Color::DarkGray),
        _ => Style::default(),
    }
}

fn priority_style(priority: &str) -> Style {
    match priority {
        "Urgent" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        "High" => Style::default().fg(Color::Red),
        "Medium" => Style::default().fg(Color::Yellow),
        "Low" => Style::default().fg(Color::DarkGray),
        _ => Style::default(),
    }
}
