pub mod cli;
pub mod hook;
pub mod ledger;
pub mod map;
pub mod project;
pub mod status;
pub mod trap;

use thiserror::Error;

use colored::Colorize;

use crate::cli::{Cli, Command, HookCommand, TrapCommand};

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
        Command::Scan { check, all, path } => {
            if all {
                return scan_all(path);
            }
            let project_root = if let Some(p) = path {
                let abs = if p.is_relative() {
                    std::env::current_dir()?.join(p)
                } else {
                    p
                };
                project::find_root(&abs).unwrap_or(abs)
            } else {
                resolve_project_root()?
            };
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
                let imp_count = output.imports.len();
                map::write_map(&wp_dir, &output.entries)?;
                if let Err(e) = map::index::rebuild_symbols(&wp_dir, &output.symbols) {
                    eprintln!("Warning: symbol index failed: {e}");
                }
                if let Err(e) = map::index::rebuild_imports(&wp_dir, &output.imports) {
                    eprintln!("Warning: import index failed: {e}");
                }
                println!(
                    "Scanned {count} files, {sym_count} symbols, {imp_count} imports → .waypoint/map.md"
                );
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
            TrapCommand::Delete { id, context } => {
                let project_root = project::resolve_with_context(context.as_deref())?;
                let wp_dir = if context.is_some() {
                    project::require_waypoint_dir(&project_root)?
                } else {
                    project::waypoint_dir(&project_root)
                };

                match trap::delete(&wp_dir, &id)? {
                    Some(entry) => println!("Deleted {} [{}]", entry.id, entry.file),
                    None => println!("No trap found with id: {id}"),
                }
                Ok(())
            }
            TrapCommand::Prune {
                older_than,
                all,
                context,
            } => {
                let days = parse_older_than(older_than.as_deref())?;
                if all {
                    return prune_all_traps(days);
                }
                let project_root = project::resolve_with_context(context.as_deref())?;
                let wp_dir = project::require_waypoint_dir(&project_root)?;
                let pruned = trap::prune(&wp_dir, days)?;
                print_pruned_traps(&pruned);
                Ok(())
            }
        },

        Command::Status { all } => {
            let project_root = resolve_project_root()?;
            if all {
                status::run_all(&project_root)
            } else {
                status::run(&project_root)
            }
        }

        Command::Sketch { symbol, context } => {
            let project_root = project::resolve_with_context(context.as_deref())?;
            let wp_dir = project::require_waypoint_dir(&project_root)?;
            let results = map::index::sketch(&wp_dir, &symbol)?;
            if results.is_empty() {
                let _ = ledger::record_event(
                    ledger::EventKind::SketchMiss,
                    project_root.to_string_lossy().as_ref(),
                    0,
                );
                println!("No symbols found: {symbol}");
            } else {
                let _ = ledger::record_event(
                    ledger::EventKind::SketchHit,
                    project_root.to_string_lossy().as_ref(),
                    0,
                );
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

        Command::Callers { symbol, context } => {
            let project_root = project::resolve_with_context(context.as_deref())?;
            let wp_dir = project::require_waypoint_dir(&project_root)?;
            let results = map::index::find_importers(&wp_dir, &symbol, None)?;
            if results.is_empty() {
                println!("No importers found for: {symbol}");
            } else {
                // Group by file, show lines per file, count unique files
                let mut by_file: std::collections::BTreeMap<&str, Vec<i64>> =
                    std::collections::BTreeMap::new();
                for (file, line) in &results {
                    by_file.entry(file).or_default().push(*line);
                }
                println!("{} file(s) import {symbol}:", by_file.len());
                for (file, lines) in &by_file {
                    let line_list: Vec<String> = lines.iter().map(ToString::to_string).collect();
                    println!("  {file}:{}", line_list.join(","));
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

fn scan_all(path: Option<std::path::PathBuf>) -> Result<(), AppError> {
    let base = match path {
        Some(p) => {
            if p.is_relative() {
                std::env::current_dir()?.join(p)
            } else {
                p
            }
        }
        None => std::env::current_dir()?,
    };

    let projects = project::discover_projects(&base)?;
    if projects.is_empty() {
        eprintln!("No git repos found under {}", base.display());
        std::process::exit(1);
    }

    eprintln!(
        "Scanning projects under {} ...",
        base.display().to_string().cyan()
    );

    let mut scanned = 0u32;
    let mut errored = 0u32;

    for root in &projects {
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );

        match scan_one_project(root) {
            Ok((files, symbols, initialized)) => {
                let init_note = if initialized { " (initialized)" } else { "" };
                eprintln!(
                    "  {} {:<24} ({files} files, {symbols} symbols{init_note})",
                    "✓".green(),
                    name,
                );
                scanned += 1;
            }
            Err(e) => {
                eprintln!("  {} {:<24} ({e})", "✗".red(), name);
                errored += 1;
            }
        }
    }

    let total = projects.len();
    let summary = format!("{total} repos found, {scanned} scanned");
    let summary = if errored > 0 {
        format!("{summary}, {errored} errored")
    } else {
        summary
    };
    eprintln!("\n{summary}");

    if errored > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Scan a single project: ensure initialized, scan files, write map + symbols.
fn scan_one_project(root: &std::path::Path) -> Result<(usize, usize, bool), AppError> {
    let initialized = !root.join(".waypoint").exists();
    let wp_dir = project::ensure_initialized(root)?;
    let output = map::scan::scan_project(root)?;
    let files = output.entries.len();
    let symbols = output.symbols.len();
    map::write_map(&wp_dir, &output.entries)?;
    if let Err(e) = map::index::rebuild_symbols(&wp_dir, &output.symbols) {
        eprintln!("    Warning: symbol index failed: {e}");
    }
    if let Err(e) = map::index::rebuild_imports(&wp_dir, &output.imports) {
        eprintln!("    Warning: import index failed: {e}");
    }
    Ok((files, symbols, initialized))
}

fn resolve_project_root() -> Result<std::path::PathBuf, AppError> {
    let cwd = std::env::current_dir()?;
    Ok(project::find_root(&cwd).unwrap_or(cwd))
}

/// Parse `--older-than` flag, requiring `Nd` format.
fn parse_older_than(value: Option<&str>) -> Result<i64, AppError> {
    match value {
        Some(s) => s
            .trim()
            .strip_suffix('d')
            .and_then(|n| n.parse::<i64>().ok())
            .filter(|&d| d > 0)
            .ok_or_else(|| {
                AppError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Invalid duration: {s}. Use Nd format, e.g. --older-than 90d"),
                ))
            }),
        None => Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Required: --older-than <duration> (e.g., --older-than 90d)",
        ))),
    }
}

/// Discover sibling projects for batch prune operations.
fn discover_prune_targets() -> Result<Vec<std::path::PathBuf>, AppError> {
    let base = std::env::current_dir()?;
    let base = project::find_root(&base)
        .and_then(|r| r.parent().map(std::path::Path::to_path_buf))
        .unwrap_or(base);

    let projects = project::discover_projects(&base)?;
    if projects.is_empty() {
        eprintln!("No git repos found under {}", base.display());
        std::process::exit(1);
    }
    Ok(projects)
}

fn prune_all_traps(days: i64) -> Result<(), AppError> {
    for root in &discover_prune_targets()? {
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let wp_dir = root.join(".waypoint");
        if !wp_dir.exists() {
            continue;
        }
        match trap::prune(&wp_dir, days) {
            Ok(pruned) if !pruned.is_empty() => {
                eprintln!(
                    "  {name}: pruned {} trap(s) older than {days}d",
                    pruned.len()
                );
            }
            Ok(_) => {}
            Err(e) => eprintln!("  {name}: error — {e}"),
        }
    }
    Ok(())
}

fn print_pruned_traps(pruned: &[trap::TrapEntry]) {
    if pruned.is_empty() {
        println!("No traps to prune");
    } else {
        println!("Pruned {} trap(s):\n", pruned.len());
        for t in pruned {
            println!("{} [{}]", t.id, t.file);
            println!("  error: {}", t.error_message);
            println!("  cause: {}", t.root_cause);
            println!("  fix:   {}", t.fix);
            println!("  tags:  {}", t.tags.join(", "));
            println!("  logged: {}", t.logged_at);
            println!();
        }
    }
}
