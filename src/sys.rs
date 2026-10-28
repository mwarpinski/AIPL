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

// ===== Processes (docs/design/SYS_PLAN.md D2-D7) =====

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// WASI's ENOTCAPABLE: a program that was not granted `--allow-run` for the
/// program it asked to start.
pub const NOT_GRANTED: i32 = -76;
/// A handle that is not the program's.
const BAD_HANDLE: i32 = EBADF;

/// The processes a program started and the pipes to them, in tables of their
/// own, apart from WASI's file descriptors: a pipe's handle counts up from 3,
/// a process's from 1024, and neither is a descriptor (std/proc keeps them
/// private; what a number means is each backend's). Shared by every thread of
/// a program; blocking calls run outside the table's lock.
#[derive(Clone, Default)]
pub struct Procs {
    allow: Arc<Vec<String>>,
    table: Arc<Mutex<Table>>,
}

#[derive(Default)]
struct Table {
    next_pipe: i32,
    next_proc: i32,
    procs: HashMap<i32, Proc>,
    pipes: HashMap<i32, Pipe>,
}

struct Proc {
    child: Arc<Mutex<Child>>,
    status: Arc<Mutex<Option<i32>>>,
    /// the pipe handles of its input, output, and error, where they are pipes
    streams: [Option<i32>; 3],
}

enum Pipe {
    /// the program's output or error: this program reads it
    Read(Arc<File>),
    /// the program's input: this program writes it
    Write(Arc<File>),
}

impl Procs {
    /// A table whose program may start the programs `allow` names ("*": any).
    pub fn new(allow: Vec<String>) -> Procs {
        Procs { allow: Arc::new(allow), table: Arc::new(Mutex::new(Table { next_pipe: 3, next_proc: 1024, ..Table::default() })) }
    }

    fn granted(&self, path: &[u8]) -> bool {
        self.allow.iter().any(|g| g == "*" || g.as_bytes() == path)
    }

    /// The command for the program at `path` with `args`: each argument
    /// followed by a zero byte, the first the program's own name.
    fn command(path: &[u8], args: &[u8]) -> Command {
        let mut words: Vec<&[u8]> = args.split(|&b| b == 0).collect();
        // the block ends with a zero byte, so the last piece is empty
        if args.last() == Some(&0) || args.is_empty() {
            words.pop();
        }
        let mut cmd = Command::new(OsStr::from_bytes(path));
        let mut words = words.into_iter();
        if let Some(name) = words.next() {
            cmd.arg0(OsStr::from_bytes(name));
        }
        for w in words {
            cmd.arg(OsStr::from_bytes(w));
        }
        cmd
    }

    /// `proc.spawn`: a process's handle, or minus an errno (-76 if not
    /// granted, -28 for a `stdio` that is not one of the three fields' values).
    pub fn spawn(&self, path: &[u8], args: &[u8], stdio: i32) -> i32 {
        if !self.granted(path) {
            return NOT_GRANTED;
        }
        let (input, output, error) = (stdio & 3, (stdio >> 2) & 3, (stdio >> 4) & 3);
        if input == 3 || output == 3 || stdio >> 6 != 0 {
            return EINVAL;
        }
        let mut cmd = Procs::command(path, args);
        let mut kept: [Option<Pipe>; 3] = [None, None, None];
        let made = (|| -> std::io::Result<()> {
            match input {
                0 => {}
                1 => {
                    let (r, w) = std::io::pipe()?;
                    cmd.stdin(Stdio::from(r));
                    kept[0] = Some(Pipe::Write(Arc::new(File::from(OwnedFd::from(w)))));
                }
                _ => {
                    cmd.stdin(Stdio::null());
                }
            }
            // the output's writing end, for the error to share (error 3)
            let mut out_end: Option<OwnedFd> = None;
            match output {
                0 => {}
                1 => {
                    let (r, w) = std::io::pipe()?;
                    let w = OwnedFd::from(w);
                    out_end = Some(w.try_clone()?);
                    cmd.stdout(Stdio::from(w));
                    kept[1] = Some(Pipe::Read(Arc::new(File::from(OwnedFd::from(r)))));
                }
                _ => {
                    cmd.stdout(Stdio::null());
                }
            }
            match error {
                0 => {}
                1 => {
                    let (r, w) = std::io::pipe()?;
                    cmd.stderr(Stdio::from(w));
                    kept[2] = Some(Pipe::Read(Arc::new(File::from(OwnedFd::from(r)))));
                }
                2 => {
                    cmd.stderr(Stdio::null());
                }
                _ => match (output, out_end) {
                    (1, Some(w)) => {
                        cmd.stderr(Stdio::from(w));
                    }
                    (0, _) => {
                        cmd.stderr(Stdio::from(std::io::stdout().as_fd().try_clone_to_owned()?));
                    }
                    _ => {
                        cmd.stderr(Stdio::null());
                    }
                },
            }
            Ok(())
        })();
        if let Err(e) = made {
            return crate::vm::wasi_errno(&e) as i32;
        }
        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return crate::vm::wasi_errno(&e) as i32,
        };
        // the parent's copies of the program's ends go with `cmd`
        drop(cmd);
        let mut t = self.table.lock().unwrap();
        let mut streams = [None, None, None];
        for (k, pipe) in kept.into_iter().enumerate() {
            if let Some(p) = pipe {
                let h = t.next_pipe;
                t.next_pipe += 1;
                t.pipes.insert(h, p);
                streams[k] = Some(h);
            }
        }
        let p = t.next_proc;
        t.next_proc += 1;
        t.procs.insert(p, Proc { child: Arc::new(Mutex::new(child)), status: Arc::new(Mutex::new(None)), streams });
        p
    }

    /// `proc.stream`: the pipe of process `p`'s input (0), output (1), or
    /// error (2), or -1 if that stream is not a pipe, minus EBADF for no such process.
    pub fn stream(&self, p: i32, which: i32) -> i32 {
        let t = self.table.lock().unwrap();
        match t.procs.get(&p) {
            None => BAD_HANDLE,
            Some(pr) => match usize::try_from(which).ok().and_then(|k| pr.streams.get(k)) {
                Some(Some(h)) if t.pipes.contains_key(h) => *h,
                _ => -1,
            },
        }
    }

    fn pipe_file(&self, h: i32, reading: bool) -> Option<Arc<File>> {
        match self.table.lock().unwrap().pipes.get(&h)? {
            Pipe::Read(f) if reading => Some(Arc::clone(f)),
            Pipe::Write(f) if !reading => Some(Arc::clone(f)),
            _ => None,
        }
    }

    /// `proc.read`: up to `max` bytes (waits for some), 0 at the end, or minus an errno.
    pub fn read(&self, h: i32, max: usize) -> Result<Vec<u8>, i32> {
        let f = self.pipe_file(h, true).ok_or(BAD_HANDLE)?;
        let mut buf = vec![0u8; max];
        match (&*f).read(&mut buf) {
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(e) => Err(crate::vm::wasi_errno(&e) as i32),
        }
    }

    /// `proc.write`: bytes written, or minus an errno (a program that has gone is -64, not a signal).
    pub fn write(&self, h: i32, bytes: &[u8]) -> i32 {
        let Some(f) = self.pipe_file(h, false) else { return BAD_HANDLE };
        match (&*f).write(bytes) {
            Ok(n) => n as i32,
            Err(e) => crate::vm::wasi_errno(&e) as i32,
        }
    }

    /// `proc.close`: closes a pipe (closing the input tells the program it
    /// has ended), or lets go of a process (which keeps running); 0, or minus EBADF.
    pub fn close(&self, h: i32) -> i32 {
        let mut t = self.table.lock().unwrap();
        if t.pipes.remove(&h).is_some() || t.procs.remove(&h).is_some() {
            0
        } else {
            BAD_HANDLE
        }
    }

    /// `proc.wait`: waits for the program to end: its exit status, or 256
    /// plus the signal that ended it; the same on a second wait.
    pub fn wait(&self, p: i32) -> i32 {
        let (child, status) = {
            let t = self.table.lock().unwrap();
            match t.procs.get(&p) {
                Some(pr) => (Arc::clone(&pr.child), Arc::clone(&pr.status)),
                None => return BAD_HANDLE,
            }
        };
        let mut done = status.lock().unwrap();
        if let Some(s) = *done {
            return s;
        }
        let result = match child.lock().unwrap().wait() {
            Ok(st) => st.code().unwrap_or_else(|| 256 + st.signal().unwrap_or(0)),
            Err(e) => return crate::vm::wasi_errno(&e) as i32,
        };
        *done = Some(result);
        result
    }

    /// `proc.exec`: replaces this process with the program; does not return
    /// except with minus an errno (-76 if not granted).
    pub fn exec(&self, path: &[u8], args: &[u8]) -> i32 {
        if !self.granted(path) {
            return NOT_GRANTED;
        }
        let _ = std::io::stdout().flush();
        crate::vm::wasi_errno(&Procs::command(path, args).exec()) as i32
    }
}
