//! The process table of `aipl_sys_v1` (docs/design/SYS_PLAN.md D2-D7) on
//! Windows: the interface sys_unix.rs has, answering every call that would
//! start or touch a process with WASI's ENOSYS ("function not implemented"),
//! which std/proc reports to the program as an error it can handle. A program
//! that is not granted the program it asks for still gets ENOTCAPABLE, as on
//! Unix, so the grant check means the same everywhere.

use super::EBADF;

/// WASI's ENOTCAPABLE: a program that was not granted `--allow-run` for the
/// program it asked to start.
pub const NOT_GRANTED: i32 = -76;
/// WASI's ENOSYS, negated as the file operations' results are.
const UNSUPPORTED: i32 = -52;

/// The processes a program started: none can be, on Windows.
#[derive(Clone, Default)]
pub struct Procs {
    allow: std::sync::Arc<Vec<String>>,
}

impl Procs {
    /// A table whose program may start the programs `allow` names ("*": any).
    pub fn new(allow: Vec<String>) -> Procs {
        Procs { allow: std::sync::Arc::new(allow) }
    }

    fn granted(&self, path: &[u8]) -> bool {
        self.allow.iter().any(|g| g == "*" || g.as_bytes() == path)
    }

    /// `proc.spawn`: -76 if not granted, else ENOSYS.
    pub fn spawn(&self, path: &[u8], _args: &[u8], _stdio: i32) -> i32 {
        if !self.granted(path) {
            return NOT_GRANTED;
        }
        UNSUPPORTED
    }

    /// `proc.stream`: no process has a pipe, so EBADF.
    pub fn stream(&self, _p: i32, _which: i32) -> i32 {
        EBADF
    }

    /// `proc.read`: no handle is a pipe, so EBADF.
    pub fn read(&self, _h: i32, _max: usize) -> Result<Vec<u8>, i32> {
        Err(EBADF)
    }

    /// `proc.write`: no handle is a pipe, so EBADF.
    pub fn write(&self, _h: i32, _bytes: &[u8]) -> i32 {
        EBADF
    }

    /// `proc.close`: no handle is open, so EBADF.
    pub fn close(&self, _h: i32) -> i32 {
        EBADF
    }

    /// `proc.wait`: no process, so EBADF.
    pub fn wait(&self, _p: i32) -> i32 {
        EBADF
    }

    /// `proc.try_wait`: no process, so EBADF.
    pub fn try_wait(&self, _p: i32) -> i32 {
        EBADF
    }

    /// `proc.kill`: no process, so EBADF.
    pub fn kill(&self, _p: i32, _sig: i32) -> i32 {
        EBADF
    }

    /// `proc.poll`: no pipe, so EBADF.
    pub fn poll(&self, _h: i32, _events: i32, _timeout_ms: i32) -> i32 {
        EBADF
    }

    /// `proc.exec`: -76 if not granted, else ENOSYS (Windows has no exec).
    pub fn exec(&self, path: &[u8], _args: &[u8]) -> i32 {
        if !self.granted(path) {
            return NOT_GRANTED;
        }
        UNSUPPORTED
    }
}
