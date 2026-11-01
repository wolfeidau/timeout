mod cli;
mod signals;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::io::Write;

use cli::Action;

#[cfg(unix)]
use unix::run;
#[cfg(windows)]
use windows::run;

const VERSION: &str = match option_env!("APP_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

const HELP: &str = "\
Usage: timeout [OPTION]... DURATION COMMAND [ARG]...
Start COMMAND, and kill it if still running after DURATION.

  -f, --foreground       run COMMAND in the foreground; do not create a process
                           group, so only COMMAND receives signals
  -k, --kill-after=DURATION
                         also send KILL if COMMAND is still running this long
                           after the first signal was sent
  -p, --preserve-status  exit with the same status as COMMAND, even on timeout
  -s, --signal=SIGNAL    signal to send on timeout (default TERM)
  -v, --verbose          diagnose to stderr any signal sent upon timeout
      --help             display this help and exit
      --version          output version information and exit

DURATION is a number with an optional suffix: 's' seconds (default), 'm'
minutes, 'h' hours, 'd' days. A duration of 0 disables the timeout.

Exit status: 124 on timeout, 125 if timeout itself fails, 126 if COMMAND cannot
be invoked, 127 if COMMAND is not found, 137 if COMMAND (or timeout) is killed
by KILL; otherwise the exit status of COMMAND.
";

fn main() {
    let code = match cli::parse(std::env::args_os().skip(1)) {
        Ok(Action::Run(cfg)) => run(&cfg),
        Ok(Action::Help) => {
            let _ = write!(std::io::stdout(), "{HELP}");
            0
        }
        Ok(Action::Version) => {
            let _ = writeln!(std::io::stdout(), "timeout {VERSION}");
            0
        }
        Err(msg) => {
            cli::error(&msg);
            let _ = writeln!(
                std::io::stderr(),
                "Try 'timeout --help' for more information."
            );
            125
        }
    };
    std::process::exit(code);
}
