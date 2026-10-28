//! The `aipl_sys_v1` host interface (docs/design/SYS_PLAN.md) for the two
//! hosts written in Rust: the VM and aipl-run. Native executables do the
//! same in AIPL, as Linux system calls (aipl_src/native/wasi.aipl).
//!
//! `chmod` sets a file's permission bits, the nine of 0o777 and nothing
//! else (no setuid, setgid, or sticky bit). It is confined as the other
//! file operations are: a relative path may not climb out of the working
//! directory or reach out of it through a symbolic link, and under
//! `--sandbox` there is no way to name an absolute path at all.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path};

/// WASI's EBADF, EINVAL, and EPERM, negated as the file operations' results are.
const EBADF: i32 = -8;
const EINVAL: i32 = -28;
const EPERM: i32 = -63;

/// The import: `chmod(dirfd, path, len, mode)`, the directory chosen as the
/// file operations choose it: 3 is the working directory, which a path may
/// not leave (as under wasmtime's preopens, `--sandbox` or not), 4 is `/`
/// (a program built with `--sandbox` is not given it: the descriptor is
/// not open, and the host is not asked), the path then without its first
/// slash. 0, or minus a WASI errno.
pub fn chmod_at(dirfd: i32, path: &[u8], mode: i32) -> i32 {
    match dirfd {
        3 => chmod(path, mode, true),
        4 => chmod(&[b"/", path].concat(), mode, false),
        _ => EBADF,
    }
}

/// `(fs.chmod path len mode)` as the VM has it, paths as they are: 0, or
/// minus a WASI errno. `path` is the bytes the program gave; `confined` is
/// whether it may name nothing outside the working directory (the VM's
/// paths are ordinary: it passes false).
pub fn chmod(path: &[u8], mode: i32, confined: bool) -> i32 {
    if mode & !0o777 != 0 {
        return EINVAL;
    }
    // a path is bytes, as the kernel takes it (native executables do not
    // ask that it be UTF-8 either)
    let text = Path::new(OsStr::from_bytes(path));
    if confined && !stays_inside(text) {
        return EPERM;
    }
    match std::fs::set_permissions(text, std::fs::Permissions::from_mode(mode as u32)) {
        Ok(()) => 0,
        Err(e) => crate::vm::wasi_errno(&e) as i32,
    }
}

/// Whether `text` names something inside the working directory: relative,
/// never above where it starts, and (where it exists) not through a link
/// to somewhere else.
fn stays_inside(text: &Path) -> bool {
    let mut depth = 0i32;
    for part in text.components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::ParentDir => depth -= 1,
            Component::CurDir => {}
            _ => return false,
        }
        if depth < 0 {
            return false;
        }
    }
    let Ok(here) = std::env::current_dir().and_then(|d| d.canonicalize()) else { return false };
    match here.join(text).canonicalize() {
        Ok(real) => real.starts_with(&here),
        // a path that does not exist is the file operation's to report
        Err(_) => true,
    }
}
