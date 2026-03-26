pub mod cli;
pub mod hook;
pub mod journal;
pub mod ledger;
pub mod map;
pub mod project;
pub mod status;
pub mod trap;

use thiserror::Error;

use colored::Colorize;

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
                let output = map::scan::scan_project(&project_root)?;
                let existing = map::read_map(&wp_dir)?;

                let report = map::check_staleness(&output.entries, &existing);
                if report.is_stale() {
                    eprintln!("Map is stale: {report}");
                    std::process::exit(1);
                }
                println!("Map is up to date ({} files)", existing.len());
            } else {
                let output = map::scan::scan_project(&project_root)?;
                let count = output.entries.len();
                let sym_count = output.symbols.len();
                map::write_map(&wp_dir, &output.entries)?;
                if let Err(e) = map::index::rebuild_symbols(&wp_dir, &output.symbols) {
                    eprintln!("Warning: symbol index failed: {e}");
                }
                println!("Scanned {count} files, {sym_count} symbols → .waypoint/map.md");
            }
            Ok(())
        }

        Command::Gain { global } => {
            let (label, stats) = if global {
                ("all projects".to_string(), ledger::gain_stats(None)?)
            } else {
                let project_root = resolve_project_root()?;
                let label = project_root.display().to_string();
                (
                    label,
                    ledger::gain_stats(Some(&project_root.to_string_lossy()))?,
                )
            };

            println!(
                "{} {} {}",
                "Waypoint Gain".bold(),
                "—".dimmed(),
                label.cyan()
            );
            print!("{stats}");
            Ok(())
        }

        Command::Trap { command } => match command {
            TrapCommand::Search { term, context } => {
                let project_root = project::resolve_with_context(context.as_deref())?;
                let wp_dir = if context.is_some() {
                    project::require_waypoint_dir(&project_root)?
                } else {
                    project::waypoint_dir(&project_root)
                };

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
                // FR-2: Resolve project from --file path; FR-3: fall back to cwd
                let (wp_dir, relative_file) =
                    if let Some(resolved) = project::resolve_foreign(&file) {
                        (resolved.wp_dir, resolved.relative_path)
                    } else {
                        let project_root = resolve_project_root()?;
                        let wp_dir = project::ensure_initialized(&project_root)?;
                        (wp_dir, file.clone())
                    };

                let new_trap = trap::NewTrap {
                    error_message: &error,
                    file: &relative_file,
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
            JournalCommand::Add {
                section,
                entry,
                context,
            } => {
                let project_root = project::resolve_with_context(context.as_deref())?;
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

        Command::Sketch { symbol, context } => {
            let project_root = project::resolve_with_context(context.as_deref())?;
            let wp_dir = project::require_waypoint_dir(&project_root)?;
            let results = map::index::sketch(&wp_dir, &symbol)?;
            if results.is_empty() {
                println!("No symbols found: {symbol}");
            } else {
                for row in &results {
                    println!(
                        "  {}:{}-{}  {}",
                        row.file_path, row.line_start, row.line_end, row.signature
                    );
                }
            }
            Ok(())
        }

        Command::Find {
            query,
            limit,
            context,
        } => {
            let project_root = project::resolve_with_context(context.as_deref())?;
            let wp_dir = project::require_waypoint_dir(&project_root)?;
            let results = map::index::find_symbols(&wp_dir, &query, limit)?;
            if results.is_empty() {
                println!("No symbols found: {query}");
            } else {
                for row in &results {
                    println!(
                        "  {:6}  {:<30}  {}:{}",
                        row.kind, row.name, row.file_path, row.line_start
                    );
                }
            }
            Ok(())
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
