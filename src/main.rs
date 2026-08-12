mod actions;
mod adapters;
mod analyze;
mod classify;
mod clean;
mod db;
mod explain;
mod mutation_lock;
mod plan;
mod recover;
mod restore;
mod rules;
mod scan;
mod top;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use tidyfs::ai_contract::AiPathMode;

#[derive(Debug, Parser)]
#[command(name = "tidyfs")]
#[command(about = "Conservative disk usage scanner and cleanup planner")]
struct Cli {
    /// SQLite database path. Defaults to ~/.local/share/tidyfs/tidyfs.db
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Scan a filesystem tree into the local index and classify known paths.
    Scan {
        /// Root path to scan.
        root: PathBuf,

        /// Do not cross filesystem/device boundaries.
        #[arg(long)]
        one_file_system: bool,

        /// Include Linux pseudo-filesystems such as /proc, /sys, /dev, /run.
        #[arg(long)]
        include_pseudo: bool,

        /// Number of parallel scanner workers. Defaults to available parallelism.
        #[arg(long)]
        jobs: Option<usize>,
    },

    /// Run deterministic classification for an existing scan.
    Classify {
        /// Scan id to classify. Defaults to latest completed scan.
        #[arg(long)]
        scan_id: Option<i64>,

        /// Print classification counts by label.
        #[arg(long)]
        summary: bool,
    },

    /// Ask a local AI gateway for bounded, non-mutating analysis of classified scan facts.
    Analyze {
        /// Explicit numeric loopback gateway endpoint, for example http://127.0.0.1:8000.
        #[arg(long)]
        endpoint: String,

        /// Scan id to analyze. Defaults to latest completed scan.
        #[arg(long)]
        scan_id: Option<i64>,

        /// Restrict analysis to a subtree of the selected scan.
        #[arg(long)]
        root: Option<PathBuf>,

        /// Maximum number of classified paths to send for analysis (1..=100).
        #[arg(long, default_value_t = 10)]
        limit: usize,

        /// Path disclosure mode for facts sent to inference.
        #[arg(long, value_enum, default_value_t = CliAiPathMode::Full)]
        path_mode: CliAiPathMode,

        /// Deterministic maximum-risk context supplied to the model.
        #[arg(long, value_enum, default_value_t = CliRisk::Low)]
        risk: CliRisk,

        /// Gateway connection timeout in milliseconds.
        #[arg(long, default_value_t = 3000)]
        connect_timeout_ms: u64,

        /// Gateway read/write timeout in milliseconds.
        #[arg(long, default_value_t = 15000)]
        timeout_ms: u64,

        /// Maximum accepted gateway response body size in bytes.
        #[arg(long, default_value_t = 65536)]
        max_response_bytes: usize,
    },

    /// Show largest indexed directories from the latest scan by default.
    Top {
        /// Scan id to inspect. Defaults to latest completed scan.
        #[arg(long)]
        scan_id: Option<i64>,

        /// Limit number of rows.
        #[arg(long, default_value_t = 25)]
        limit: usize,

        /// Only show directories at or below this relative depth.
        #[arg(long)]
        depth: Option<usize>,

        /// Restrict output to a subtree.
        #[arg(long)]
        root: Option<PathBuf>,
    },

    /// Explain what a path appears to be using deterministic classifications.
    Explain {
        /// Path to explain.
        path: PathBuf,

        /// Scan id to inspect. Defaults to latest completed scan.
        #[arg(long)]
        scan_id: Option<i64>,

        /// Include child classifications directly under this path.
        #[arg(long)]
        children: bool,
    },

    /// Build a read-only cleanup plan from rules and policy.
    Plan {
        /// Scan id to inspect. Defaults to latest completed scan.
        #[arg(long)]
        scan_id: Option<i64>,

        /// Equivalent to --risk low.
        #[arg(long)]
        safe: bool,

        /// Maximum allowed risk for candidates.
        #[arg(long, value_enum, default_value_t = CliRisk::Low)]
        risk: CliRisk,

        /// Restrict output to a subtree.
        #[arg(long)]
        root: Option<PathBuf>,

        /// Include blocked/report-only findings.
        #[arg(long, default_value_t = true)]
        include_blocked: bool,

        /// Include read-only tool-native adapter candidates.
        #[arg(long)]
        include_adapters: bool,

        /// Limit printed allowed candidates.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },

    /// Preview allowed cleanup candidates without touching the filesystem.
    Clean {
        /// Scan id to inspect. Defaults to latest completed scan.
        #[arg(long)]
        scan_id: Option<i64>,

        /// Preview only. No filesystem changes are made.
        #[arg(long)]
        dry_run: bool,

        /// Required for reversible cleanup execution.
        #[arg(long)]
        interactive: bool,

        /// Equivalent to --risk low.
        #[arg(long)]
        safe: bool,

        /// Maximum allowed risk for dry-run preview.
        #[arg(long, value_enum, default_value_t = CliRisk::Low)]
        risk: CliRisk,

        /// Restrict output to a subtree.
        #[arg(long)]
        root: Option<PathBuf>,

        /// Limit printed candidates.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },

    /// Inspect available tool-native adapters.
    Adapters,

    /// List recorded cleanup/restore actions.
    Actions {
        /// Maximum number of actions to show.
        #[arg(long, default_value_t = 25)]
        limit: usize,
    },

    /// Reconcile interrupted cleanup or restore actions without moving files.
    Recover {
        /// Recover a specific interrupted action id.
        #[arg(long)]
        action: Option<i64>,

        /// Recover all interrupted actions.
        #[arg(long)]
        all: bool,
    },

    /// Restore a quarantined action.
    Restore {
        /// Restore a specific action id.
        #[arg(long)]
        action: Option<i64>,

        /// Restore the latest quarantined action.
        #[arg(long)]
        latest: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliRisk {
    Low,
    Medium,
    High,
    Forbidden,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliAiPathMode {
    Full,
    Basename,
    Redacted,
}

impl From<CliRisk> for rules::Risk {
    fn from(value: CliRisk) -> Self {
        match value {
            CliRisk::Low => rules::Risk::Low,
            CliRisk::Medium => rules::Risk::Medium,
            CliRisk::High => rules::Risk::High,
            CliRisk::Forbidden => rules::Risk::Forbidden,
        }
    }
}

impl From<CliAiPathMode> for AiPathMode {
    fn from(value: CliAiPathMode) -> Self {
        match value {
            CliAiPathMode::Full => AiPathMode::Full,
            CliAiPathMode::Basename => AiPathMode::Basename,
            CliAiPathMode::Redacted => AiPathMode::Redacted,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = util::resolve_db_path(cli.db)?;
    let mut database = db::Database::open(&db_path)?;
    database.migrate()?;

    match cli.command {
        Command::Scan {
            root,
            one_file_system,
            include_pseudo,
            jobs,
        } => {
            let root = util::normalize_existing_path(&root)?;
            let opts = scan::ScanOptions {
                one_file_system,
                include_pseudo,
                jobs,
            };
            let result = scan::scan_path(&mut database, &root, opts)?;
            let classified = classify::classify_scan(&mut database, result.scan_id)?;

            println!("scan_id: {}", result.scan_id);
            println!("root: {}", root.display());
            println!("entries: {}", result.entries);
            println!("errors: {}", result.errors);
            println!("classifications: {}", classified.classifications);
            println!(
                "indexed_size: {}",
                util::format_bytes(result.total_allocated_size)
            );
        }
        Command::Classify { scan_id, summary } => {
            let scan_id = database.resolve_scan_id(scan_id)?;
            let result = classify::classify_scan(&mut database, scan_id)?;
            println!("scan_id: {scan_id}");
            println!("classifications: {}", result.classifications);

            if summary {
                classify::print_classification_summary(&database, scan_id)?;
            }
        }
        Command::Analyze {
            endpoint,
            scan_id,
            root,
            limit,
            path_mode,
            risk,
            connect_timeout_ms,
            timeout_ms,
            max_response_bytes,
        } => {
            analyze::run_analyze(
                &database,
                analyze::AnalyzeQuery {
                    endpoint,
                    scan_id,
                    root,
                    limit,
                    path_mode: path_mode.into(),
                    max_risk: risk.into(),
                    connect_timeout_ms,
                    timeout_ms,
                    max_response_bytes,
                },
            )?;
        }
        Command::Top {
            scan_id,
            limit,
            depth,
            root,
        } => {
            let query = top::TopQuery {
                scan_id,
                limit,
                depth,
                root,
            };
            top::print_top(&database, query)?;
        }
        Command::Explain {
            path,
            scan_id,
            children,
        } => {
            let query = explain::ExplainQuery {
                scan_id,
                path,
                children,
            };
            explain::print_explanation(&database, query)?;
        }
        Command::Plan {
            scan_id,
            safe,
            risk,
            root,
            include_blocked,
            include_adapters,
            limit,
        } => {
            let max_risk = if safe { rules::Risk::Low } else { risk.into() };
            let query = plan::PlanQuery {
                scan_id,
                max_risk,
                root,
                include_blocked,
                include_adapters,
                limit,
            };
            plan::run_plan(&mut database, query)?;
        }
        Command::Clean {
            scan_id,
            dry_run,
            interactive,
            safe,
            risk,
            root,
            limit,
        } => {
            let max_risk = if safe { rules::Risk::Low } else { risk.into() };
            let query = clean::CleanQuery {
                scan_id,
                dry_run,
                safe,
                interactive,
                max_risk,
                root,
                limit,
            };
            if dry_run {
                clean::run_clean(&database, query)?;
            } else {
                let _lock = mutation_lock::MutationLock::acquire(&db_path)?;
                clean::run_clean(&database, query)?;
            }
        }
        Command::Adapters => {
            adapters::print_adapters();
        }
        Command::Actions { limit } => {
            actions::print_actions(&database, actions::ActionsQuery { limit })?;
        }
        Command::Recover { action, all } => {
            let _lock = mutation_lock::MutationLock::acquire(&db_path)?;
            recover::run_recover(
                &database,
                recover::RecoverQuery {
                    action_id: action,
                    all,
                },
            )?;
        }
        Command::Restore { action, latest } => {
            let _lock = mutation_lock::MutationLock::acquire(&db_path)?;
            restore::run_restore(
                &database,
                restore::RestoreQuery {
                    action_id: action,
                    latest,
                },
            )?;
        }
    }

    Ok(())
}
