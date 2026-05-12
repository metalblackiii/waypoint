#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

use divan::Bencher;
use tempfile::TempDir;
use waypoint::hook::session_start::has_mtime_drift;
use waypoint::map::{self, MapEntry};

fn main() {
    divan::main();
}

/// Generate N synthetic map entries spread across directories.
fn synthetic_entries(n: usize) -> Vec<MapEntry> {
    (0..n)
        .map(|i| {
            let dir = format!("src/module_{:03}", i % 100);
            MapEntry {
                path: format!("{dir}/file_{i:05}.rs"),
                description: format!("pub fn handler_{i}(), pub struct Model{i}"),
                token_estimate: 200 + (i % 500),
                ..Default::default()
            }
        })
        .collect()
}

/// Write synthetic entries to a tempdir, return it for subsequent benchmarks.
fn prepared_dir(n: usize) -> (TempDir, Vec<MapEntry>) {
    let tmp = TempDir::new().unwrap();
    let entries = synthetic_entries(n);
    map::write_map(tmp.path(), &entries).unwrap();
    (tmp, entries)
}

// --- parse (read_map) ---

#[divan::bench(args = [1000, 3000, 5000])]
fn read_map(bencher: Bencher, n: usize) {
    let (tmp, _entries) = prepared_dir(n);
    bencher.bench(|| map::read_map(tmp.path()).unwrap());
}

// --- write_map ---

#[divan::bench(args = [1000, 3000, 5000])]
fn write_map(bencher: Bencher, n: usize) {
    let tmp = TempDir::new().unwrap();
    let entries = synthetic_entries(n);
    bencher.bench(|| map::write_map(tmp.path(), &entries).unwrap());
}

// --- lookup (linear scan on Vec) ---

#[divan::bench(args = [1000, 3000, 5000, 9000])]
fn lookup(bencher: Bencher, n: usize) {
    let entries = synthetic_entries(n);
    let target = format!("src/module_050/file_{:05}.rs", n / 2);
    bencher.bench(|| map::lookup(&entries, &target));
}

// --- index_lookup (SQLite O(1)) ---

#[divan::bench(args = [1000, 3000, 5000, 9000])]
fn index_lookup(bencher: Bencher, n: usize) {
    let (tmp, _entries) = prepared_dir(n);
    let target = format!("src/module_050/file_{:05}.rs", n / 2);
    bencher.bench(|| map::index::lookup(tmp.path(), &target).unwrap());
}

// --- pre_read full path: read_map + lookup (what pre_read did before) ---

#[divan::bench(args = [1000, 3000, 5000, 9000])]
fn read_map_then_lookup(bencher: Bencher, n: usize) {
    let (tmp, _entries) = prepared_dir(n);
    let target = format!("src/module_050/file_{:05}.rs", n / 2);
    bencher.bench(|| {
        let entries = map::read_map(tmp.path()).unwrap();
        map::lookup(&entries, &target).cloned()
    });
}

// --- extract_description (tree-sitter) ---

const RUST_SOURCE: &str = r#"
use std::collections::HashMap;

pub struct Config {
    pub name: String,
    pub values: HashMap<String, i64>,
}

pub fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    todo!()
}

impl Config {
    pub fn get(&self, key: &str) -> Option<i64> {
        self.values.get(key).copied()
    }
}
"#;

const TS_SOURCE: &str = r#"
import { Request, Response } from 'express';

export interface UserProfile {
    id: string;
    name: string;
    email: string;
}

export async function getUser(req: Request, res: Response): Promise<void> {
    const userId = req.params.id;
    const profile = await fetchProfile(userId);
    res.json(profile);
}

export class UserService {
    constructor(private db: Database) {}

    async findById(id: string): Promise<UserProfile | null> {
        return this.db.query('SELECT * FROM users WHERE id = ?', [id]);
    }
}

export default UserService;
"#;

const PY_SOURCE: &str = r#"
from dataclasses import dataclass
from typing import Optional

@dataclass
class Patient:
    id: str
    name: str
    email: str

class PatientService:
    def __init__(self, db):
        self.db = db

    def find_by_id(self, patient_id: str) -> Optional[Patient]:
        return self.db.query(patient_id)

def create_patient(name: str, email: str) -> Patient:
    return Patient(id="generated", name=name, email=email)
"#;

#[divan::bench]
fn extract_rust(bencher: Bencher) {
    let path = Path::new("src/config.rs");
    bencher.bench(|| map::extract::extract_description(path, RUST_SOURCE));
}

#[divan::bench]
fn extract_typescript(bencher: Bencher) {
    let path = Path::new("src/user.ts");
    bencher.bench(|| map::extract::extract_description(path, TS_SOURCE));
}

#[divan::bench]
fn extract_python(bencher: Bencher) {
    let path = Path::new("src/patient.py");
    bencher.bench(|| map::extract::extract_description(path, PY_SOURCE));
}

// --- estimate_tokens ---

#[divan::bench(args = [1000, 10000, 50000])]
fn estimate_tokens(bencher: Bencher, size: usize) {
    let content = "x".repeat(size);
    let path = Path::new("src/big.rs");
    bencher.bench(|| map::estimate_tokens(&content, path));
}

// --- has_mtime_drift ---

/// Build a temp project with `n` non-empty `.rs` files, return it alongside
/// a stored-mtimes map that matches every file exactly (steady-state baseline).
/// Mtime-capture logic mirrors `mtime_ms` in `session_start` unit tests.
fn make_project_with_stored(n: usize) -> (TempDir, HashMap<String, i64>) {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    let mut stored = HashMap::new();
    for i in 0..n {
        let rel = format!("src/file_{i:05}.rs");
        let path = tmp.path().join(&rel);
        std::fs::write(&path, format!("pub fn f_{i}() {{}}")).unwrap();
        #[allow(clippy::cast_possible_truncation)]
        // Unix millis (~1.7 trillion) fits comfortably in i64 (~9.2 quintillion)
        let mtime = std::fs::metadata(&path)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        stored.insert(rel, mtime);
    }
    (tmp, stored)
}

/// Steady state: every on-disk file is in stored with a matching mtime.
/// Hot path — runs on every session-start when nothing has changed.
/// Measures pure walk + stat cost; no content reads.
#[divan::bench(args = [1000, 3000, 5000, 9000])]
fn mtime_drift_steady_state(bencher: Bencher, n: usize) {
    let (tmp, stored) = make_project_with_stored(n);
    bencher.bench(|| has_mtime_drift(tmp.path(), &stored));
}

/// One new non-empty file: walk until the new file is found, do one content
/// read, return true. Named `z_new.rs` (alphabetically late among the
/// `file_NNNNN.rs` set) to encourage a longer walk before early-return;
/// actual order depends on `ignore::WalkBuilder` traversal, not guaranteed.
#[divan::bench(args = [1000, 3000, 5000, 9000])]
fn mtime_drift_one_new_file(bencher: Bencher, n: usize) {
    let (tmp, stored) = make_project_with_stored(n);
    std::fs::write(tmp.path().join("src/z_new.rs"), "pub fn new() {}").unwrap();
    bencher.bench(|| has_mtime_drift(tmp.path(), &stored));
}

/// Whitespace-only new files: the walk completes in full because blank files
/// are read and skipped rather than triggering an early return. Measures the
/// per-blank-file content-read overhead added by the fix.
/// Fixed stored count (1000); varying blank counts isolate the per-file cost.
#[divan::bench(args = [10, 50, 100])]
fn mtime_drift_whitespace_only_new_files(bencher: Bencher, n_blank: usize) {
    let (tmp, stored) = make_project_with_stored(1000);
    for i in 0..n_blank {
        std::fs::write(tmp.path().join(format!("src/blank_{i:03}.js")), "\n").unwrap();
    }
    bencher.bench(|| has_mtime_drift(tmp.path(), &stored));
}
