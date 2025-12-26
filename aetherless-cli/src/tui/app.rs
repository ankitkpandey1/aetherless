// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! TUI Dashboard using ratatui.
//!
//! Visualizes the warm pool of functions and real-time statistics.

use std::io::stdout;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Gauge, List, ListItem, Paragraph, Row, Table},
};

/// Dashboard state.
struct App {
    /// Whether to quit the application.
    should_quit: bool,
    /// Current tick for animations.
    tick: u64,
}

impl App {
    fn new() -> Self {
        Self {
            should_quit: false,
            tick: 0,
        }
    }

    fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }
}

/// Run the TUI dashboard.
pub async fn run_dashboard() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let mut app = App::new();

    // Main loop
    loop {
        terminal.draw(|frame| render(frame, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }

        app.tick();
    }

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}

fn render(frame: &mut Frame, _app: &App) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(10),   // Main content
            Constraint::Length(3), // Footer
        ])
        .split(frame.area());

    // Title
    let title = Paragraph::new(" AETHERLESS DASHBOARD ")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
    frame.render_widget(title, main_layout[0]);

    // Main content - split into columns
    let content_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_layout[1]);

    // Left column - Warm Pool
    let left_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(content_layout[0]);

    // Warm Pool table
    let warm_pool_header = Row::new(vec![
        Cell::from("Function ID"),
        Cell::from("State"),
        Cell::from("Memory"),
        Cell::from("Port"),
    ])
    .style(
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Yellow),
    );

    let warm_pool_rows = vec![Row::new(vec![
        Cell::from("(no functions)"),
        Cell::from("-"),
        Cell::from("-"),
        Cell::from("-"),
    ])
    .style(Style::default().fg(Color::DarkGray))];

    let warm_pool = Table::new(
        warm_pool_rows,
        [
            Constraint::Percentage(40),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ],
    )
    .header(warm_pool_header)
    .block(
        Block::default()
            .title(" Warm Pool ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green)),
    );
    frame.render_widget(warm_pool, left_layout[0]);

    // eBPF Stats
    let ebpf_stats = Paragraph::new(vec![
        Line::from(vec![
            Span::raw("XDP Program: "),
            Span::styled("Not loaded", Style::default().fg(Color::Red)),
        ]),
        Line::from(vec![
            Span::raw("Interface:   "),
            Span::styled("lo", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::raw("Packets:     "),
            Span::styled("--", Style::default().fg(Color::DarkGray)),
        ]),
    ])
    .block(
        Block::default()
            .title(" eBPF Data Plane ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta)),
    );
    frame.render_widget(ebpf_stats, left_layout[1]);

    // Right column - Metrics
    let right_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Min(0),
        ])
        .split(content_layout[1]);

    // SHM Latency gauge
    let shm_gauge = Gauge::default()
        .block(
            Block::default()
                .title(" SHM Latency ")
                .borders(Borders::ALL),
        )
        .gauge_style(Style::default().fg(Color::Cyan))
        .percent(0)
        .label("-- μs");
    frame.render_widget(shm_gauge, right_layout[0]);

    // CRIU Restore gauge
    let criu_gauge = Gauge::default()
        .block(
            Block::default()
                .title(" CRIU Restore Time ")
                .borders(Borders::ALL),
        )
        .gauge_style(Style::default().fg(Color::Yellow))
        .percent(0)
        .label("-- ms (limit: 15ms)");
    frame.render_widget(criu_gauge, right_layout[1]);

    // Memory usage
    let mem_gauge = Gauge::default()
        .block(
            Block::default()
                .title(" Memory Usage ")
                .borders(Borders::ALL),
        )
        .gauge_style(Style::default().fg(Color::Green))
        .percent(0)
        .label("0 / 0 MB");
    frame.render_widget(mem_gauge, right_layout[2]);

    // Events log
    let events: Vec<ListItem> = vec![
        ListItem::new("Dashboard started"),
        ListItem::new("Waiting for orchestrator..."),
    ];
    let events_list = List::new(events)
        .block(Block::default().title(" Events ").borders(Borders::ALL))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(events_list, right_layout[3]);

    // Footer
    let footer = Paragraph::new(" Press 'q' to quit | 'r' to refresh ")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, main_layout[2]);
}
