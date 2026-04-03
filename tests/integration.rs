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
fn cli_trap_delete_removes_entry() {
    let project = setup_project();

    // Log a trap first
    waypoint()
        .args([
            "trap",
            "log",
            "--error",
            "test error",
            "--file",
            "src/main.rs",
            "--cause",
            "test cause",
            "--fix",
            "test fix",
            "--tags",
            "test",
        ])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Trap logged"));

    // Search to get the ID
    let output = waypoint()
        .args(["trap", "search", "test error"])
        .current_dir(project.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let id = stdout
        .lines()
        .find(|l| l.starts_with("trap-"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();

    // Delete it
    waypoint()
        .args(["trap", "delete", id])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted"))
        .stdout(predicate::str::contains(id));

    // Verify it's gone
    waypoint()
        .args(["trap", "search", "test error"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No traps found"));
}

#[test]
fn cli_trap_delete_nonexistent_id() {
    let project = setup_project();

    waypoint()
        .args(["trap", "delete", "trap-00000000"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No trap found with id"));
}

#[test]
fn cli_trap_delete_with_context_flag() {
    let project = setup_project();

    // Log a trap
    waypoint()
        .args([
            "trap",
            "log",
            "--error",
            "ctx error",
            "--file",
            "src/main.rs",
            "--cause",
            "ctx cause",
            "--fix",
            "ctx fix",
            "--tags",
            "ctx",
        ])
        .current_dir(project.path())
        .assert()
        .success();

    // Search with -C to get the ID
    let output = waypoint()
        .args([
            "trap",
            "search",
            "ctx error",
            "-C",
            &project.path().display().to_string(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let id = stdout
        .lines()
        .find(|l| l.starts_with("trap-"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();

    // Delete with -C from a different cwd
    waypoint()
        .args([
            "trap",
            "delete",
            id,
            "-C",
            &project.path().display().to_string(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted"));
}

#[test]
fn cli_gain_shows_project_stats() {
    let project = setup_project();

    waypoint()
        .arg("gain")
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Waypoint Gain"));
}

#[test]
fn cli_gain_global_works_outside_project() {
    // --global should not require a project root
    let tmp = TempDir::new().unwrap();

    waypoint()
        .args(["gain", "--global"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Waypoint Gain"))
        .stdout(predicate::str::contains("all projects"));
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

// ── Cross-Project Hook Tests ─────────────────────────────────────

#[test]
fn hook_pre_read_cross_project_lookup() {
    // Project A is the cwd project; project B has a scanned map.
    let project_a = setup_project();
    let project_b = setup_project();

    // Scan project B so it has a .waypoint/map.md
    waypoint()
        .arg("scan")
        .current_dir(project_b.path())
        .assert()
        .success();

    // Read a file in project B while cwd is project A
    let payload = serde_json::json!({
        "cwd": project_a.path().to_string_lossy(),
        "tool_input": {
            "file_path": project_b.path().join("src/main.rs").to_string_lossy().as_ref()
        }
    })
    .to_string();

    let assert = waypoint()
        .args(["hook", "pre-read"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    assert_eq!(hook["hookEventName"], "PreToolUse");
    let ctx = hook["additionalContext"].as_str().unwrap();
    assert!(
        ctx.contains("[waypoint] map:"),
        "expected map context, got: {ctx}"
    );
    assert!(
        ctx.contains("main.rs"),
        "expected main.rs in context, got: {ctx}"
    );
}

#[test]
fn hook_pre_read_cross_project_no_waypoint() {
    // Project A is the cwd; project B exists but has no .waypoint/
    let project_a = setup_project();
    let project_b = setup_project();

    let payload = serde_json::json!({
        "cwd": project_a.path().to_string_lossy(),
        "tool_input": {
            "file_path": project_b.path().join("src/main.rs").to_string_lossy().as_ref()
        }
    })
    .to_string();

    let assert = waypoint()
        .args(["hook", "pre-read"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    assert_eq!(hook["hookEventName"], "PreToolUse");
    // No map in project B, so no context
    assert!(
        hook.get("additionalContext").is_none(),
        "unscanned foreign project should have no context, got: {hook}"
    );
}

#[test]
fn hook_pre_read_nested_project_prefers_child() {
    // Parent repo is scanned, nested child repo is also scanned.
    // The child's map should win for files inside the child.
    let parent = setup_project();

    // Scan parent first so it has a .waypoint/map.md
    waypoint()
        .arg("scan")
        .current_dir(parent.path())
        .assert()
        .success();

    // Create a nested child repo (gitignored by parent so parent map won't include it)
    let child_dir = parent.path().join("nested");
    fs::create_dir_all(child_dir.join(".git")).unwrap();
    fs::create_dir_all(child_dir.join("src")).unwrap();
    fs::write(child_dir.join("src/lib.rs"), "pub fn nested() {}\n").unwrap();
    fs::write(parent.path().join(".gitignore"), "nested/\n").unwrap();

    // Scan the child so it has its own .waypoint/map.md
    waypoint()
        .arg("scan")
        .current_dir(&child_dir)
        .assert()
        .success();

    // Read a child file while cwd is the parent
    let payload = serde_json::json!({
        "cwd": parent.path().to_string_lossy(),
        "tool_input": {
            "file_path": child_dir.join("src/lib.rs").to_string_lossy().as_ref()
        }
    })
    .to_string();

    let assert = waypoint()
        .args(["hook", "pre-read"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    assert_eq!(hook["hookEventName"], "PreToolUse");
    let ctx = hook["additionalContext"].as_str().unwrap();
    assert!(
        ctx.contains("[waypoint] map:"),
        "expected map context from child, got: {ctx}"
    );
    assert!(
        ctx.contains("lib.rs"),
        "expected lib.rs from child map, got: {ctx}"
    );
}

// ── Hook Integration Tests ───────────────────────────────────────

#[test]
fn hook_session_start_outputs_invocation_prompts_and_creates_map() {
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
        .stdout(predicate::str::contains("waypoint trap log"));

    // Auto-scan creates map.md on first session
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

// ── Cross-Project CLI Tests ─────────────────────────────────────

/// Create a scanned project with .waypoint/ initialized.
fn setup_scanned_project() -> TempDir {
    let project = setup_project();
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();
    project
}

/// Build a cross-project hook payload: cwd is `project_a`, file is in `project_b`.
fn cross_project_payload(project_a: &TempDir, project_b: &TempDir, file_path: &str) -> String {
    serde_json::json!({
        "cwd": project_a.path().to_string_lossy(),
        "tool_input": {
            "file_path": project_b.path().join(file_path).to_string_lossy().as_ref()
        }
    })
    .to_string()
}

// ── AC-1: trap log --file resolves foreign project ──────────────

#[test]
fn cli_trap_log_foreign_file_writes_to_foreign_project() {
    let project_a = setup_scanned_project();
    let project_b = setup_scanned_project();

    let foreign_file = project_b.path().join("src/main.rs");

    waypoint()
        .args([
            "trap",
            "log",
            "--error",
            "foreign bug",
            "--file",
            foreign_file.to_str().unwrap(),
            "--cause",
            "foreign cause",
            "--fix",
            "foreign fix",
            "--tags",
            "cross-project",
        ])
        .current_dir(project_a.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Trap logged"));

    // Trap should be in project B, not project A
    let traps_b = fs::read_to_string(project_b.path().join(".waypoint/traps.json")).unwrap();
    assert!(
        traps_b.contains("foreign bug"),
        "trap should be in project B: {traps_b}"
    );

    let traps_a_path = project_a.path().join(".waypoint/traps.json");
    let traps_a = if traps_a_path.exists() {
        fs::read_to_string(&traps_a_path).unwrap()
    } else {
        String::new()
    };
    assert!(
        !traps_a.contains("foreign bug"),
        "trap should NOT be in project A: {traps_a}"
    );
}

// ── AC-10: trap log normalizes file path to project-relative ────

#[test]
fn cli_trap_log_foreign_file_stores_relative_path() {
    let project_a = setup_scanned_project();
    let project_b = setup_scanned_project();

    let foreign_file = project_b.path().join("src/main.rs");

    waypoint()
        .args([
            "trap",
            "log",
            "--error",
            "path test",
            "--file",
            foreign_file.to_str().unwrap(),
            "--cause",
            "testing",
            "--fix",
            "testing",
            "--tags",
            "test",
        ])
        .current_dir(project_a.path())
        .assert()
        .success();

    let traps_b = fs::read_to_string(project_b.path().join(".waypoint/traps.json")).unwrap();
    let traps: serde_json::Value = serde_json::from_str(&traps_b).unwrap();
    let file_field = traps[0]["file"].as_str().unwrap();
    assert_eq!(
        file_field, "src/main.rs",
        "file should be project-relative, got: {file_field}"
    );
}

// ── AC-11: trap log fallback to cwd when no .waypoint ───────────

#[test]
fn cli_trap_log_unknown_repo_falls_back_to_cwd() {
    let project_a = setup_scanned_project();
    let unknown = TempDir::new().unwrap();
    fs::create_dir(unknown.path().join(".git")).unwrap();
    // No .waypoint/ in unknown

    let foreign_file = unknown.path().join("foo.rs");
    fs::write(&foreign_file, "").unwrap();

    waypoint()
        .args([
            "trap",
            "log",
            "--error",
            "fallback bug",
            "--file",
            foreign_file.to_str().unwrap(),
            "--cause",
            "testing",
            "--fix",
            "testing",
            "--tags",
            "test",
        ])
        .current_dir(project_a.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Trap logged"));

    // Should fall back to project A
    let traps_a = fs::read_to_string(project_a.path().join(".waypoint/traps.json")).unwrap();
    assert!(
        traps_a.contains("fallback bug"),
        "trap should fall back to cwd project: {traps_a}"
    );
}

// ── AC-2: trap search -C targets foreign project ────────────────

#[test]
fn cli_trap_search_with_context_flag() {
    let project_a = setup_scanned_project();
    let project_b = setup_scanned_project();

    // Log a trap in project B
    waypoint()
        .args([
            "trap",
            "log",
            "--error",
            "foreign error",
            "--file",
            "src/main.rs",
            "--cause",
            "testing",
            "--fix",
            "testing",
            "--tags",
            "test",
        ])
        .current_dir(project_b.path())
        .assert()
        .success();

    // Search from project A with -C pointing to project B
    waypoint()
        .args([
            "trap",
            "search",
            "-C",
            project_b.path().to_str().unwrap(),
            "foreign error",
        ])
        .current_dir(project_a.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("foreign error"));
}

// ── AC-3: sketch -C targets foreign project ─────────────────────

#[test]
fn cli_sketch_with_context_flag() {
    let project_a = setup_scanned_project();
    let project_b = setup_scanned_project();

    // Sketch from project A targeting project B
    waypoint()
        .args(["sketch", "-C", project_b.path().to_str().unwrap(), "main"])
        .current_dir(project_a.path())
        .assert()
        .success();
    // Just verifying it doesn't error — symbol may or may not match
}

// ── AC-4: find -C targets foreign project ───────────────────────

#[test]
fn cli_find_with_context_flag() {
    let project_a = setup_scanned_project();
    let project_b = setup_scanned_project();

    // Find from project A targeting project B
    waypoint()
        .args(["find", "-C", project_b.path().to_str().unwrap(), "main"])
        .current_dir(project_a.path())
        .assert()
        .success();
}

// ── AC-12: -C with nonexistent path returns clear error ─────────

#[test]
fn cli_context_flag_nonexistent_path_errors() {
    let project = setup_scanned_project();

    waypoint()
        .args(["sketch", "-C", "/nonexistent/deeply/nested/path", "main"])
        .current_dir(project.path())
        .assert()
        .failure();
}

// ── AC-6: pre-read annotates foreign project ────────────────────

#[test]
fn hook_pre_read_annotates_foreign_project() {
    let project_a = setup_scanned_project();
    let project_b = setup_scanned_project();

    let payload = cross_project_payload(&project_a, &project_b, "src/main.rs");

    let assert = waypoint()
        .args(["hook", "pre-read"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    let ctx = hook["additionalContext"].as_str().unwrap();
    assert!(
        ctx.contains("[waypoint] foreign:"),
        "expected foreign annotation, got: {ctx}"
    );
    assert!(
        ctx.contains(&project_b.path().to_string_lossy().to_string()),
        "expected project B path in annotation, got: {ctx}"
    );
}

// ── AC-7: pre-write checks foreign project traps ────────────────

#[test]
fn hook_pre_write_checks_foreign_project_traps() {
    let project_a = setup_scanned_project();
    let project_b = setup_scanned_project();

    // Log a trap in project B for src/main.rs
    waypoint()
        .args([
            "trap",
            "log",
            "--error",
            "foreign trap error",
            "--file",
            "src/main.rs",
            "--cause",
            "testing",
            "--fix",
            "testing",
            "--tags",
            "test",
        ])
        .current_dir(project_b.path())
        .assert()
        .success();

    // Pre-write on a project B file while cwd is project A
    let payload = cross_project_payload(&project_a, &project_b, "src/main.rs");

    let assert = waypoint()
        .args(["hook", "pre-write"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    let ctx = hook["additionalContext"].as_str().unwrap();
    assert!(
        ctx.contains("foreign trap error"),
        "expected foreign trap warning, got: {ctx}"
    );
}

// ── AC-8: post-write updates foreign project map ────────────────

#[test]
fn hook_post_write_updates_foreign_project_map() {
    let project_a = setup_scanned_project();
    let project_b = setup_scanned_project();

    // Modify file in project B
    fs::write(
        project_b.path().join("src/main.rs"),
        "fn main() {}\n\npub fn cross_project_helper() {}\n",
    )
    .unwrap();

    let payload = cross_project_payload(&project_a, &project_b, "src/main.rs");

    let assert = waypoint()
        .args(["hook", "post-write"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    let ctx = hook["additionalContext"].as_str().unwrap();
    assert!(
        ctx.contains("[waypoint] map updated: src/main.rs"),
        "got: {ctx}"
    );

    // Verify project B's map was updated
    let map_b = fs::read_to_string(project_b.path().join(".waypoint/map.md")).unwrap();
    assert!(
        map_b.contains("cross_project_helper"),
        "project B map should reflect update: {map_b}"
    );
}

// ── AC-9: post-failure includes -C for foreign files ────────────

#[test]
fn hook_post_failure_includes_context_flag_for_foreign() {
    let project_a = setup_scanned_project();
    let project_b = setup_scanned_project();

    let payload = cross_project_payload(&project_a, &project_b, "src/main.rs");

    let assert = waypoint()
        .args(["hook", "post-failure"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    let ctx = hook["additionalContext"].as_str().unwrap();
    assert!(
        ctx.contains("-C"),
        "expected -C flag in suggestion, got: {ctx}"
    );
    assert!(
        ctx.contains(&project_b.path().to_string_lossy().to_string()),
        "expected project B path in suggestion, got: {ctx}"
    );
}

// ── AC-13: no auto-initialization of foreign projects ───────────

#[test]
fn cli_context_flag_no_auto_init() {
    let project_a = setup_scanned_project();
    let project_b = TempDir::new().unwrap();
    fs::create_dir(project_b.path().join(".git")).unwrap();
    // No .waypoint/ in project B

    // -C to project B should fail, not auto-create .waypoint/
    waypoint()
        .args(["sketch", "-C", project_b.path().to_str().unwrap(), "main"])
        .current_dir(project_a.path())
        .assert()
        .failure();

    assert!(
        !project_b.path().join(".waypoint").exists(),
        ".waypoint/ should NOT be auto-created in foreign project"
    );
}

// ── Scan --all Tests ────────────────────────────────────────────

/// Helper: create a parent dir with N child git repos, each containing a source file.
fn setup_multi_project(names: &[&str]) -> TempDir {
    let parent = TempDir::new().unwrap();
    for name in names {
        let repo = parent.path().join(name);
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::create_dir(repo.join(".git")).unwrap();
        fs::write(
            repo.join("src/main.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
    }
    parent
}

#[test]
fn cli_scan_all_discovers_and_scans_children() {
    let parent = setup_multi_project(&["alpha", "beta"]);

    waypoint()
        .args(["scan", "--all", parent.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("alpha"))
        .stderr(predicate::str::contains("beta"))
        .stderr(predicate::str::contains("2 repos found, 2 scanned"));

    // Both repos should have .waypoint/ with map.md
    assert!(parent.path().join("alpha/.waypoint/map.md").exists());
    assert!(parent.path().join("beta/.waypoint/map.md").exists());
}

#[test]
fn cli_scan_all_check_conflict() {
    waypoint()
        .args(["scan", "--all", "--check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn cli_scan_all_empty_dir_exits_nonzero() {
    let empty = TempDir::new().unwrap();

    waypoint()
        .args(["scan", "--all", empty.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No git repos found"));
}

#[test]
fn cli_scan_all_initializes_new_repos() {
    let parent = setup_multi_project(&["fresh"]);
    assert!(!parent.path().join("fresh/.waypoint").exists());

    waypoint()
        .args(["scan", "--all", parent.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("initialized"));

    assert!(parent.path().join("fresh/.waypoint/map.md").exists());
    // traps.json is lazy-created on first log
}

#[test]
fn cli_trap_prune_requires_older_than() {
    let project = setup_project();

    waypoint()
        .args(["trap", "prune"])
        .current_dir(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--older-than"));
}
