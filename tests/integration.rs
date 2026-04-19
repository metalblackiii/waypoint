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

// ── AC-1 / AC-2: removed commands exit non-zero ─────────────────

#[test]
fn cli_trap_subcommand_is_removed() {
    let project = setup_project();
    waypoint()
        .args(["trap", "search", "foo"])
        .current_dir(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn hook_post_write_subcommand_is_removed() {
    let project = setup_project();
    waypoint()
        .args(["hook", "post-write"])
        .current_dir(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn hook_pre_write_subcommand_is_removed() {
    let project = setup_project();
    waypoint()
        .args(["hook", "pre-write"])
        .current_dir(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn hook_post_failure_subcommand_is_removed() {
    let project = setup_project();
    waypoint()
        .args(["hook", "post-failure"])
        .current_dir(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
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
fn hook_session_start_auto_scans_and_creates_map() {
    let project = setup_project();
    let payload = serde_json::json!({
        "cwd": project.path().to_string_lossy()
    })
    .to_string();

    waypoint()
        .args(["hook", "session-start"])
        .write_stdin(payload)
        .assert()
        .success();

    // Auto-scan creates map.md on first session
    assert!(
        project.path().join(".waypoint/map.md").exists(),
        "session-start should auto-create map.md"
    );
}

#[test]
fn session_start_drift_threshold_boundary() {
    // Verify the 3% file-count drift threshold:
    //   34-file project → 1 file added = 2.94% drift (below) → no rescan
    //                   → 2nd file added = 5.88% drift (above) → rescan
    let project = setup_project(); // creates src/main.rs (1 file)

    // Create 33 more files so total = 34
    for i in 0..33_u32 {
        fs::write(
            project.path().join(format!("src/mod_{i}.rs")),
            format!("// module {i}\n"),
        )
        .unwrap();
    }

    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    let map_after_scan = fs::read_to_string(project.path().join(".waypoint/map.md")).unwrap();
    let payload = || serde_json::json!({ "cwd": project.path().to_string_lossy() }).to_string();

    // Add 1 file: drift = 1/34 ≈ 2.94% — below threshold, should NOT rescan
    fs::write(project.path().join("src/below_threshold.rs"), "// below\n").unwrap();
    waypoint()
        .args(["hook", "session-start"])
        .write_stdin(payload())
        .assert()
        .success();
    let map = fs::read_to_string(project.path().join(".waypoint/map.md")).unwrap();
    assert!(
        !map.contains("below_threshold.rs"),
        "session-start should not rescan at 2.94% drift; map:\n{map}"
    );
    assert_eq!(
        map, map_after_scan,
        "map.md should be unchanged below threshold"
    );

    // Add a 2nd new file: drift = 2/34 ≈ 5.88% — above threshold, SHOULD rescan
    fs::write(project.path().join("src/above_threshold.rs"), "// above\n").unwrap();
    waypoint()
        .args(["hook", "session-start"])
        .write_stdin(payload())
        .assert()
        .success();
    let map = fs::read_to_string(project.path().join(".waypoint/map.md")).unwrap();
    assert!(
        map.contains("above_threshold.rs"),
        "session-start should rescan at 5.88% drift; map:\n{map}"
    );
}

#[test]
fn hook_session_start_rebuilds_import_index() {
    // Regression: session-start rescan must rebuild imports so `waypoint callers` is fresh.
    let project = setup_project();

    // Add a lib file so main.rs can import from it
    fs::write(project.path().join("src/lib.rs"), "pub fn greet() {}\n").unwrap();
    fs::write(
        project.path().join("src/main.rs"),
        "use crate::greet;\nfn main() { greet(); }\n",
    )
    .unwrap();

    let payload = serde_json::json!({
        "cwd": project.path().to_string_lossy()
    })
    .to_string();

    waypoint()
        .args(["hook", "session-start"])
        .write_stdin(payload)
        .assert()
        .success();

    // map.md and map_index.db should exist
    assert!(project.path().join(".waypoint/map.md").exists());
    assert!(project.path().join(".waypoint/map_index.db").exists());

    // `waypoint callers greet` should return results (import index was built)
    let output = waypoint()
        .args(["callers", "greet"])
        .current_dir(project.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("src/main.rs"),
        "session-start should rebuild import index so callers finds src/main.rs; got: {stdout}"
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
}

// ── Ranked Find Tests ──────────────────────────────────────────

#[test]
fn cli_find_ranks_by_fan_in() {
    let project = setup_project();
    // Create symbols with different import fan-in
    fs::write(
        project.path().join("src/core.rs"),
        "pub fn process_core() {}\npub fn process_extra() {}\n",
    )
    .unwrap();
    fs::write(
        project.path().join("src/a.rs"),
        "use crate::process_core;\nfn a() { process_core(); }\n",
    )
    .unwrap();
    fs::write(
        project.path().join("src/b.rs"),
        "use crate::process_core;\nfn b() { process_core(); }\n",
    )
    .unwrap();
    fs::write(
        project.path().join("src/c.rs"),
        "use crate::process_core;\nfn c() { process_core(); }\n",
    )
    .unwrap();

    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    let output = waypoint()
        .args(["find", "process"])
        .current_dir(project.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // process_core has 3 importers, should appear before process_extra (0 importers)
    assert!(
        lines.len() >= 2,
        "expected at least 2 results, got: {stdout}"
    );
    assert!(
        lines[0].contains("process_core"),
        "process_core should rank first; got: {stdout}"
    );
}

#[test]
fn cli_find_output_format_unchanged() {
    let project = setup_project();
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    // Output should contain kind, name, file, line — same format as before ranking
    waypoint()
        .args(["find", "main"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("fn"))
        .stdout(predicate::str::contains("main"))
        .stdout(predicate::str::contains("src/main.rs"));
}

// ── Architecture Context Injection Tests ───────────────────────

/// Create a project with N numbered source files for arch gating tests.
fn setup_project_with_files(n: usize) -> TempDir {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join(".git")).unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .unwrap();
    for i in 1..n {
        fs::write(
            tmp.path().join(format!("src/mod_{i}.rs")),
            format!("pub fn func_{i}() {{}}\n"),
        )
        .unwrap();
    }
    tmp
}

#[test]
fn hook_session_start_emits_arch_context_large_project() {
    let project = setup_project_with_files(25);
    let payload = serde_json::json!({
        "cwd": project.path().to_string_lossy()
    })
    .to_string();

    let assert = waypoint()
        .args(["hook", "session-start"])
        .write_stdin(payload)
        .assert()
        .success();

    let hook = parse_hook_output(&assert);
    let context = hook["additionalContext"].as_str().unwrap_or("");
    assert!(
        context.contains("[waypoint] arch:"),
        "large project should emit arch context; got: {context}"
    );
    assert!(
        context.contains("Rust"),
        "arch context should show Rust as primary language; got: {context}"
    );
}

#[test]
fn hook_session_start_suppresses_arch_context_small_project() {
    let project = setup_project(); // only 1 file — below threshold
    let payload = serde_json::json!({
        "cwd": project.path().to_string_lossy()
    })
    .to_string();

    let assert = waypoint()
        .args(["hook", "session-start"])
        .write_stdin(payload)
        .assert()
        .success();

    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    // Small project: either no JSON output at all, or no arch context in it
    if !stdout.trim().is_empty() {
        let hook = parse_hook_output(&assert);
        let context = hook["additionalContext"].as_str().unwrap_or("");
        assert!(
            !context.contains("[waypoint] arch:"),
            "small project should NOT emit arch context; got: {context}"
        );
    }
}

#[test]
fn scan_persists_arch_summary() {
    let project = setup_project_with_files(25);
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    // The arch_summary table should be populated in the index db
    let db_path = project.path().join(".waypoint/map_index.db");
    assert!(db_path.exists(), "index db should exist after scan");

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT file_count FROM arch_summary WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        count >= 25,
        "arch_summary should record file count >= 25; got {count}"
    );
}

// ── Impact Analysis Tests ──────────────────────────────────────

/// Create a git-initialized project for impact tests (real git repo, not just .git marker).
fn setup_git_project() -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn greet() {}\npub fn farewell() {}\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("src/main.rs"),
        "use crate::greet;\nfn main() { greet(); }\n",
    )
    .unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    // Scan to build the index
    waypoint()
        .arg("scan")
        .current_dir(tmp.path())
        .assert()
        .success();

    tmp
}

#[test]
fn cli_impact_uncommitted_changes() {
    let project = setup_git_project();

    // Make an uncommitted change to an exported symbol
    fs::write(
        project.path().join("src/lib.rs"),
        "pub fn greet() { println!(\"hi\"); }\npub fn farewell() {}\n",
    )
    .unwrap();

    waypoint()
        .arg("impact")
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Changed:"))
        .stdout(predicate::str::contains("greet"))
        .stdout(predicate::str::contains("Risk:"));
}

#[test]
fn cli_impact_no_changes() {
    let project = setup_git_project();

    // No changes — diff against HEAD should report clean
    waypoint()
        .args(["impact", "--base", "HEAD"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No symbol changes detected."));
}

#[test]
fn cli_impact_non_git_directory() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".waypoint")).unwrap();
    // Create a dummy map so require_waypoint_dir succeeds
    fs::write(tmp.path().join(".waypoint/map.md"), "# Map\n").unwrap();

    waypoint()
        .arg("impact")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not a git repository"));
}

#[test]
fn cli_impact_private_symbols_shown() {
    let project = setup_git_project();

    // Add a private function
    fs::write(
        project.path().join("src/lib.rs"),
        "pub fn greet() {}\npub fn farewell() {}\nfn secret_helper() {}\n",
    )
    .unwrap();

    // Rescan so the new symbol is indexed
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    let output = waypoint()
        .arg("impact")
        .current_dir(project.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Private symbols appear with (private) marker and Risk: LOW
    assert!(
        stdout.contains("secret_helper"),
        "private symbols should appear in impact output; got: {stdout}"
    );
}

#[test]
fn cli_impact_with_base_flag() {
    let project = setup_git_project();

    // Create a branch with changes
    std::process::Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(project.path())
        .output()
        .unwrap();
    fs::write(
        project.path().join("src/lib.rs"),
        "pub fn greet() { println!(\"hello\"); }\npub fn farewell() {}\n",
    )
    .unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(project.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "feature change"])
        .current_dir(project.path())
        .output()
        .unwrap();

    // Rescan to pick up changes
    waypoint()
        .arg("scan")
        .current_dir(project.path())
        .assert()
        .success();

    waypoint()
        .args(["impact", "--base", "main"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Changed:"))
        .stdout(predicate::str::contains("greet"));
}

// ── Version Test ───────────────────────────────────────────────

#[test]
fn cli_version_reports_0_8_1() {
    waypoint()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("0.8.1"));
}
