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
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

/// Dashboard state.
struct App {
    /// Whether to quit the application.
    should_quit: bool,
    /// Current tick for animations.
    tick: u64,
    /// Latest stats loaded from SHM
    stats: Option<aetherless_core::stats::AetherlessStats>,
}

impl App {
    fn new() -> Self {
        Self {
            should_quit: false,
            tick: 0,
            stats: None,
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

        // Try reading stats
        if let Ok(content) = std::fs::read_to_string("/dev/shm/aetherless-stats.json") {
            if let Ok(stats) =
                serde_json::from_str::<aetherless_core::stats::AetherlessStats>(&content)
            {
                app.stats = Some(stats);
            }
        }

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

fn render(frame: &mut Frame, app: &App) {
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
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(main_layout[1]);

    // Left column - Function List
    let left_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(100)])
        .split(content_layout[0]);

    // Function table
    let header = Row::new(vec![
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

    let rows: Vec<Row> = if let Some(stats) = &app.stats {
        if stats.functions.is_empty() {
            vec![Row::new(vec![
                Cell::from("(no functions running)"),
                Cell::from("-"),
                Cell::from("-"),
                Cell::from("-"),
            ])
            .style(Style::default().fg(Color::DarkGray))]
        } else {
            let mut sorted_funcs: Vec<_> = stats.functions.values().collect();
            sorted_funcs.sort_by_key(|f| &f.id);

            sorted_funcs
                .into_iter()
                .map(|f| {
                    let state_color = match f.state {
                        aetherless_core::FunctionState::Running => Color::Green,
                        aetherless_core::FunctionState::WarmSnapshot => Color::Blue,
                        aetherless_core::FunctionState::Uninitialized => Color::Gray,
                        _ => Color::White,
                    };

                    Row::new(vec![
                        Cell::from(f.id.as_str()),
                        Cell::from(format!("{:?}", f.state))
                            .style(Style::default().fg(state_color)),
                        Cell::from(format!("{} MB", f.memory_mb)),
                        Cell::from(f.port.to_string()),
                    ])
                })
                .collect()
        }
    } else {
        vec![Row::new(vec![
            Cell::from("Waiting for orchestrator..."),
            Cell::from("-"),
            Cell::from("-"),
            Cell::from("-"),
        ])
        .style(Style::default().fg(Color::DarkGray))]
    };

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(40),
            Constraint::Percentage(25),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(" Functions ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green)),
    );
    frame.render_widget(table, left_layout[0]);

    // Right column - Metrics
    let right_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Active Stats
            Constraint::Length(8), // Warm Pool
            Constraint::Min(5),    // Events
        ])
        .split(content_layout[1]);

    // Stats
    let active_count = app.stats.as_ref().map(|s| s.active_instances).unwrap_or(0);
    let warm_pool_status = app
        .stats
        .as_ref()
        .map(|s| s.warm_pool_active)
        .unwrap_or(false);

    let stats_text = vec![
        Line::from(vec![
            Span::raw("Active Instances: "),
            Span::styled(
                active_count.to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("Warm Pool: "),
            if warm_pool_status {
                Span::styled("ENABLED", Style::default().fg(Color::Cyan))
            } else {
                Span::styled("DISABLED", Style::default().fg(Color::Red))
            },
        ]),
        Line::from(vec![
            Span::raw("SHM Latency: "),
            Span::styled("-- μs", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let stats_block = Paragraph::new(stats_text).block(
        Block::default()
            .title(" System Stats ")
            .borders(Borders::ALL),
    );

    frame.render_widget(stats_block, right_layout[0]);

    // Warm Pool Details (Placeholder if not detailed yet)
    let wp_block = Block::default()
        .title(" Warm Pool Metrics ")
        .borders(Borders::ALL);
    frame.render_widget(wp_block, right_layout[1]);

    // Footer
    let footer = Paragraph::new(" Press 'q' to quit ")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, main_layout[2]);
}
