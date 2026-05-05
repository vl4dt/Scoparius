//! TUI rendering logic.

use crate::tui::app::{App, Tab, ScanStatus};
use bytesize::ByteSize;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Tabs, Gauge, Table, Row, Cell, Sparkline},
    Frame,
};

pub fn render(app: &App, frame: &mut Frame) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header / Tabs
            Constraint::Min(0),    // Body
            Constraint::Length(1), // Footer
        ])
        .split(frame.area());

    render_header(app, frame, chunks[0]);
    render_body(app, frame, chunks[1]);
    render_footer(app, frame, chunks[2]);
}

fn render_header(app: &App, frame: &mut Frame, area: Rect) {
    let titles = vec![" [1] DASHBOARD ", " [2] SCANNER ", " [3] APPS ", " [4] DUPLICATES ", " [5] CONFIG "];
    
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(Color::Cyan)))
        .select(app.active_tab as usize)
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    frame.render_widget(tabs, area);
    
    let logo = " SCOPARIUS // FUI_V1 ";
    let logo_area = Rect {
        x: area.width.saturating_sub(logo.len() as u16 + 2),
        y: area.y,
        width: logo.len() as u16 + 2,
        height: 1,
    };
    frame.render_widget(Paragraph::new(logo).cyan().bold(), logo_area);
}

fn render_body(app: &App, frame: &mut Frame, area: Rect) {
    match app.active_tab {
        Tab::Dashboard => render_dashboard(app, frame, area),
        Tab::Scanner => render_scanner(app, frame, area),
        Tab::Apps => render_apps(app, frame, area),
        Tab::Duplicates => render_duplicates(app, frame, area),
        _ => {
            let msg = format!("Content for {:?} Coming Soon...", app.active_tab);
            frame.render_widget(
                Paragraph::new(msg)
                    .block(Block::default().borders(Borders::ALL).title(" STATUS "))
                    .alignment(ratatui::layout::Alignment::Center),
                area,
            );
        }
    }
}

fn render_dashboard(app: &App, frame: &mut Frame, area: Rect) {
    let inner_area = area.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 });
    let dash_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(60),
        ])
        .split(inner_area);
        
    render_storage_panel(app, frame, dash_chunks[0]);
    
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40), // System status
            Constraint::Percentage(60), // Activity Monitor (Sparkline)
        ])
        .split(dash_chunks[1]);

    render_status_panel(app, frame, right_chunks[0]);
    render_activity_panel(app, frame, right_chunks[1]);
}

fn render_status_panel(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" SYSTEM_STATUS ");
    
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut status_text = match &app.scan_status {
        ScanStatus::Idle => vec![Line::from("READY: WAITING FOR COMMAND")],
        ScanStatus::Scanning { files, size, current_path, .. } => vec![
            Line::from(vec![Span::styled("SCANNING: ", Style::default().fg(Color::Yellow).bold()), Span::raw("ACTIVE")]),
            Line::from(format!("FILES: {}", files)),
            Line::from(format!("SIZE:  {}", ByteSize(*size))),
            Line::from(format!("PATH:  {}", current_path)).dim(),
        ],
        ScanStatus::Hashing { files_hashed, total_to_hash, current_path } => vec![
            Line::from(vec![Span::styled("HASHING:  ", Style::default().fg(Color::Magenta).bold()), Span::raw("ACTIVE")]),
            Line::from(format!("PROGRESS: {} / {}", files_hashed, total_to_hash)),
            Line::from(format!("PATH:     {}", current_path)).dim(),
        ],
        ScanStatus::Completed { total_files, total_size, duration } => vec![
            Line::from(vec![Span::styled("COMPLETED: ", Style::default().fg(Color::Green).bold()), Span::raw("SUCCESS")]),
            Line::from(format!("TOTAL FILES: {}", total_files)),
            Line::from(format!("TOTAL SIZE:  {}", ByteSize(*total_size))),
            Line::from(format!("DURATION:    {:.1}s", duration.as_secs_f32())),
        ],
        ScanStatus::Error(e) => vec![
            Line::from(vec![Span::styled("ERROR: ", Style::default().fg(Color::Red).bold()), Span::raw(e)]),
        ],
    };

    status_text.push(Line::from(""));
    status_text.push(Line::from(format!("INSTALLED APPS: {}", app.apps.len())));
    let unused = app.apps.iter().filter(|a| a.appears_unused).count();
    status_text.push(Line::from(format!("UNUSED APPS:    {}", unused)).yellow());

    frame.render_widget(Paragraph::new(status_text), inner);
}

fn render_activity_panel(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" I/O_ACTIVITY_MONITOR ");
    
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Create a pseudo-random animation for the sparkline based on tick count
    let mut data = Vec::with_capacity(area.width as usize);
    for i in 0..area.width as usize {
        let val = ((((app.tick_count + i as u64) as f64 * 0.3).sin() + 1.0) * 4.0) as u64;
        data.push(val);
    }

    let sparkline = Sparkline::default()
        .data(&data)
        .style(Style::default().fg(Color::Cyan));
    
    frame.render_widget(sparkline, inner);
}

fn render_storage_panel(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" STORAGE_HEALTH ");
    
    let inner = block.inner(area);
    frame.render_widget(block, area);
    
    let disk_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            app.disks.iter().map(|_| Constraint::Length(4)).collect::<Vec<_>>()
        )
        .split(inner);
        
    for (i, disk) in app.disks.iter().enumerate() {
        if i >= disk_chunks.len() { break; }
        let area = disk_chunks[i];
        let usage_pct = disk.usage_percent();
        let color = if usage_pct > 90.0 { Color::Red } else if usage_pct > 75.0 { Color::Yellow } else { Color::Green };
        
        let label = format!("{} ({})", disk.mount_point.display(), disk.fs_type);
        let usage_text = format!("{}/{} ({:.1}%)", ByteSize(disk.used_bytes()), ByteSize(disk.total_bytes), usage_pct);
        
        let gauge = Gauge::default()
            .block(Block::default().title(Span::styled(label, Style::default().fg(Color::DarkGray))))
            .gauge_style(Style::default().fg(color).bg(Color::Black))
            .percent(usage_pct as u16)
            .label(usage_text);
            
        frame.render_widget(gauge, area);
    }
}

fn render_scanner(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" SCANNER_INTERFACE ");
    
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Min(0),
        ])
        .split(inner);

    render_scan_progress(app, frame, chunks[0]);
    
    let results_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" DISCOVERED_ASSETS ");
    frame.render_widget(results_block, chunks[1]);
}

fn render_scan_progress(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::uniform(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    match &app.scan_status {
        ScanStatus::Scanning { files, size, current_path, .. } => {
            let text = vec![
                Line::from(vec![Span::raw("STATUS: "), Span::styled("SCANNING", Style::default().fg(Color::Yellow).bold())]),
                Line::from(vec![Span::raw("OBJECTS: "), Span::styled(files.to_string(), Style::default().fg(Color::Cyan))]),
                Line::from(vec![Span::raw("VOLUME:  "), Span::styled(ByteSize(*size).to_string(), Style::default().fg(Color::Green))]),
                Line::from(vec![Span::raw("SOURCE:  "), Span::raw(current_path).dim()]),
            ];
            frame.render_widget(Paragraph::new(text), inner);
        }
        ScanStatus::Hashing { files_hashed, total_to_hash, current_path } => {
            let pct = if *total_to_hash > 0 { (*files_hashed as f64 / *total_to_hash as f64 * 100.0) as u16 } else { 100 };
            let gauge = Gauge::default()
                .block(Block::default().title(" CONTENT_HASHING_PASS "))
                .gauge_style(Style::default().fg(Color::Magenta).bg(Color::Black))
                .percent(pct)
                .label(format!("{} / {} FILES", files_hashed, total_to_hash));
            
            let chunks = Layout::default()
                .constraints([Constraint::Length(3), Constraint::Min(0)])
                .split(inner);
            
            frame.render_widget(gauge, chunks[0]);
            frame.render_widget(Paragraph::new(format!("HASHING: {}", current_path)).dim(), chunks[1]);
        }
        ScanStatus::Idle => {
            frame.render_widget(Paragraph::new("READY TO SCAN\nPress [S] to start analysis of all disks.").alignment(ratatui::layout::Alignment::Center), inner);
        }
        ScanStatus::Completed { total_files, total_size, duration } => {
            let text = vec![
                Line::from(vec![Span::raw("STATUS: "), Span::styled("ANALYSIS COMPLETE", Style::default().fg(Color::Green).bold())]),
                Line::from(vec![Span::raw("TOTAL:  "), Span::raw(format!("{} files ({})", total_files, ByteSize(*total_size)))]),
                Line::from(vec![Span::raw("TIME:   "), Span::raw(format!("{:.2}s", duration.as_secs_f32()))]),
            ];
            frame.render_widget(Paragraph::new(text), inner);
        }
        _ => {}
    }
}

fn render_apps(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" APPLICATION_HEURISTICS ");
    
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows: Vec<Row> = app.apps.iter().map(|app| {
        let name = Span::raw(&app.name);
        let last_used = match app.last_used {
            Some(dt) => dt.format("%Y-%m-%d").to_string(),
            None => "NEVER".to_string(),
        };
        let status = if app.appears_unused {
            Span::styled("UNUSED", Style::default().fg(Color::Yellow))
        } else {
            Span::styled("ACTIVE", Style::default().fg(Color::Green))
        };
        
        let size = app.size.map(|s| ByteSize(s).to_string()).unwrap_or_else(|| "—".into());

        Row::new(vec![
            Cell::from(name),
            Cell::from(size),
            Cell::from(last_used),
            Cell::from(status),
        ])
    }).collect();

    let header = Row::new(vec!["APPLICATION", "DISK USAGE", "LAST EXECUTED", "STATUS"])
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let table = Table::new(rows, [
        Constraint::Percentage(40),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
    ])
    .header(header)
    .block(Block::default().padding(Padding::uniform(1)));

    frame.render_widget(table, inner);
}

fn render_duplicates(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" DUPLICATE_DETECTION_ENGINE ");
    
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.duplicates.is_empty() {
        let msg = if matches!(app.scan_status, ScanStatus::Completed { .. }) {
            "NO DUPLICATES FOUND"
        } else {
            "SCAN REQUIRED TO IDENTIFY DUPLICATES"
        };
        frame.render_widget(
            Paragraph::new(msg)
                .alignment(ratatui::layout::Alignment::Center)
                .block(Block::default().padding(Padding::uniform(5))),
            inner,
        );
        return;
    }

    let rows: Vec<Row> = app.duplicates.iter().map(|group| {
        let hash = &group.hash[..8];
        let count = group.paths.len();
        let size = ByteSize(group.file_size).to_string();
        let wasted = ByteSize(group.wasted_bytes).to_string();
        let primary_path = group.paths[0].to_string_lossy();

        Row::new(vec![
            Cell::from(Span::styled(hash, Style::default().fg(Color::DarkGray))),
            Cell::from(primary_path.to_string()),
            Cell::from(count.to_string()).cyan(),
            Cell::from(size),
            Cell::from(wasted).yellow(),
        ])
    }).collect();

    let header = Row::new(vec!["HASH", "CANDIDATE_PATH", "COUNT", "SIZE", "WASTED"])
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let table = Table::new(rows, [
        Constraint::Length(10),
        Constraint::Min(40),
        Constraint::Length(10),
        Constraint::Length(15),
        Constraint::Length(15),
    ])
    .header(header)
    .block(Block::default().padding(Padding::uniform(1)));

    frame.render_widget(table, inner);
}

fn render_footer(app: &App, frame: &mut Frame, area: Rect) {
    let mut keys = vec![
        Span::raw(" [Q]uit "),
        Span::raw(" [TAB] Nav "),
        Span::raw(" [R]efresh "),
    ];

    if matches!(app.scan_status, ScanStatus::Scanning { .. }) {
        keys.push(Span::styled(" [C]ancel ", Style::default().fg(Color::Red)));
    } else {
        keys.push(Span::styled(" [S]tart Scan ", Style::default().fg(Color::Green)));
    }

    let footer = Paragraph::new(Line::from(keys));
    frame.render_widget(footer, area);
}
