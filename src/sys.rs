//! The `aipl_sys_v1` host interface (docs/design/SYS_PLAN.md) for the two
//! hosts written in Rust: the VM and aipl-run. Native executables do the
//! same in AIPL, as Linux system calls (aipl_src/native/wasi.aipl).
//!
//! `chmod` sets a file's permission bits, the nine of 0o777 and nothing
//! else (no setuid, setgid, or sticky bit). It is confined as the other
//! file operations are: a relative path may not climb out of the working
//! directory or reach out of it through a symbolic link, and under
//! `--sandbox` there is no way to name an absolute path at all.

use std::ffi::OsString;
use std::path::{Component, Path};

/// `chmod` for the hosts that run the toolchain's own modules and the tools
/// under wasmtime (src/selfhost.rs, src/native.rs, src/tools.rs): the
/// compiler imports std/fs, whose `chmod` names the import, so a module must
/// find it to start. The program has the working directory and `/`, as
/// aipl-run gives them; a path outside its memory is -21, as there.
pub fn add_chmod(linker: &mut wasmtime::Linker<wasmtime_wasi::p1::WasiP1Ctx>) -> Result<(), String> {
    linker
        .func_wrap("aipl_sys_v1", "chmod", |mut c: wasmtime::Caller<'_, wasmtime_wasi::p1::WasiP1Ctx>, dirfd: i32, path: i32, len: i32, mode: i32| -> i32 {
            let (at, n) = (path as u32 as usize, len as u32 as usize);
            let bytes = c.get_export("memory").and_then(|e| e.into_memory()).and_then(|m| m.data(&c).get(at..at.checked_add(n)?).map(|b| b.to_vec()));
            match bytes {
                Some(p) => chmod_at(dirfd, &p, mode),
                None => -21,
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// WASI's EBADF, EINVAL, and EPERM, negated as the file operations' results are.
const EBADF: i32 = -8;
/// WASI's EAGAIN: not yet (try_wait on a program still running).
#[cfg_attr(not(unix), allow(dead_code))]
const EAGAIN: i32 = -6;
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
    let owned = os_str(path);
    let text = Path::new(&owned);
    if confined && !stays_inside(text) {
        return EPERM;
    }
    match set_mode(text, mode as u32) {
        Ok(()) => 0,
        Err(e) => crate::vm::wasi_errno(&e) as i32,
    }
}

/// A path or an argument the program gave as bytes, as the operating system
/// takes it: unchanged on Unix, where a name is bytes; on Windows, where a name
/// is text, as UTF-8 (bytes that are not are replaced).
pub(crate) fn os_str(bytes: &[u8]) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        std::ffi::OsStr::from_bytes(bytes).to_owned()
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// Sets the permission bits: the nine of `mode` on Unix; on Windows, which has
/// no such bits, the read-only attribute, set when no one may write.
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_readonly(mode & 0o222 == 0);
        std::fs::set_permissions(path, perms)
    }
}

/// Makes a file runnable (mode 0o755) where that is a permission, as the
/// toolchain does for the executables it writes; Windows runs a file by its name.
pub fn make_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        set_mode(path, 0o755)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
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

// ===== Paths and the file system a program sees =====

/// A host path as the programs that run under wasmtime name it. They see the
/// working directory (".") and `/`, and take `/` as their separator. On
/// Windows `/` is the root of the working directory's drive and another
/// drive's root is `/X:/` (`preopen_roots`), so `C:\a\b` becomes `/a/b` when C:
/// is the working directory's drive and `/C:/a/b` when it is not; a verbatim
/// prefix (`\\?\`) is dropped. Anywhere else a path is already what they use.
pub fn guest_path(path: &str) -> String {
    #[cfg(not(windows))]
    {
        path.to_string()
    }
    #[cfg(windows)]
    {
        let path = path.strip_prefix(r"\\?\").unwrap_or(path);
        let s = path.replace('\\', "/");
        match drive_of(&s) {
            Some(d) => {
                let rest = &s[2..];
                let rest = if rest.starts_with('/') { rest.to_string() } else { format!("/{rest}") };
                if Some(d) == current_drive() { rest } else { format!("/{d}:{rest}") }
            }
            None => s,
        }
    }
}

/// A command-line argument as a program should see it: one that is an absolute Windows path (`C:\x`,
/// `D:/x`) becomes the guest's name for it (`guest_path`), so a program can open what it was given; any
/// other argument is unchanged, as everywhere else.
pub fn guest_arg(arg: &str) -> String {
    #[cfg(windows)]
    {
        let s = arg.strip_prefix(r"\\?\").unwrap_or(arg);
        if drive_of(&s.replace('\\', "/")).is_some() && matches!(s.as_bytes().get(2), Some(b'\\' | b'/')) {
            return guest_path(arg);
        }
    }
    arg.to_string()
}

/// The drive letter (upper case) a path starts with, as in `C:/x`.
#[cfg(windows)]
fn drive_of(s: &str) -> Option<char> {
    let b = s.as_bytes();
    (b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic()).then(|| (b[0] as char).to_ascii_uppercase())
}

/// The drive the working directory is on.
#[cfg(windows)]
fn current_drive() -> Option<char> {
    let dir = std::env::current_dir().ok()?;
    let s = dir.to_string_lossy().replace('\\', "/");
    drive_of(s.strip_prefix("//?/").unwrap_or(&s))
}

/// Gives a program `/`: the root of the file system on Unix; on Windows the
/// root of the working directory's drive, and each other fixed drive's root
/// as `/X:/` (see `guest_path`).
pub fn preopen_roots(b: &mut wasmtime_wasi::WasiCtxBuilder, perms: wasmtime_wasi::FsPerms) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        b.preopened_dir("/", "/", perms).map_err(|e| format!("cannot open /: {e}"))?;
        Ok(())
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
        const DRIVE_FIXED: u32 = 3;
        let here = current_drive();
        if let Some(d) = here {
            b.preopened_dir(format!("{d}:\\"), "/", perms).map_err(|e| format!("cannot open {d}:\\: {e}"))?;
        }
        // SAFETY: no arguments
        let mask = unsafe { GetLogicalDrives() };
        for i in 0..26u32 {
            let d = (b'A' + i as u8) as char;
            if mask & (1 << i) == 0 || Some(d) == here {
                continue;
            }
            let root = format!("{d}:\\");
            let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: a zero-terminated UTF-16 root path
            if unsafe { GetDriveTypeW(wide.as_ptr()) } == DRIVE_FIXED {
                b.preopened_dir(&root, format!("/{d}:/"), perms).map_err(|e| format!("cannot open {root}: {e}"))?;
            }
        }
        Ok(())
    }
}

// ===== Processes (docs/design/SYS_PLAN.md D2-D7) =====

#[path = "sys_procs.rs"]
mod procs;

pub use procs::{Procs, NOT_GRANTED};
