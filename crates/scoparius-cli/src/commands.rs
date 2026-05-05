//! CLI command definitions.

use clap::Subcommand;
use scoparius_core::config::ScopariusConfig;
use scoparius_core::scanner::{ScanConfig, Scanner};
use scoparius_core::analyzer::Analyzer;
use scoparius_core::cleaner::Cleaner;
use scoparius_core::platform;
use std::path::PathBuf;
use crate::output;

#[derive(Subcommand)]
pub enum Commands {
    /// Scan filesystem and build cleanup report
    Scan {
        /// Paths to scan (defaults to all mounted disks)
        #[arg(value_name = "PATH")]
        paths: Vec<PathBuf>,

        /// Only include files not accessed in N days (e.g., 90d, 180d)
        #[arg(long, value_name = "DURATION")]
        older_than: Option<String>,

        /// Maximum directory depth (0 = unlimited)
        #[arg(long, default_value = "0")]
        max_depth: usize,

        /// Minimum file size to include (e.g., 1KB, 10MB)
        #[arg(long)]
        min_size: Option<String>,

        /// Skip hidden files and directories
        #[arg(long)]
        skip_hidden: bool,

        /// Exclude paths matching these patterns
        #[arg(long)]
        exclude: Vec<String>,
    },

    /// Execute cleanup based on scan results
    Clean {
        /// Paths to scan and clean
        #[arg(value_name = "PATH")]
        paths: Vec<PathBuf>,

        /// Only clean specific categories (comma-separated)
        #[arg(long, value_delimiter = ',')]
        category: Vec<String>,

        /// Auto-approve all safe operations
        #[arg(long)]
        auto_approve: bool,
    },

    /// Show disk usage analysis
    Analyze {
        /// Path to analyze
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Show top N largest entries
        #[arg(long, default_value = "20")]
        top: usize,

        /// Sort by: size, name, age
        #[arg(long, default_value = "size")]
        sort: String,
    },

    /// List installed applications with usage stats
    Apps {
        /// Show only unused apps
        #[arg(long)]
        unused: bool,

        /// Threshold for "unused" (days)
        #[arg(long, default_value = "180")]
        older_than: u64,
    },

    /// Show system disk information
    Disks,

    /// Find duplicate files
    Duplicates {
        /// Path to analyze
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Minimum file size to consider (e.g., 1MB)
        #[arg(long, default_value = "1048576")] // 1MB default
        min_size: u64,
    },

    /// Manage undo manifests and history
    Manifests {
        #[command(subcommand)]
        action: ManifestAction,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current configuration
    Show,
    /// Show config file path
    Path,
    /// Initialize default config
    Init,
}

#[derive(Subcommand)]
pub enum ManifestAction {
    /// List all cleanup manifests
    List,
    /// Show details of a specific manifest
    Show {
        /// Manifest UUID
        id: uuid::Uuid,
    },
    /// Restore quarantined files from a manifest
    Restore {
        /// Manifest UUID
        id: uuid::Uuid,
    },
}

/// Run the selected command.
pub async fn run(
    cmd: Commands, config: ScopariusConfig, dry_run: bool, json: bool, quiet: bool,
) -> anyhow::Result<()> {
    match cmd {
        Commands::Scan { paths, older_than: _, max_depth, min_size: _, skip_hidden, exclude } => {
            let roots = if paths.is_empty() {
                let adapter = platform::create_adapter();
                adapter.list_disks()?.into_iter().map(|d| d.mount_point).collect()
            } else {
                paths
            };

            let scan_config = ScanConfig {
                roots: roots.clone(),
                max_depth,
                skip_hidden,
                exclude_patterns: exclude,
                ..Default::default()
            };

            let scanner = Scanner::new(scan_config);

            if !quiet {
                eprintln!("🔍 Scanning {} path(s)...", roots.len());
            }

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let quiet_clone = quiet;
            let progress_handle = tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    if !quiet_clone {
                        output::print_progress_event(&event);
                    }
                }
            });

            let mut result = scanner.scan(Some(tx)).await?;
            progress_handle.await?;

            // Analyze results
            let analyzer = Analyzer::new(config.thresholds.clone());
            analyzer.analyze(&mut result);

            // Output
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                output::print_scan_summary(&result, &config);
            }
        }

        Commands::Clean { paths, category: _, auto_approve } => {
            let roots = if paths.is_empty() { vec![PathBuf::from(".")] } else { paths };

            let scan_config = ScanConfig { roots, ..Default::default() };
            let scanner = Scanner::new(scan_config);

            if !quiet { eprintln!("🔍 Scanning..."); }
            let mut result = scanner.scan(None).await?;

            let analyzer = Analyzer::new(config.thresholds.clone());
            analyzer.analyze(&mut result);

            let cleaner = Cleaner::new(config);
            let mut plan = cleaner.plan(&result, dry_run);

            if json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                output::print_clean_plan(&plan);
            }

            if dry_run {
                eprintln!("\n💡 Dry run — no files were modified. Remove --dry-run to execute.");
                return Ok(());
            }
            
            if !auto_approve {
                use dialoguer::Confirm;
                let proceed = Confirm::new()
                    .with_prompt("Execute this cleanup plan?")
                    .default(false)
                    .interact()?;
                    
                if !proceed {
                    eprintln!("Cleanup aborted.");
                    return Ok(());
                }
            }
            
            // Mark actions as approved
            for action in &mut plan.actions {
                if action.status == scoparius_core::models::ActionStatus::Pending {
                    action.status = scoparius_core::models::ActionStatus::Approved;
                }
            }
            
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let quiet_clone = quiet;
            let progress_handle = tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    if !quiet_clone {
                        output::print_progress_event(&event);
                    }
                }
            });
            
            cleaner.execute(plan, Some(tx)).await?;
            progress_handle.await?;
        }

        Commands::Manifests { action } => match action {
            ManifestAction::List => {
                let cleaner = Cleaner::new(config);
                let manifests = cleaner.list_manifests()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&manifests)?);
                } else {
                    for m in manifests {
                        println!("ID: {} | Created: {} | Total Freed: {} | Actions: {}", 
                            m.id, 
                            m.created_at.format("%Y-%m-%d %H:%M:%S"),
                            bytesize::ByteSize(m.total_freeable),
                            m.actions.len()
                        );
                    }
                }
            }
            ManifestAction::Show { id } => {
                let cleaner = Cleaner::new(config);
                let manifest = cleaner.read_manifest(id)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&manifest)?);
                } else {
                    println!("Manifest ID: {}", manifest.id);
                    println!("Created At: {}", manifest.created_at);
                    println!("Actions:");
                    for a in manifest.actions {
                        println!("  [{:?}] {:?} -> {}", a.status, a.operation, a.path.display());
                    }
                }
            }
            ManifestAction::Restore { id } => {
                let cleaner = Cleaner::new(config);
                if !quiet { eprintln!("Restoring files from manifest {}...", id); }
                let restored = cleaner.restore_manifest(id)?;
                if !quiet { eprintln!("Successfully restored {} file(s).", restored); }
            }
        }

        Commands::Analyze { path, top, sort: _ } => {
            let scan_config = ScanConfig { roots: vec![path], ..Default::default() };
            let scanner = Scanner::new(scan_config);

            if !quiet { eprintln!("📊 Analyzing..."); }
            let result = scanner.scan(None).await?;
            output::print_top_entries(&result, top);
        }

        Commands::Apps { unused: _, older_than: _ } => {
            let adapter = platform::create_adapter();
            let apps = adapter.list_installed_apps()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&apps)?);
            } else {
                output::print_apps(&apps);
            }
        }

        Commands::Disks => {
            let adapter = platform::create_adapter();
            let disks = adapter.list_disks()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&disks)?);
            } else {
                output::print_disks(&disks);
            }
        }

        Commands::Duplicates { path, min_size } => {
            let scan_config = ScanConfig { roots: vec![path], ..Default::default() };
            let scanner = Scanner::new(scan_config);

            if !quiet { eprintln!("🔍 Scanning for files..."); }
            let result = scanner.scan(None).await?;

            if !quiet { eprintln!("🧬 Hashing and detecting duplicates..."); }
            let duplicates = scoparius_core::analyzer::duplicates::find_duplicates(&result.entries, min_size, None)?;
            
            if json {
                // Temporary struct to make JSON output nicer
                #[derive(serde::Serialize)]
                struct DupOutput {
                    total_wasted: u64,
                    groups: Vec<scoparius_core::analyzer::duplicates::DuplicateGroup>,
                }
                let out = DupOutput {
                    total_wasted: duplicates.iter().map(|g| g.wasted_bytes).sum(),
                    groups: duplicates,
                };
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                output::print_duplicates(&duplicates);
            }
        }

        Commands::Config { action } => match action {
            ConfigAction::Show => println!("{}", toml::to_string_pretty(&config)?),
            ConfigAction::Path => println!("{}", ScopariusConfig::default_path().display()),
            ConfigAction::Init => { config.save()?; eprintln!("✅ Config saved to {}", ScopariusConfig::default_path().display()); }
        },
    }
    Ok(())
}
