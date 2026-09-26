use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

fn walk_max_mtime(dir: &Path, best: &mut Option<SystemTime>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            walk_max_mtime(&path, best);
            continue;
        }
        let t = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if best.map_or(true, |b| t > b) {
            *best = Some(t);
        }
    }
}

fn newest_source_time(frontend: &Path) -> SystemTime {
    let mut best: Option<SystemTime> = None;
    for name in ["index.html", "package.json", "package-lock.json"] {
        let p = frontend.join(name);
        if let Ok(meta) = p.metadata() {
            let t = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if best.map_or(true, |b| t > b) {
                best = Some(t);
            }
        }
    }
    for name in ["src", ".vite"] {
        walk_max_mtime(&frontend.join(name), &mut best);
    }
    best.unwrap_or(SystemTime::UNIX_EPOCH)
}

fn run(cmd: &mut Command, what: &str) -> bool {
    match cmd.status() {
        Ok(s) if s.success() => true,
        Ok(s) => {
            println!("cargo:warning={what} failed with exit code {}", s.code().unwrap_or(-1));
            false
        }
        Err(e) => {
            println!("cargo:warning=could not run {what}: {e}");
            false
        }
    }
}

fn main() {
    tauri_build::build();

    let frontend = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let dist = frontend.join("dist");
    let dist_index = dist.join("index.html");

    println!("cargo:rerun-if-changed=../index.html");
    println!("cargo:rerun-if-changed=../package.json");
    println!("cargo:rerun-if-changed=../package-lock.json");
    println!("cargo:rerun-if-changed=../src");
    println!("cargo:rerun-if-changed=../vite.config.ts");

    let stale = dist_index
        .metadata()
        .ok()
        .map(|m| m.modified().unwrap_or(SystemTime::UNIX_EPOCH) <= newest_source_time(&frontend))
        .unwrap_or(true);

    if !frontend.join("node_modules").is_dir() {
        run(
            Command::new("npm").args(["install", "--no-audit", "--no-fund"]).current_dir(&frontend),
            "npm install",
        );
    }

    if stale {
        if !run(Command::new("npm").args(["run", "build"]).current_dir(&frontend), "npm run build") {
            println!("cargo:warning=frontend build failed; using any existing dist/ output");
        }
    }
}