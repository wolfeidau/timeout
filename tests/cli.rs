#![cfg(unix)]

use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_timeout"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().unwrap()
}

fn code(args: &[&str]) -> i32 {
    run(args).status.code().expect("exited by signal")
}

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("timeout-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn wait_for(path: &Path) {
    let end = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(Instant::now() < end, "timed out waiting for {path:?}");
        sleep(Duration::from_millis(10));
    }
}

#[test]
fn times_out_with_124() {
    let t = Instant::now();
    assert_eq!(code(&["0.2", "sleep", "10"]), 124);
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn passes_child_status() {
    assert_eq!(code(&["10", "sh", "-c", "exit 7"]), 7);
    assert_eq!(code(&["10", "true"]), 0);
}

#[test]
fn zero_disables_timer() {
    assert_eq!(code(&["0", "sh", "-c", "sleep 0.3; exit 3"]), 3);
}

#[test]
fn not_found_is_127() {
    let o = run(&["10", "/nonexistent/command"]);
    assert_eq!(o.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("timeout: failed to run command"));
}

#[test]
fn not_executable_is_126() {
    let d = tmpdir("noexec");
    let f = d.join("file");
    fs::write(&f, "x").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(code(&["10", f.to_str().unwrap()]), 126);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn usage_errors_are_125() {
    for args in [
        &[][..],
        &["10"],
        &["abc", "true"],
        &["-z", "1", "true"],
        &["-s", "NOPE", "1", "true"],
    ] {
        let o = run(args);
        assert_eq!(o.status.code(), Some(125), "{args:?}");
        assert!(String::from_utf8_lossy(&o.stderr).starts_with("timeout: "));
    }
}

#[test]
fn help_and_version_exit_0_on_stdout() {
    for flag in ["--help", "--version"] {
        let o = run(&[flag]);
        assert_eq!(o.status.code(), Some(0));
        assert!(!o.stdout.is_empty());
    }
}

#[test]
fn kill_after_escalates_to_137() {
    // Foreground: only the child is signalled, so timeout itself exits 137.
    let t = Instant::now();
    let c = code(&[
        "--foreground",
        "-k",
        "0.3",
        "0.5",
        "sh",
        "-c",
        "trap '' TERM; exec sleep 30",
    ]);
    assert_eq!(c, 137);
    assert!(t.elapsed() < Duration::from_secs(10));
}

#[test]
fn kill_after_group_kill_takes_down_timeout() {
    // Group mode: the final KILL also kills timeout, so the shell sees 137.
    let o = run(&[
        "-k",
        "0.3",
        "0.5",
        "sh",
        "-c",
        "trap '' TERM; exec sleep 30",
    ]);
    assert_eq!(o.status.signal(), Some(9));
}

#[test]
fn preserve_status_returns_child_status() {
    assert_eq!(code(&["-p", "0.3", "sleep", "10"]), 128 + 15);
    assert_eq!(
        code(&[
            "-p",
            "0.3",
            "sh",
            "-c",
            "trap 'exit 5' TERM; sleep 10 & wait"
        ]),
        5
    );
}

#[test]
fn child_exiting_by_itself_after_timeout_is_124() {
    assert_eq!(
        code(&["0.3", "sh", "-c", "trap 'exit 5' TERM; sleep 10 & wait"]),
        124
    );
}

#[test]
fn signal_option_selects_signal() {
    // USR1 terminates sleep; -p exposes which signal was used.
    let n = if cfg!(target_os = "macos") { 30 } else { 10 };
    assert_eq!(code(&["-p", "-s", "USR1", "0.3", "sleep", "10"]), 128 + n);
    assert_eq!(code(&["-p", "-s", "SIGHUP", "0.3", "sleep", "10"]), 128 + 1);
    assert_eq!(code(&["-p", "-s", "2", "0.3", "sleep", "10"]), 128 + 2);
}

#[test]
fn foreground_kill_exits_137() {
    assert_eq!(
        code(&["--foreground", "-s", "KILL", "0.3", "sleep", "10"]),
        137
    );
}

#[test]
fn group_kill_with_kill_signal_kills_timeout_itself() {
    let o = run(&["-s", "KILL", "0.3", "sleep", "10"]);
    assert_eq!(o.status.signal(), Some(9));
}

#[test]
fn verbose_reports_signal() {
    let o = run(&["-v", "0.2", "sleep", "10"]);
    assert_eq!(o.status.code(), Some(124));
    assert!(
        String::from_utf8_lossy(&o.stderr)
            .contains("timeout: sending signal TERM to command 'sleep'"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

#[test]
fn long_option_forms() {
    assert_eq!(
        code(&["--kill=1", "--sig=TERM", "--pres", "0.2", "sleep", "10"]),
        143
    );
    assert_eq!(code(&["-k1", "-s", "TERM", "0.2", "sleep", "10"]), 124);
    assert_eq!(code(&["--", "0.2", "sleep", "10"]), 124);
}

#[test]
fn forwards_signals_to_child() {
    let d = tmpdir("fwd");
    let ready = d.join("ready");
    let script = format!(
        "trap 'exit 42' TERM; touch '{}'; sleep 30 & wait",
        ready.display()
    );
    let mut child = bin().args(["30", "sh", "-c", &script]).spawn().unwrap();
    wait_for(&ready);
    let st = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(st.success());
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(42));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn forwarded_signal_starts_kill_after() {
    let d = tmpdir("fwdkill");
    let ready = d.join("ready");
    let script = format!("trap '' TERM; touch '{}'; exec sleep 30", ready.display());
    let t = Instant::now();
    let mut child = bin()
        .args(["-f", "-k", "0.3", "30", "sh", "-c", &script])
        .spawn()
        .unwrap();
    wait_for(&ready);
    let st = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(st.success());
    assert_eq!(child.wait().unwrap().code(), Some(137));
    assert!(t.elapsed() < Duration::from_secs(10));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn runs_scripts_without_shebang() {
    let d = tmpdir("noshebang");
    let f = d.join("script");
    // Written by a separate process: an fd inherited by a concurrent test's
    // fork could otherwise fail exec with ETXTBSY.
    let st = Command::new("sh")
        .args(["-c", r#"echo 'exit 9' > "$0"; chmod +x "$0""#])
        .arg(&f)
        .status()
        .unwrap();
    assert!(st.success());
    let path = f.to_str().unwrap();
    assert_eq!(code(&["5", path]), 9);
    assert_eq!(code(&["-f", "5", path]), 9);
    // A bare name is resolved through PATH.
    let st = bin()
        .env("PATH", format!("{}:/usr/bin:/bin", d.display()))
        .args(["5", "script"])
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(9));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn kills_grandchildren_via_group() {
    let d = tmpdir("grand");
    let pidfile = d.join("pid");
    let script = format!("sleep 30 & echo $! > '{}'; wait", pidfile.display());
    let o = bin()
        .args(["1", "sh", "-c", &script])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(124));
    let pid = fs::read_to_string(&pidfile).unwrap().trim().to_string();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let alive = Command::new("kill")
            .args(["-0", &pid])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        if !alive {
            break;
        }
        assert!(Instant::now() < end, "grandchild {pid} still alive");
        sleep(Duration::from_millis(50));
    }
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn non_utf8_args_pass_through() {
    let arg = OsStr::from_bytes(b"a\xff\xfeb");
    let o = bin()
        .args(["10", "sh", "-c", r#"printf %s "$1""#, "sh"])
        .arg(arg)
        .output()
        .unwrap();
    assert_eq!(o.stdout, b"a\xff\xfeb");
}

#[test]
fn inherited_blocked_sigchld_does_not_hang() {
    let start = std::time::Instant::now();
    let status = std::process::Command::new("perl")
        .args([
            "-e",
            "use POSIX; sigprocmask(SIG_BLOCK, POSIX::SigSet->new(SIGCHLD)); exec @ARGV",
            env!("CARGO_BIN_EXE_timeout"),
            "-k",
            "1",
            "5",
            "true",
        ])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
    assert!(start.elapsed() < std::time::Duration::from_secs(3));
}
