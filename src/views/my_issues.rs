use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};

use crate::api::client::LinearApi;
use crate::app::{App, SortDirection};

pub fn render<A: LinearApi>(frame: &mut Frame, area: Rect, app: &App<A>) {
    // Loading state — show centered message instead of empty table
    if app.loading {
        let loading = Paragraph::new(Line::from(Span::styled(
            "Loading issues…",
            Style::default().fg(Color::Yellow),
        )))
        .block(Block::default().borders(Borders::ALL).title("My Issues"));
        frame.render_widget(loading, area);
        return;
    }

    let issues = app.filtered_issues();

    let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);

    let header = Row::new(vec![
        Cell::from("ID").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Title").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Project").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Priority").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Amp").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);

    let search_query = app.search.as_deref();

    let rows: Vec<Row> = issues
        .iter()
        .enumerate()
        .map(|(idx, issue)| {
            let is_multi_selected = app.selected_indices.contains(&idx);
            let base_style = if is_multi_selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            let thread_display = app.thread_count_display(&issue.identifier);
            let thread_style = if thread_display.contains('/') {
                Style::default().fg(Color::Green)
            } else if thread_display == "-" {
                Style::default().fg(Color::DarkGray)
            } else {
                base_style
            };
            let select_marker = if is_multi_selected { "▌ " } else { "  " };
            let mut id_spans = vec![Span::styled(
                select_marker,
                if is_multi_selected {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                },
            )];
            id_spans.extend(highlight_match(
                issue.identifier.as_str(),
                search_query,
                base_style,
            ));
            Row::new(vec![
                Cell::from(Line::from(id_spans)),
                Cell::from(Line::from(highlight_match(
                    issue.title.as_str(),
                    search_query,
                    base_style,
                ))),
                Cell::from(Line::from(highlight_match(
                    issue.project_str(),
                    search_query,
                    base_style,
                ))),
                Cell::from(Line::from(highlight_match(
                    issue.status_str(),
                    search_query,
                    if is_multi_selected {
                        base_style
                    } else {
                        status_style(issue.status_str())
                    },
                ))),
                Cell::from(Line::from(highlight_match(
                    issue.priority_str(),
                    search_query,
                    if is_multi_selected {
                        base_style
                    } else {
                        priority_style(issue.priority_str())
                    },
                ))),
                Cell::from(Span::styled(thread_display, thread_style)),
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

    let search_indicator = match &app.search {
        Some(q) => format!(" [search: {}]", q),
        None => String::new(),
    };

    let select_indicator = if app.selected_indices.len() > 1 {
        format!(" [{} selected]", app.selected_indices.len())
    } else {
        String::new()
    };

    let title = match &app.filter {
        Some((col, f)) => format!(
            "My Issues ({} of {}){}{}{} [filter: {} = {}]",
            issues.len(),
            app.issues.len(),
            sort_indicator,
            search_indicator,
            select_indicator,
            col.label(),
            f
        ),
        None => format!(
            "My Issues ({}){}{}{}",
            issues.len(),
            sort_indicator,
            search_indicator,
            select_indicator
        ),
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Min(30),
            Constraint::Length(20),
            Constraint::Length(15),
            Constraint::Length(10),
            Constraint::Length(5),
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

    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

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
    if let Some(ref err) = app.error {
        let line = Line::from(vec![
            Span::styled(
                "Error: ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(&err.message, Style::default().fg(Color::Red)),
            Span::styled(
                " (press Esc to dismiss)",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if app.refreshing {
        let line = Line::from(Span::styled(
            "Refreshing...",
            Style::default().fg(Color::Yellow),
        ));
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if let Some((ref msg, _)) = app.flash {
        let line = Line::from(Span::styled(
            msg.as_str(),
            Style::default().fg(Color::Green),
        ));
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if app.awaiting_quit {
        let line = Line::from(vec![
            Span::raw("Press "),
            Span::styled("q", key_style),
            Span::raw(" again to quit"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if app.awaiting_open {
        let line = Line::from(vec![
            Span::raw("open in: "),
            Span::styled("l", key_style),
            Span::raw("inear  "),
            Span::styled("g", key_style),
            Span::raw("ithub PR"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if app.awaiting_state_change {
        let mut spans = vec![Span::raw("move to: ")];
        if !app.state_type_ahead.is_empty() {
            spans.push(Span::styled(
                format!("[{}] ", app.state_type_ahead),
                Style::default().fg(Color::Cyan),
            ));
        }
        for (i, state) in app.state_options.iter().enumerate() {
            if i == app.state_selected {
                spans.push(Span::styled(
                    state.as_str(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw(state.as_str()));
            }
            if i < app.state_options.len() - 1 {
                spans.push(Span::raw("  "));
            }
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), chunks[1]);
    } else if app.awaiting_sort {
        let mut spans = vec![Span::raw("sort by: ")];
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
            Span::styled(
                &app.filter_input,
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("▏"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if app.searching {
        let line = Line::from(vec![
            Span::raw("/"),
            Span::styled(
                &app.search_input,
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("▏"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if app.search.is_some() || app.filter.is_some() {
        let line = Line::from(vec![Span::raw("Active — press Esc to clear")]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else {
        let sep = Span::styled(" │ ", Style::default().fg(Color::DarkGray));
        let line = Line::from(vec![
            Span::styled("s", key_style),
            Span::raw("ort  "),
            Span::styled("f", key_style),
            Span::raw("ilter  "),
            Span::styled("/", key_style),
            Span::raw("search"),
            sep.clone(),
            Span::styled("i", key_style),
            Span::raw(" assign  "),
            Span::styled("c", key_style),
            Span::raw("reate  "),
            Span::styled("a", key_style),
            Span::raw("mp  "),
            Span::styled("p", key_style),
            Span::raw("rojects  "),
            Span::styled("o", key_style),
            Span::raw("pen  "),
            Span::styled("m", key_style),
            Span::raw("ove  "),
            Span::styled("r", key_style),
            Span::raw("efresh"),
            sep,
            Span::styled("?", key_style),
            Span::raw("help"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    }
}

fn highlight_match<'a>(text: &'a str, query: Option<&str>, base_style: Style) -> Vec<Span<'a>> {
    let query = match query {
        Some(q) if !q.is_empty() => q,
        _ => return vec![Span::styled(text, base_style)],
    };
    let lower_text = text.to_lowercase();
    let lower_query = query.to_lowercase();
    let mut spans = Vec::new();
    let mut start = 0;
    while let Some(pos) = lower_text[start..].find(&lower_query) {
        let abs_pos = start + pos;
        if abs_pos > start {
            spans.push(Span::styled(&text[start..abs_pos], base_style));
        }
        spans.push(Span::styled(
            &text[abs_pos..abs_pos + query.len()],
            base_style.add_modifier(Modifier::BOLD),
        ));
        start = abs_pos + query.len();
    }
    if start < text.len() {
        spans.push(Span::styled(&text[start..], base_style));
    }
    if spans.is_empty() {
        spans.push(Span::styled(text, base_style));
    }
    spans
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
