pub mod cli;
pub mod hook;
pub mod journal;
pub mod ledger;
pub mod map;
pub mod project;
pub mod status;
pub mod trap;

use thiserror::Error;

use crate::cli::{Cli, Command, HookCommand, JournalCommand, TrapCommand};

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("ledger: {0}")]
    Ledger(String),
}

#[allow(clippy::too_many_lines)] // CLI dispatch — flat match arms, not deep nesting
pub fn run(cli: Cli) -> Result<(), AppError> {
    match cli.command {
        Command::Scan { check } => {
            let project_root = resolve_project_root()?;
            let wp_dir = project::ensure_initialized(&project_root)?;

            if check {
                let current = map::scan::scan_project(&project_root)?;
                let existing = map::read_map(&wp_dir)?;

                let stale = map::check_staleness(&current, &existing);
                if !stale.is_empty() {
                    eprintln!("Map is stale: {stale}");
                    std::process::exit(1);
                }
                println!("Map is up to date ({} files)", existing.len());
            } else {
                let entries = map::scan::scan_project(&project_root)?;
                let count = entries.len();
                map::write_map(&wp_dir, &entries)?;
                println!("Scanned {count} files → .waypoint/map.md");
            }
            Ok(())
        }

        Command::Gain => {
            let project_root = resolve_project_root()?;
            let stats = ledger::gain_stats(Some(&project_root.to_string_lossy()))?;

            println!("Waypoint Gain — {}\n", project_root.display());
            println!("Total events:        {}", stats.total_events);
            println!("Map hits:            {}", stats.map_hits);
            println!("Map misses:          {}", stats.map_misses);
            println!("Map hit rate:        {:.1}%", stats.map_hit_rate);
            println!("Trap hits:           {}", stats.trap_hits);
            println!("Est. tokens saved:   {}", stats.estimated_tokens_saved);

            if !stats.daily.is_empty() {
                println!("\nDaily breakdown:");
                for day in &stats.daily {
                    println!(
                        "  {} — {} events, ~{} tokens saved",
                        day.date, day.events, day.tokens_saved
                    );
                }
            }
            Ok(())
        }

        Command::Trap { command } => match command {
            TrapCommand::Search { term } => {
                let project_root = resolve_project_root()?;
                let wp_dir = project::waypoint_dir(&project_root);

                let traps = trap::read_traps(&wp_dir)?;
                let results = trap::search(&traps, &term);

                if results.is_empty() {
                    println!("No traps found for: {term}");
                } else {
                    for t in &results {
                        println!("{} [{}]", t.id, t.file);
                        println!("  error: {}", t.error_message);
                        println!("  cause: {}", t.root_cause);
                        println!("  fix:   {}", t.fix);
                        println!("  tags:  {}", t.tags.join(", "));
                        println!();
                    }
                }
                Ok(())
            }
            TrapCommand::Log {
                error,
                file,
                cause,
                fix,
                tags,
            } => {
                let project_root = resolve_project_root()?;
                let wp_dir = project::ensure_initialized(&project_root)?;

                let new_trap = trap::NewTrap {
                    error_message: &error,
                    file: &file,
                    root_cause: &cause,
                    fix: &fix,
                    tags_str: &tags,
                };
                match trap::log_trap(&wp_dir, &new_trap)? {
                    Some(warning) => println!("{warning}"),
                    None => println!("Trap logged"),
                }
                Ok(())
            }
        },

        Command::Journal { command } => match command {
            JournalCommand::Add { section, entry } => {
                let project_root = resolve_project_root()?;
                let wp_dir = project::ensure_initialized(&project_root)?;

                journal::add_entry(&wp_dir, section, &entry)?;
                println!("Added to journal");
                Ok(())
            }
        },

        Command::Status => {
            let project_root = resolve_project_root()?;
            status::run(&project_root)
        }

        Command::Hook { command } => match command {
            HookCommand::PreRead => hook::pre_read::run(),
            HookCommand::SessionStart => hook::session_start::run(),
            HookCommand::PreWrite => hook::pre_write::run(),
            HookCommand::PostWrite => hook::post_write::run(),
            HookCommand::PostFailure => hook::post_failure::run(),
        },
    }
}

fn resolve_project_root() -> Result<std::path::PathBuf, AppError> {
    let cwd = std::env::current_dir()?;
    Ok(project::find_root(&cwd).unwrap_or(cwd))
}
