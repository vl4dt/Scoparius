//! Application state and event loop.

use crate::tui::terminal::Tui;
use crate::tui::ui;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use scoparius_core::config::ScopariusConfig;
use scoparius_core::models::{DiskInfo, ProgressEvent, ScanResult, AppInfo};
use scoparius_core::platform;
use scoparius_core::scanner::{ScanConfig, Scanner};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Tab {
    Dashboard,
    Scanner,
    Apps,
    Duplicates,
    Config,
}

#[derive(Debug, Clone)]
pub enum ScanStatus {
    Idle,
    Scanning {
        files: u64,
        size: u64,
        current_path: String,
        start_time: Instant,
    },
    Hashing {
        files_hashed: u64,
        total_to_hash: u64,
        current_path: String,
    },
    Completed {
        total_files: u64,
        total_size: u64,
        duration: Duration,
    },
    Error(String),
}

pub struct App {
    pub config: ScopariusConfig,
    pub active_tab: Tab,
    pub should_quit: bool,
    pub tick_count: u64,
    pub disks: Vec<DiskInfo>,
    pub apps: Vec<AppInfo>,
    
    // Scan State
    pub scan_status: ScanStatus,
    pub last_scan_result: Option<ScanResult>,
    pub duplicates: Vec<scoparius_core::analyzer::duplicates::DuplicateGroup>,
    pub scan_rx: Option<mpsc::UnboundedReceiver<ProgressEvent>>,
    pub cancel_handle: Option<Arc<AtomicBool>>,
}

impl App {
    pub fn new(config: ScopariusConfig) -> Self {
        let adapter = platform::create_adapter();
        let disks = adapter.list_disks().unwrap_or_default();
        let apps = adapter.list_installed_apps().unwrap_or_default();

        Self {
            config,
            active_tab: Tab::Dashboard,
            should_quit: false,
            tick_count: 0,
            disks,
            apps,
            scan_status: ScanStatus::Idle,
            last_scan_result: None,
            duplicates: Vec::new(),
            scan_rx: None,
            cancel_handle: None,
        }
    }

    pub async fn run(&mut self, terminal: &mut Tui) -> anyhow::Result<()> {
        while !self.should_quit {
            terminal.draw(|f| ui::render(self, f))?;

            // Check for scanner events
            self.update_scan_status();

            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key.code);
                    }
                }
            }
            
            self.on_tick();
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => self.next_tab(),
            KeyCode::BackTab => self.prev_tab(),
            KeyCode::Char('1') => self.active_tab = Tab::Dashboard,
            KeyCode::Char('2') => self.active_tab = Tab::Scanner,
            KeyCode::Char('3') => self.active_tab = Tab::Apps,
            KeyCode::Char('4') => self.active_tab = Tab::Duplicates,
            KeyCode::Char('5') => self.active_tab = Tab::Config,
            KeyCode::Char('r') => self.refresh_data(),
            KeyCode::Char('s') => self.start_scan(),
            KeyCode::Char('c') => self.cancel_scan(),
            _ => {}
        }
    }

    fn on_tick(&mut self) {
        self.tick_count += 1;
        if self.tick_count % 100 == 0 {
            self.refresh_data();
        }
    }

    fn refresh_data(&mut self) {
        let adapter = platform::create_adapter();
        if let Ok(disks) = adapter.list_disks() {
            self.disks = disks;
        }
        if let Ok(apps) = adapter.list_installed_apps() {
            self.apps = apps;
        }
    }

    fn start_scan(&mut self) {
        if matches!(self.scan_status, ScanStatus::Scanning { .. }) {
            return;
        }

        let mut scan_config = ScanConfig::default();
        scan_config.roots = self.disks.iter().map(|d| d.mount_point.clone()).collect();
        scan_config.parallelism = self.config.scanner.parallelism;
        scan_config.skip_hidden = self.config.scanner.skip_hidden;
        scan_config.enable_hashing = true; // Enable hashing for duplicates in TUI

        let scanner = Scanner::new(scan_config);
        let cancel_handle = scanner.cancel_handle();
        let (tx, rx) = mpsc::unbounded_channel();

        self.scan_status = ScanStatus::Scanning {
            files: 0,
            size: 0,
            current_path: String::new(),
            start_time: Instant::now(),
        };
        self.scan_rx = Some(rx);
        self.cancel_handle = Some(cancel_handle);
        self.active_tab = Tab::Scanner;

        tokio::spawn(async move {
            let _ = scanner.scan(Some(tx)).await;
        });
    }

    fn cancel_scan(&mut self) {
        if let Some(handle) = &self.cancel_handle {
            handle.store(true, Ordering::Relaxed);
        }
    }

    fn update_scan_status(&mut self) {
        let mut completed = false;
        let rx = match &mut self.scan_rx {
            Some(rx) => rx,
            None => return,
        };

        while let Ok(event) = rx.try_recv() {
            match event {
                ProgressEvent::ScanStarted { .. } => {}
                ProgressEvent::ScanProgress { files_scanned, bytes_scanned, current_path, .. } => {
                    if let ScanStatus::Scanning { files, size, current_path: path, .. } = &mut self.scan_status {
                        *files = files_scanned;
                        *size = bytes_scanned;
                        *path = current_path.to_string_lossy().to_string();
                    }
                }
                ProgressEvent::HashProgress { files_hashed, total_to_hash, current_path } => {
                    self.scan_status = ScanStatus::Hashing {
                        files_hashed,
                        total_to_hash,
                        current_path: current_path.to_string_lossy().to_string(),
                    };
                }
                ProgressEvent::ScanCompleted { result: res, duration_ms } => {
                    self.scan_status = ScanStatus::Completed {
                        total_files: res.total_files,
                        total_size: res.total_size,
                        duration: Duration::from_millis(duration_ms),
                    };
                    
                    // Compute duplicates if hashing was enabled
                    if self.config.scanner.enable_hashing {
                        if let Ok(groups) = scoparius_core::analyzer::duplicates::find_duplicates(&res.entries, 1024 * 1024, None) {
                            self.duplicates = groups;
                        }
                    }
                    
                    self.last_scan_result = Some(res);
                    completed = true;
                }
                ProgressEvent::ScanError { path, error } => {
                    self.scan_status = ScanStatus::Error(format!("{}: {}", path.display(), error));
                    completed = true;
                }
                _ => {}
            }
        }

        if completed {
            self.scan_rx = None;
            self.cancel_handle = None;
        }
    }

    fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Dashboard => Tab::Scanner,
            Tab::Scanner => Tab::Apps,
            Tab::Apps => Tab::Duplicates,
            Tab::Duplicates => Tab::Config,
            Tab::Config => Tab::Dashboard,
        };
    }

    fn prev_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Dashboard => Tab::Config,
            Tab::Scanner => Tab::Dashboard,
            Tab::Apps => Tab::Scanner,
            Tab::Duplicates => Tab::Apps,
            Tab::Config => Tab::Duplicates,
        };
    }
}
