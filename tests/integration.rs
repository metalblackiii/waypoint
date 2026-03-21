#![allow(clippy::unwrap_used)]

//! Integration tests for waypoint CLI commands and Claude Code hook contracts.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// Create a minimal project with .git marker and a Rust source file.
fn setup_project() -> TempDir {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join(".git")).unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src/main.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();
    tmp
}

fn waypoint() -> Command {
    Command::cargo_bin("waypoint").unwrap()
}

/// Build a hook stdin payload with `cwd` and `file_path`.
fn hook_payload(project: &TempDir, file_path: &str) -> String {
    serde_json::json!({
        "cwd": project.path().to_string_lossy(),
        "tool_input": {
            "file_path": project.path().join(file_path).to_string_lossy().as_ref()
        }
    })
    .to_string()
}

/// Parse hook JSON output from stdout, returning the hookSpecificOutput object.
fn parse_hook_output(assert: &assert_cmd::assert::Assert) -> serde_json::Value {
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    json["hookSpecificOutput"].clone()
}

// ── CLI Round-Trip Tests ─────────────────────────────────────────

#[test]
fn cli_scan_creates_map() {
    let project = setup_project();

    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Scanned"))
        .stdout(predicate::str::contains("map.md"));

    let map = fs::read_to_string(project.path().join(".waypoint/map.md")).unwrap();
    assert!(
        map.contains("main.rs"),
        "map should list main.rs, got:\n{map}"
    );
}

#[test]
fn cli_scan_check_up_to_date() {
    let project = setup_project();
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    waypoint()
        .args(["scan", "--check"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("up to date"));
}

#[test]
fn cli_scan_check_detects_stale() {
    let project = setup_project();
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    // Add a new file after scanning
    fs::write(project.path().join("src/lib.rs"), "pub fn hello() {}\n").unwrap();

    waypoint()
        .args(["scan", "--check"])
        .current_dir(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("stale"));
}

#[test]
fn cli_journal_add() {
    let project = setup_project();

    waypoint()
        .args([
            "journal",
            "add",
            "--section",
            "learnings",
            "integration test entry",
        ])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Added to journal"));

    let journal = fs::read_to_string(project.path().join(".waypoint/journal.md")).unwrap();
    assert!(journal.contains("integration test entry"));
}

#[test]
fn cli_trap_log_and_search() {
    let project = setup_project();

    waypoint()
        .args([
            "trap",
            "log",
            "--error",
            "TypeError: undefined is not a function",
            "--file",
            "src/main.rs",
            "--cause",
            "null reference",
            "--fix",
            "added null check",
            "--tags",
            "null,error",
        ])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Trap logged"));

    waypoint()
        .args(["trap", "search", "TypeError"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("TypeError"))
        .stdout(predicate::str::contains("null check"));
}

#[test]
fn cli_trap_search_no_results() {
    let project = setup_project();

    waypoint()
        .args(["trap", "search", "nonexistent"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No traps found"));
}

#[test]
fn cli_status() {
    let project = setup_project();

    waypoint()
        .arg("status")
        .current_dir(project.path())
        .assert()
        .success();
}

// ── Hook Integration Tests ───────────────────────────────────────

#[test]
fn hook_session_start_outputs_journal_and_creates_map() {
    let project = setup_project();
    let payload = serde_json::json!({
        "cwd": project.path().to_string_lossy()
    })
    .to_string();

    waypoint()
        .args(["hook", "session-start"])
        .write_stdin(payload)
        .assert()
        .success()
        // Session-start uses plain stdout, not JSON wrapper
        .stdout(predicate::str::contains("Waypoint Journal"))
        .stdout(predicate::str::contains("waypoint journal add"))
        .stdout(predicate::str::contains("waypoint trap log"));

    // FR-22: Auto-scan creates map.md on first session
    assert!(
        project.path().join(".waypoint/map.md").exists(),
        "session-start should auto-create map.md"
    );
}

#[test]
fn hook_pre_read_returns_map_context() {
    let project = setup_project();
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    let payload = hook_payload(&project, "src/main.rs");

    let assert = waypoint()
        .args(["hook", "pre-read"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    assert_eq!(hook["hookEventName"], "PreToolUse");
    let ctx = hook["additionalContext"].as_str().unwrap();
    assert!(ctx.contains("[waypoint] map:"), "got: {ctx}");
    assert!(ctx.contains("main.rs"), "got: {ctx}");
}

#[test]
fn hook_pre_read_no_context_for_unknown_file() {
    let project = setup_project();
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    let payload = hook_payload(&project, "src/nonexistent.rs");

    let assert = waypoint()
        .args(["hook", "pre-read"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    assert_eq!(hook["hookEventName"], "PreToolUse");
    assert!(
        hook.get("additionalContext").is_none(),
        "unknown file should have no context, got: {hook}"
    );
}

#[test]
fn hook_pre_write_surfaces_traps() {
    let project = setup_project();

    // Log a trap for src/main.rs
    waypoint()
        .args([
            "trap",
            "log",
            "--error",
            "null reference error",
            "--file",
            "src/main.rs",
            "--cause",
            "missing null check",
            "--fix",
            "added optional chaining",
            "--tags",
            "null",
        ])
        .current_dir(project.path())
        .assert()
        .success();

    let payload = hook_payload(&project, "src/main.rs");

    let assert = waypoint()
        .args(["hook", "pre-write"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    assert_eq!(hook["hookEventName"], "PreToolUse");
    let ctx = hook["additionalContext"].as_str().unwrap();
    assert!(ctx.contains("[waypoint] traps for"), "got: {ctx}");
    assert!(ctx.contains("null reference error"), "got: {ctx}");
}

#[test]
fn hook_pre_write_no_traps_for_clean_file() {
    let project = setup_project();
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    let payload = hook_payload(&project, "src/main.rs");

    let assert = waypoint()
        .args(["hook", "pre-write"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    assert_eq!(hook["hookEventName"], "PreToolUse");
    assert!(
        hook.get("additionalContext").is_none(),
        "clean file should have no trap context, got: {hook}"
    );
}

#[test]
fn hook_post_write_updates_map() {
    let project = setup_project();
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    // Modify file to add a new function
    fs::write(
        project.path().join("src/main.rs"),
        "fn main() {}\n\npub fn helper() {}\n",
    )
    .unwrap();

    let payload = hook_payload(&project, "src/main.rs");

    let assert = waypoint()
        .args(["hook", "post-write"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    assert_eq!(hook["hookEventName"], "PostToolUse");
    let ctx = hook["additionalContext"].as_str().unwrap();
    assert!(
        ctx.contains("[waypoint] map updated: src/main.rs"),
        "got: {ctx}"
    );

    // Verify map.md was actually updated with the new function
    let map = fs::read_to_string(project.path().join(".waypoint/map.md")).unwrap();
    assert!(
        map.contains("helper"),
        "map should reflect updated file, got:\n{map}"
    );
}

#[test]
fn hook_post_write_ignores_out_of_project_file() {
    let project = setup_project();
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    // Payload with a file outside the project
    let payload = serde_json::json!({
        "cwd": project.path().to_string_lossy(),
        "tool_input": {
            "file_path": "/tmp/outside_project.rs"
        }
    })
    .to_string();

    let assert = waypoint()
        .args(["hook", "post-write"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    assert_eq!(hook["hookEventName"], "PostToolUse");
    // No map update for out-of-project files
    assert!(
        hook.get("additionalContext").is_none()
            || !hook["additionalContext"]
                .as_str()
                .unwrap_or("")
                .contains("map updated"),
        "out-of-project file should not trigger map update"
    );
}

#[test]
fn hook_post_failure_suggests_trap_search() {
    let project = setup_project();
    let payload = hook_payload(&project, "src/main.rs");

    let assert = waypoint()
        .args(["hook", "post-failure"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    assert_eq!(hook["hookEventName"], "PostToolUse");
    let ctx = hook["additionalContext"].as_str().unwrap();
    assert!(ctx.contains("waypoint trap search"), "got: {ctx}");
    assert!(ctx.contains("main.rs"), "got: {ctx}");
}
