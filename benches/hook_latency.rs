#![allow(clippy::unwrap_used)]

use std::path::Path;

use divan::Bencher;
use tempfile::TempDir;
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

// --- update_entry (read + modify + write) ---

#[divan::bench(args = [1000, 3000, 5000])]
fn update_entry(bencher: Bencher, n: usize) {
    let (tmp, _entries) = prepared_dir(n);
    let updated = MapEntry {
        path: format!("src/module_050/file_{:05}.rs", n / 2),
        description: "pub fn updated_handler()".into(),
        token_estimate: 999,
    };
    bencher.bench(|| map::update_entry(tmp.path(), updated.clone()).unwrap());
}

// --- lookup ---

#[divan::bench(args = [1000, 3000, 5000])]
fn lookup(bencher: Bencher, n: usize) {
    let entries = synthetic_entries(n);
    let target = format!("src/module_050/file_{:05}.rs", n / 2);
    bencher.bench(|| map::lookup(&entries, &target));
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
