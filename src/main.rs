mod db;
mod scan;
mod top;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    /// Scan a filesystem tree into the local index.
    Scan {
        /// Root path to scan.
        root: PathBuf,

        /// Do not cross filesystem/device boundaries.
        #[arg(long)]
        one_file_system: bool,

        /// Include Linux pseudo-filesystems such as /proc, /sys, /dev, /run.
        #[arg(long)]
        include_pseudo: bool,
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
        } => {
            let root = util::normalize_existing_path(&root)?;
            let opts = scan::ScanOptions {
                one_file_system,
                include_pseudo,
            };
            let result = scan::scan_path(&mut database, &root, opts)?;
            println!("scan_id: {}", result.scan_id);
            println!("root: {}", root.display());
            println!("entries: {}", result.entries);
            println!("errors: {}", result.errors);
            println!(
                "indexed_size: {}",
                util::format_bytes(result.total_allocated_size)
            );
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
    }

    Ok(())
}
