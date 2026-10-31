//! Running an AArch64 Linux executable from a test, wherever there is a way to:
//! directly on an AArch64 Linux machine, under qemu-user (`qemu-aarch64` or
//! `qemu-aarch64-static` on the PATH), or on another machine over ssh in a
//! `linux/arm64` Docker container (set AIPL_ARM64_SSH to the host, as
//! `pennylane`: a Mac with Docker Desktop), or, on a Mac, in a `linux/arm64`
//! container started here. With none of these there is nothing to run it on
//! and `run` gives None, and the test says it did not.

use std::io::Write;
use std::process::{Command, Output, Stdio};

/// A `docker` that runs `linux/arm64` containers on this machine (Docker
/// Desktop on an Apple Silicon Mac), with the places Homebrew keeps it on the PATH.
fn local_docker() -> bool {
    static FOUND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FOUND.get_or_init(|| {
        cfg!(all(target_os = "macos", target_arch = "aarch64"))
            && Command::new("sh")
                .args(["-c", "export PATH=/opt/homebrew/bin:/usr/local/bin:$PATH; docker run --rm --platform linux/arm64 alpine true"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
    })
}

/// Where AArch64 executables can run, if anywhere.
pub fn available() -> Option<String> {
    if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        return Some("this machine".into());
    }
    for q in ["qemu-aarch64", "qemu-aarch64-static"] {
        if Command::new(q).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success()) {
            return Some(q.into());
        }
    }
    if local_docker() {
        return Some("docker on this machine".into());
    }
    std::env::var("AIPL_ARM64_SSH").ok().filter(|h| !h.is_empty()).map(|h| format!("docker over ssh on {h}"))
}

/// Runs the executable `exe` with `args` and `input` on standard input.
pub fn run(exe: &[u8], args: &[&str], input: &[u8]) -> Option<Output> {
    run_env(exe, args, input, &[])
}

/// `run`, with the environment variables `env` set for the program.
pub fn run_env(exe: &[u8], args: &[&str], input: &[u8], env: &[(&str, &str)]) -> Option<Output> {
    use std::os::unix::fs::PermissionsExt;
    let how = available()?;
    let dir = std::env::temp_dir().join(format!("aipl_arm64_{}_{}", std::process::id(), exe.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog");
    std::fs::write(&path, exe).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cmd = if how == "this machine" {
        let mut c = Command::new(&path);
        c.args(args).envs(env.iter().copied()).current_dir(&dir);
        c
    } else if how == "docker on this machine" {
        let mut c = Command::new("sh");
        let flags: String = env.iter().map(|(k, _)| format!("-e {k} ")).collect();
        let script = format!("export PATH=/opt/homebrew/bin:/usr/local/bin:$PATH; d=$1; shift; exec docker run --rm -i --platform linux/arm64 {flags}-v \"$d\":/w -w /w alpine /w/prog \"$@\"");
        c.args(["-c", &script, "sh"]);
        c.arg(&dir).args(args).envs(env.iter().copied());
        c
    } else if how.starts_with("qemu") {
        let mut c = Command::new(&how);
        c.arg(&path).args(args).envs(env.iter().copied()).current_dir(&dir);
        c
    } else {
        // the file goes over ssh on standard input; then a container runs it
        let host = std::env::var("AIPL_ARM64_SSH").unwrap();
        let quoted: Vec<String> = args.iter().map(|a| format!("'{}'", a.replace('\'', "'\\''"))).collect();
        let flags: String = env.iter().map(|(k, v)| format!("-e '{k}={v}' ")).collect();
        // the program's standard input travels in the script, as hex (the executable takes ssh's own)
        let hex: String = input.iter().map(|b| format!("{b:02x}")).collect();
        let (feed, interactive, redirect) = if input.is_empty() { (String::new(), "", "") } else { (format!("printf '%s' {hex} | xxd -r -p > $d/in; "), "-i ", " < $d/in") };
        let script = format!(
            "export PATH=/opt/homebrew/bin:/usr/local/bin:$PATH; d=$(mktemp -d ~/aipl-arm64-XXXXXX); cat > $d/prog; chmod +x $d/prog; {feed}\
             docker run --rm {interactive}--platform linux/arm64 {flags}-v $d:/w -w /w alpine /w/prog {}{redirect}; s=$?; rm -rf $d; exit $s",
            quoted.join(" ")
        );
        let mut c = Command::new("ssh");
        c.args(["-o", "BatchMode=yes", &host, &script]);
        let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(exe).unwrap();
        let out = child.wait_with_output().unwrap();
        std::fs::remove_dir_all(&dir).ok();
        return Some(out);
    };
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).ok();
    let out = child.wait_with_output().unwrap();
    std::fs::remove_dir_all(&dir).ok();
    Some(out)
}

/// Starts the executable `exe` with `args` as a child process whose network is this machine's, to be
/// talked to from the test (a server): where AArch64 code runs on this machine, directly or under
/// qemu-user. None elsewhere, as a container on another machine is not on this one's network.
#[allow(dead_code)] // test_native_arm64_programs.rs uses it; test_native_arm64.rs shares this module
pub fn spawn(exe: &[u8], args: &[&str]) -> Option<std::process::Child> {
    use std::os::unix::fs::PermissionsExt;
    let how = available()?;
    if !(how == "this machine" || how.starts_with("qemu")) {
        return None;
    }
    let dir = std::env::temp_dir().join(format!("aipl_arm64_srv_{}_{}", std::process::id(), exe.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog");
    std::fs::write(&path, exe).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut c = if how == "this machine" { Command::new(&path) } else { Command::new(&how) };
    if how != "this machine" {
        c.arg(&path);
    }
    c.args(args).current_dir(&dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    Some(c.spawn().unwrap())
}
