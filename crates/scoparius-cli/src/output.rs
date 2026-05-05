//! Output formatting for CLI.

use bytesize::ByteSize;
use colored::Colorize;
use scoparius_core::analyzer::{Analyzer, CategorySummary};
use scoparius_core::config::ScopariusConfig;
use scoparius_core::models::*;

/// Print a progress event to stderr.
pub fn print_progress_event(event: &ProgressEvent) {
    match event {
        ProgressEvent::ScanStarted { root } => {
            eprintln!("  {} {}", "→".cyan(), root.display());
        }
        ProgressEvent::ScanProgress { files_scanned, bytes_scanned, current_path, .. } => {
            eprint!("\r  {} files | {} | {}",
                format!("{files_scanned}").yellow(),
                format!("{}", ByteSize(*bytes_scanned)).green(),
                truncate_path(current_path, 50),
            );
        }
        ProgressEvent::ScanCompleted { result, duration_ms } => {
            eprintln!("\r{}", " ".repeat(120));
            eprintln!("  {} {} files ({}) in {:.1}s",
                "✓".green().bold(),
                result.total_files,
                ByteSize(result.total_size),
                *duration_ms as f64 / 1000.0,
            );
        }
        ProgressEvent::HashProgress { files_hashed, total_to_hash, .. } => {
            eprint!("\r  {} Hashing: {}/{}", "⚡".magenta(), files_hashed, total_to_hash);
        }
        ProgressEvent::CleanProgress { action_index, total_actions, current_path, .. } => {
            eprint!("\r  [{}/{}] {}", action_index + 1, total_actions, truncate_path(current_path, 60));
        }
        ProgressEvent::CleanCompleted { total_freed, actions_completed, actions_failed } => {
            eprintln!("\r{}", " ".repeat(120));
            eprintln!("  {} {} freed | {} completed | {} failed",
                "✓".green().bold(),
                ByteSize(*total_freed),
                actions_completed,
                if *actions_failed > 0 { format!("{actions_failed}").red().to_string() } else { "0".into() },
            );
        }
        _ => {}
    }
}

/// Print scan result summary with category breakdown.
pub fn print_scan_summary(result: &ScanResult, _config: &ScopariusConfig) {
    let summary = Analyzer::summary(result);

    eprintln!("\n{}", "═══ Scan Summary ═══".cyan().bold());
    eprintln!("  Total files:  {}", result.total_files);
    eprintln!("  Total size:   {}", ByteSize(result.total_size));
    eprintln!("  Reclaimable:  {}", ByteSize(result.reclaimable_size()).to_string().green().bold());

    if result.error_count > 0 {
        eprintln!("  Errors:       {}", format!("{}", result.error_count).red());
    }

    if !summary.is_empty() {
        eprintln!("\n{}", "─── By Category ───".dimmed());
        let mut sorted: Vec<&CategorySummary> = summary.values().collect();
        sorted.sort_by(|a, b| b.total_size.cmp(&a.total_size));

        for s in sorted {
            let bar = progress_bar(s.total_size, result.reclaimable_size(), 20);
            eprintln!("  {:<25} {:>8}  {:>6} files  {}",
                s.category.display_name(),
                ByteSize(s.total_size).to_string().yellow(),
                s.file_count,
                bar,
            );
        }
    }
    eprintln!();
}

/// Print a cleanup plan.
pub fn print_clean_plan(plan: &CleanPlan) {
    eprintln!("\n{}", "═══ Cleanup Plan ═══".cyan().bold());
    eprintln!("  Actions:    {}", plan.actions.len());
    eprintln!("  Freeable:   {}", ByteSize(plan.total_freeable).to_string().green().bold());
    eprintln!("  Dry run:    {}", if plan.dry_run { "yes".yellow() } else { "no".normal() });

    let risk_counts: std::collections::HashMap<RiskLevel, usize> = {
        let mut m = std::collections::HashMap::new();
        for a in &plan.actions { *m.entry(a.risk).or_insert(0) += 1; }
        m
    };

    eprintln!("\n{}", "─── Risk Breakdown ───".dimmed());
    for risk in &[RiskLevel::Safe, RiskLevel::Low, RiskLevel::Medium, RiskLevel::High] {
        let count = risk_counts.get(risk).unwrap_or(&0);
        let color = match risk {
            RiskLevel::Safe => "green",
            RiskLevel::Low => "yellow",
            RiskLevel::Medium => "yellow",
            RiskLevel::High | RiskLevel::Critical => "red",
        };
        eprintln!("  {:<10} {} action(s)", format!("{risk}").color(color), count);
    }
    eprintln!();
}

/// Print top N entries by size.
pub fn print_top_entries(result: &ScanResult, top: usize) {
    let mut entries: Vec<&FileEntry> = result.entries.iter().filter(|e| !e.is_dir).collect();
    entries.sort_by(|a, b| b.size.cmp(&a.size));

    eprintln!("\n{}", "═══ Top Entries by Size ═══".cyan().bold());
    for (i, entry) in entries.iter().take(top).enumerate() {
        eprintln!("  {:>3}. {:>10}  {}", i + 1, ByteSize(entry.size).to_string().yellow(), entry.path.display());
    }
    eprintln!();
}

/// Print disk information.
pub fn print_disks(disks: &[DiskInfo]) {
    eprintln!("\n{}", "═══ Disk Information ═══".cyan().bold());
    for disk in disks {
        let bar = progress_bar(disk.used_bytes(), disk.total_bytes, 30);
        let pct = disk.usage_percent();
        let pct_str = if pct > 90.0 { format!("{pct:.1}%").red().to_string() }
            else if pct > 75.0 { format!("{pct:.1}%").yellow().to_string() }
            else { format!("{pct:.1}%").green().to_string() };

        eprintln!("  {} ({}) [{}]", disk.mount_point.display(), disk.fs_type.dimmed(), disk.name);
        eprintln!("    {} / {} ({}) {}", ByteSize(disk.used_bytes()), ByteSize(disk.total_bytes), pct_str, bar);
    }
    eprintln!();
}

/// Print installed apps.
pub fn print_apps(apps: &[AppInfo]) {
    eprintln!("\n{}", "═══ Installed Applications ═══".cyan().bold());
    eprintln!("  Found {} application(s)\n", apps.len());
    for app in apps.iter().take(50) {
        let size_str = app.size.map(|s| ByteSize(s).to_string()).unwrap_or_else(|| "—".into());
        eprintln!("  {:<40} {:>10}  {}", app.name.white(), size_str.dimmed(),
            app.publisher.as_deref().unwrap_or("").dimmed());
    }
    if apps.len() > 50 { eprintln!("  ... and {} more", apps.len() - 50); }
    eprintln!();
}

/// Print duplicate files.
pub fn print_duplicates(duplicates: &[scoparius_core::analyzer::duplicates::DuplicateGroup]) {
    let total_wasted: u64 = duplicates.iter().map(|g| g.wasted_bytes).sum();
    eprintln!("\n{}", "═══ Duplicate Files ═══".cyan().bold());
    eprintln!("  Found {} group(s) wasting {}\n", duplicates.len(), ByteSize(total_wasted).to_string().red().bold());

    for (i, group) in duplicates.iter().take(20).enumerate() {
        eprintln!("  {} {} ({} copies, {} wasted)",
            format!("{}.", i + 1).dimmed(),
            &group.hash[0..8].magenta(),
            group.paths.len(),
            ByteSize(group.wasted_bytes).to_string().yellow(),
        );
        for path in &group.paths {
            eprintln!("    {} {}", "↳".dimmed(), path.display());
        }
        eprintln!();
    }
    
    if duplicates.len() > 20 {
        eprintln!("  ... and {} more groups", duplicates.len() - 20);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn progress_bar(current: u64, total: u64, width: usize) -> String {
    if total == 0 { return " ".repeat(width); }
    let filled = ((current as f64 / total as f64) * width as f64) as usize;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled).cyan(), "░".repeat(empty).dimmed())
}

fn truncate_path(path: &std::path::Path, max_len: usize) -> String {
    let s = path.to_string_lossy();
    if s.len() <= max_len { s.to_string() }
    else { format!("...{}", &s[s.len() - max_len + 3..]) }
}
