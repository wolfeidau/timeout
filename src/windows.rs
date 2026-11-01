//! Reduced Windows implementation: no signals, only the direct child is killed.

use std::io::ErrorKind;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::cli::{self, Config};

pub fn run(cfg: &Config) -> i32 {
    let mut child = match Command::new(&cfg.command).args(&cfg.args).spawn() {
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
    let deadline = cfg.duration.and_then(|d| Instant::now().checked_add(d));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code().unwrap_or(1),
            Ok(None) => {}
            Err(e) => {
                cli::error(&cli::io_msg(&e));
                return 125;
            }
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            if cfg.verbose {
                cli::error(&format!(
                    "killing command '{}'",
                    cfg.command.to_string_lossy()
                ));
            }
            let _ = child.kill();
            let status = child.wait();
            if cfg.preserve_status {
                return status.ok().and_then(|s| s.code()).unwrap_or(1);
            }
            return 124;
        }
        sleep(Duration::from_millis(10));
    }
}
