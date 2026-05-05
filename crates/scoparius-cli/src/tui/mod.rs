//! TUI module for Scoparius.

pub mod app;
pub mod ui;
pub mod terminal;

use crate::tui::app::App;
use scoparius_core::config::ScopariusConfig;

/// Run the TUI dashboard.
pub async fn run(config: ScopariusConfig) -> anyhow::Result<()> {
    let mut terminal = terminal::init()?;
    let mut app = App::new(config);
    
    let result = app.run(&mut terminal).await;
    
    terminal::restore()?;
    result
}
