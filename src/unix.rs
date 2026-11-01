//! Unix implementation. Zero dependencies, so the few libc calls are declared here.
//!
//! Signals are delivered through a self-pipe: the handler writes the signal
//! number to a non-blocking socket and the main loop reads it with a timeout
//! derived from the deadline.

use std::ffi::{OsStr, c_int, c_void};
use std::io::{self, ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

use crate::cli::{self, Config};
use crate::signals;

const SIG_DFL: usize = 0;
const SIG_IGN: usize = 1;
const MAX_WAIT: Duration = Duration::from_secs(3600);
const ENOEXEC: i32 = 8;
#[cfg(target_os = "linux")]
const SIG_SETMASK: c_int = 2;
#[cfg(target_os = "macos")]
const SIG_SETMASK: c_int = 3;

unsafe extern "C" {
    fn signal(signum: c_int, handler: usize) -> usize;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn setpgid(pid: c_int, pgid: c_int) -> c_int;
    fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    fn raise(sig: c_int) -> c_int;
    fn sigemptyset(set: *mut c_void) -> c_int;
    fn pthread_sigmask(how: c_int, set: *const c_void, old: *mut c_void) -> c_int;
    #[cfg(target_os = "linux")]
    #[link_name = "__errno_location"]
    fn errno_location() -> *mut c_int;
    #[cfg(target_os = "macos")]
    #[link_name = "__error"]
    fn errno_location() -> *mut c_int;
}

/// Write end of the self-pipe, read by the signal handler.
static WRITE_FD: AtomicI32 = AtomicI32::new(-1);

/// Async-signal-safe: only an atomic load and write(2); errno is preserved.
extern "C" fn on_signal(sig: c_int) {
    let fd = WRITE_FD.load(Ordering::Relaxed);
    if fd < 0 {
        return;
    }
    let byte = sig as u8;
    // SAFETY: errno_location returns this thread's errno, valid for reads and
    // writes. write(2) is async-signal-safe and `byte` outlives the call.
    unsafe {
        let errno = errno_location();
        let saved = *errno;
        write(fd, (&raw const byte).cast(), 1);
        *errno = saved;
    }
}

fn set_handler(sig: c_int, handler: usize) {
    // SAFETY: `handler` is SIG_DFL, SIG_IGN or an `extern "C" fn(c_int)`.
    // Failure (e.g. for KILL/STOP) is harmless and ignored.
    unsafe { signal(sig, handler) };
}

/// Sends `sig` like GNU timeout: to the child, then to the whole group.
fn send_signal(pid: c_int, sig: c_int, foreground: bool) {
    // SAFETY: kill(2) takes plain integers.
    unsafe { kill(pid, sig) };
    if foreground {
        return;
    }
    // Ignore the signal in ourselves just before the group send so we survive
    // it. Done after spawn, so the child does not inherit SIG_IGN. KILL cannot
    // be ignored, so with `-s KILL` the group send kills timeout too.
    set_handler(sig, SIG_IGN);
    // SAFETY: kill(2) takes plain integers; pid 0 is our own process group.
    unsafe { kill(0, sig) };
    if sig != signals::KILL && sig != signals::CONT {
        // SAFETY: as above.
        unsafe {
            kill(pid, signals::CONT);
            kill(0, signals::CONT);
        }
    }
}

#[derive(PartialEq)]
enum Phase {
    Term,
    Kill,
}

struct Runner<'a> {
    cfg: &'a Config,
    pid: c_int,
    phase: Phase,
    timed_out: bool,
    deadline: Option<Instant>,
}

impl Runner<'_> {
    fn send(&self, sig: c_int) {
        if self.cfg.verbose {
            let name = signals::name_of(sig).map_or_else(|| sig.to_string(), str::to_string);
            cli::error(&format!(
                "sending signal {name} to command '{}'",
                self.cfg.command.to_string_lossy()
            ));
        }
        send_signal(self.pid, sig, self.cfg.foreground);
    }

    fn arm_kill(&mut self) {
        self.phase = Phase::Kill;
        self.deadline = self
            .cfg
            .kill_after
            .and_then(|d| Instant::now().checked_add(d));
    }

    /// Forwards a received signal; like GNU, this also starts the `-k` timer.
    fn forward(&mut self, sig: c_int) {
        self.send(sig);
        if self.phase == Phase::Term && self.cfg.kill_after.is_some() {
            self.arm_kill();
        }
    }

    /// Deadline reached (or SIGALRM received): advance to the next phase.
    fn expire(&mut self) {
        match self.phase {
            Phase::Term => {
                self.timed_out = true;
                self.send(self.cfg.signal);
                self.arm_kill();
            }
            Phase::Kill => {
                self.timed_out = true;
                self.send(signals::KILL);
                self.deadline = None;
            }
        }
    }
}

fn status_code(status: ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|s| 128 + s))
        .unwrap_or(1)
}

fn exit_code(status: ExitStatus, timed_out: bool, preserve: bool) -> i32 {
    let Some(sig) = status.signal() else {
        return if timed_out && !preserve {
            124
        } else {
            status_code(status)
        };
    };
    if timed_out {
        // A KILLed child after a timeout is always reported as 137.
        return if preserve || sig == signals::KILL {
            128 + sig
        } else {
            124
        };
    }
    // Re-raise only signals that do not dump core; for others exit 128+n.
    let raisable = [
        signals::HUP,
        signals::INT,
        signals::PIPE,
        signals::ALRM,
        signals::TERM,
        signals::USR1,
        signals::USR2,
        signals::KILL,
    ];
    if raisable.contains(&sig) {
        set_handler(sig, SIG_DFL);
        // SAFETY: raise(3) takes a plain integer.
        unsafe { raise(sig) };
    }
    128 + sig
}

fn fatal(msg: &str) -> i32 {
    cli::error(msg);
    125
}

fn command(program: &OsStr, foreground: bool) -> Command {
    let mut cmd = Command::new(program);
    if !foreground {
        // SAFETY: the closure only calls signal(2), which is async-signal-safe.
        unsafe {
            cmd.pre_exec(|| {
                signal(signals::TTIN, SIG_DFL);
                signal(signals::TTOU, SIG_DFL);
                Ok(())
            });
        }
    }
    cmd
}

/// Finds `name` the way execvp does, for handing to `sh`.
fn resolve(name: &OsStr) -> PathBuf {
    if name.as_bytes().contains(&b'/') {
        return name.into();
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| {
            let dir = if dir.as_os_str().is_empty() {
                ".".into()
            } else {
                dir
            };
            dir.join(name)
        })
        .find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .unwrap_or_else(|| name.into())
}

/// Spawns the command, running scripts without a shebang through `/bin/sh` as
/// glibc's execvp does. musl and posix_spawnp have no such fallback.
fn spawn(cfg: &Config) -> io::Result<Child> {
    let err = match command(&cfg.command, cfg.foreground)
        .args(&cfg.args)
        .spawn()
    {
        Err(e) if e.raw_os_error() == Some(ENOEXEC) => e,
        result => return result,
    };
    command(OsStr::new("/bin/sh"), cfg.foreground)
        .arg("--")
        .arg(resolve(&cfg.command))
        .args(&cfg.args)
        .spawn()
        .map_err(|_| err)
}

/// Clears any inherited signal mask: a blocked SIGCHLD would hang the run loop.
fn unblock_signals() {
    // Large enough for sigset_t on Linux (128 bytes) and macOS (4 bytes).
    let mut set = [0u64; 16];
    let ptr = set.as_mut_ptr().cast::<c_void>();
    // SAFETY: `set` is aligned and large enough for sigset_t, and outlives both calls.
    unsafe {
        sigemptyset(ptr);
        pthread_sigmask(SIG_SETMASK, ptr, std::ptr::null_mut());
    }
}

pub fn run(cfg: &Config) -> i32 {
    unblock_signals();
    if !cfg.foreground {
        // SAFETY: setpgid(2) takes plain integers. Failure is non-fatal.
        unsafe { setpgid(0, 0) };
        // Avoid being stopped by tty access from the background group; the
        // child resets these before exec.
        set_handler(signals::TTIN, SIG_IGN);
        set_handler(signals::TTOU, SIG_IGN);
    }

    let (tx, mut rx) = match UnixStream::pair() {
        Ok(p) => p,
        Err(e) => return fatal(&format!("failed to create socket: {}", cli::io_msg(&e))),
    };
    if let Err(e) = tx.set_nonblocking(true) {
        return fatal(&format!("failed to configure socket: {}", cli::io_msg(&e)));
    }
    WRITE_FD.store(tx.as_raw_fd(), Ordering::Relaxed);

    // Install handlers before spawning so early signals queue in the socket.
    for sig in [
        signals::CHLD,
        signals::ALRM,
        signals::INT,
        signals::QUIT,
        signals::HUP,
        signals::TERM,
        cfg.signal,
    ] {
        set_handler(sig, on_signal as extern "C" fn(c_int) as usize);
    }

    let mut child = match spawn(cfg) {
        Ok(c) => c,
        Err(e) => {
            cli::error(&format!(
                "failed to run command '{}': {}",
                cfg.command.to_string_lossy(),
                cli::io_msg(&e)
            ));
            return if e.kind() == ErrorKind::NotFound {
                127
            } else {
                126
            };
        }
    };

    let mut r = Runner {
        cfg,
        pid: child.id() as c_int,
        phase: Phase::Term,
        timed_out: false,
        deadline: cfg.duration.and_then(|d| Instant::now().checked_add(d)),
    };
    let mut buf = [0u8; 64];
    loop {
        let now = Instant::now();
        if r.deadline.is_some_and(|d| now >= d) {
            r.expire();
            continue;
        }
        let wait = r
            .deadline
            .map_or(MAX_WAIT, |d| d - now)
            .clamp(Duration::from_millis(1), MAX_WAIT);
        if let Err(e) = rx.set_read_timeout(Some(wait)) {
            return fatal(&format!("failed to configure socket: {}", cli::io_msg(&e)));
        }
        let n = match rx.read(&mut buf) {
            Ok(0) => return fatal("signal socket closed"),
            Ok(n) => n,
            // Instant is the source of truth; re-check the deadline.
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(e) => return fatal(&format!("read failed: {}", cli::io_msg(&e))),
        };
        for &b in &buf[..n] {
            let sig = c_int::from(b);
            if sig == signals::CHLD {
                match child.try_wait() {
                    Ok(Some(status)) => return exit_code(status, r.timed_out, cfg.preserve_status),
                    Ok(None) => {}
                    Err(e) => return fatal(&format!("wait failed: {}", cli::io_msg(&e))),
                }
            } else if sig == signals::ALRM && r.phase == Phase::Term {
                r.expire();
            } else if sig != signals::ALRM {
                r.forward(sig);
            }
        }
    }
}
