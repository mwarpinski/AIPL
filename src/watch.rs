//! `aipl watch FILE [--test] [-- ARGS]` (docs/design/FEEDBACK_TOOLS_PLAN.md
//! F5): runs FILE (`aipl eval`), or its tests (`aipl test`), and again each
//! time it or a file it imports changes, so a scratch file in an editor
//! serves as a session. It looks four times a second (no file-notification
//! library); the files it watches are the ones the resolver read, found
//! again after each run (or FILE alone while it does not resolve).

use crate::resolver::Resolver;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// The files `file`'s program is read from.
fn watched(file: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match Resolver::resolve(file) {
        Ok(m) => m.file_names.keys().cloned().collect(),
        Err(_) => vec![],
    };
    if !files.iter().any(|f| f == file) {
        files.push(file.to_path_buf());
    }
    files.sort();
    files
}

fn stamps(files: &[PathBuf]) -> Vec<Option<SystemTime>> {
    files.iter().map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok()).collect()
}

/// Watches until interrupted, or until `max_runs` runs (for tests). `me` is
/// this executable.
pub fn run(file: &Path, tests: bool, args: &[String], me: &Path, max_runs: Option<usize>) -> Result<(), String> {
    use std::io::IsTerminal;
    let mut runs = 0;
    loop {
        let files = watched(file);
        let seen = stamps(&files);
        if std::io::stdout().is_terminal() {
            print!("\x1b[2J\x1b[H");
        }
        println!("aipl watch {}: {} file{}, Ctrl-C to stop", file.display(), files.len(), if files.len() == 1 { "" } else { "s" });
        let start = Instant::now();
        let mut cmd = std::process::Command::new(me);
        if tests {
            cmd.arg("test").arg(file);
        } else {
            cmd.arg("eval").arg(file).arg("--").args(args);
        }
        let status = cmd.status().map_err(|e| e.to_string())?;
        println!("[{} in {:.2} s, exit {}]", if tests { "tested" } else { "ran" }, start.elapsed().as_secs_f64(), status.code().unwrap_or(-1));
        runs += 1;
        if max_runs.is_some_and(|m| runs >= m) {
            return Ok(());
        }
        while stamps(&files) == seen {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}
