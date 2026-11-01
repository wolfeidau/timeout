//! Argument, duration and signal parsing.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use crate::signals;

// Some fields are only used on unix.
#[cfg_attr(not(unix), allow(dead_code))]
pub struct Config {
    pub foreground: bool,
    pub kill_after: Option<Duration>,
    pub preserve_status: bool,
    pub signal: i32,
    pub verbose: bool,
    /// `None` disables the timer.
    pub duration: Option<Duration>,
    pub command: OsString,
    pub args: Vec<OsString>,
}

pub enum Action {
    Run(Config),
    Help,
    Version,
}

/// (long name, short key, takes a value). Help and version use 'H' and 'V'.
const LONG: &[(&str, char, bool)] = &[
    ("foreground", 'f', false),
    ("help", 'H', false),
    ("kill-after", 'k', true),
    ("preserve-status", 'p', false),
    ("signal", 's', true),
    ("verbose", 'v', false),
    ("version", 'V', false),
];

/// Prints `timeout: MSG` to stderr, ignoring write errors.
pub fn error(msg: &str) {
    let _ = writeln!(std::io::stderr(), "timeout: {msg}");
}

/// Strips Rust's " (os error N)" suffix to match C strerror text.
pub fn io_msg(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.find(" (os error") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

fn resolve(name: &str) -> Result<(char, bool), String> {
    if let Some(&(_, k, t)) = LONG.iter().find(|(n, _, _)| *n == name) {
        return Ok((k, t));
    }
    let mut found = LONG.iter().filter(|(n, _, _)| n.starts_with(name));
    match (found.next(), found.next()) {
        (Some(&(_, k, t)), None) if !name.is_empty() => Ok((k, t)),
        (Some(_), _) if !name.is_empty() => Err(format!("option '--{name}' is ambiguous")),
        _ => Err(format!("unrecognized option '--{name}'")),
    }
}

fn next_value(it: &mut impl Iterator<Item = OsString>, opt: &str) -> Result<String, String> {
    it.next()
        .map(|v| v.to_string_lossy().into_owned())
        .ok_or_else(|| format!("option requires an argument -- '{opt}'"))
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Action, String> {
    let mut it = args.into_iter();
    let mut foreground = false;
    let mut preserve_status = false;
    let mut verbose = false;
    let mut kill_after = None;
    let mut signal = signals::TERM;
    let mut positional: Vec<OsString> = Vec::new();

    while let Some(arg) = it.next() {
        let Some(s) = arg.to_str() else {
            positional.push(arg);
            break;
        };
        if s == "--" {
            break;
        }
        // Collect (key, value) pairs for this argument.
        let mut opts: Vec<(char, Option<String>)> = Vec::new();
        if let Some(long) = s.strip_prefix("--") {
            let (name, val) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let (key, takes) = resolve(name)?;
            let full = LONG
                .iter()
                .find(|(_, k, _)| *k == key)
                .map_or(name, |l| l.0);
            let val = match (takes, val) {
                (true, None) => Some(next_value(&mut it, full)?),
                (false, Some(_)) => {
                    return Err(format!("option '--{full}' doesn't allow an argument"));
                }
                (_, v) => v,
            };
            opts.push((key, val));
        } else if s.len() > 1 && s.starts_with('-') {
            for (i, c) in s.char_indices().skip(1) {
                match c {
                    'f' | 'p' | 'v' => opts.push((c, None)),
                    'k' | 's' => {
                        let rest = &s[i + 1..];
                        let v = if rest.is_empty() {
                            next_value(&mut it, &c.to_string())?
                        } else {
                            rest.to_string()
                        };
                        opts.push((c, Some(v)));
                        break;
                    }
                    _ => return Err(format!("invalid option -- '{c}'")),
                }
            }
        } else {
            positional.push(arg);
            break;
        }
        for (key, val) in opts {
            let val = val.unwrap_or_default();
            match key {
                'f' => foreground = true,
                'p' => preserve_status = true,
                'v' => verbose = true,
                'k' => kill_after = parse_duration(&val)?,
                's' => signal = parse_signal(&val)?,
                'H' => return Ok(Action::Help),
                _ => return Ok(Action::Version),
            }
        }
    }
    positional.extend(it);

    let mut rest = positional.into_iter();
    let dur_arg = rest.next().ok_or("missing operand")?;
    let duration = parse_duration(&dur_arg.to_string_lossy())?;
    let command = rest
        .next()
        .ok_or_else(|| format!("missing operand after '{}'", dur_arg.to_string_lossy()))?;
    Ok(Action::Run(Config {
        foreground,
        kill_after,
        preserve_status,
        signal,
        verbose,
        duration,
        command,
        args: rest.collect(),
    }))
}

/// Parses a non-negative float with optional s/m/h/d suffix. Zero yields `None`.
pub fn parse_duration(s: &str) -> Result<Option<Duration>, String> {
    let bad = || format!("invalid time interval '{s}'");
    let (num, mult) = match s.chars().last() {
        Some('s') => (&s[..s.len() - 1], 1.0),
        Some('m') => (&s[..s.len() - 1], 60.0),
        Some('h') => (&s[..s.len() - 1], 3600.0),
        Some('d') => (&s[..s.len() - 1], 86400.0),
        _ => (s, 1.0),
    };
    let v: f64 = num.parse().map_err(|_| bad())?;
    if !v.is_finite() || v < 0.0 {
        return Err(bad());
    }
    let secs = v * mult;
    if secs == 0.0 {
        return Ok(None);
    }
    let d = Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX);
    Ok(Some(d.max(Duration::from_nanos(1))))
}

/// Parses a signal name (with or without `SIG`) or number.
pub fn parse_signal(s: &str) -> Result<i32, String> {
    let bad = || format!("'{s}': invalid signal");
    if let Ok(n) = s.parse::<i32>() {
        return if (1..=64).contains(&n) {
            Ok(n)
        } else {
            Err(bad())
        };
    }
    let up = s.to_ascii_uppercase();
    let name = up.strip_prefix("SIG").unwrap_or(&up);
    signals::by_name(name).ok_or_else(bad)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Action, String> {
        parse(args.iter().map(OsString::from))
    }

    fn cfg(args: &[&str]) -> Config {
        match p(args) {
            Ok(Action::Run(c)) => c,
            _ => panic!("expected Run for {args:?}"),
        }
    }

    #[test]
    fn durations() {
        assert_eq!(
            parse_duration("1.5").unwrap(),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(
            parse_duration("2m").unwrap(),
            Some(Duration::from_secs(120))
        );
        assert_eq!(
            parse_duration("1h").unwrap(),
            Some(Duration::from_secs(3600))
        );
        assert_eq!(
            parse_duration("1d").unwrap(),
            Some(Duration::from_secs(86400))
        );
        assert_eq!(parse_duration("0").unwrap(), None);
        assert_eq!(parse_duration("0s").unwrap(), None);
        assert!(parse_duration("-1").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("").is_err());
        assert!(parse_duration("inf").is_err());
        assert!(parse_duration("1x").is_err());
        assert_eq!(parse_duration("1e30d").unwrap(), Some(Duration::MAX));
    }

    #[test]
    fn signals() {
        assert_eq!(parse_signal("TERM").unwrap(), signals::TERM);
        assert_eq!(parse_signal("sigkill").unwrap(), signals::KILL);
        assert_eq!(parse_signal("9").unwrap(), 9);
        assert!(parse_signal("0").is_err());
        assert!(parse_signal("BOGUS").is_err());
    }

    #[test]
    fn option_forms() {
        let c = cfg(&["-k5", "-vp", "--signal=HUP", "10", "sleep", "1"]);
        assert_eq!(c.kill_after, Some(Duration::from_secs(5)));
        assert!(c.verbose && c.preserve_status && !c.foreground);
        assert_eq!(c.signal, signals::HUP);
        assert_eq!(c.args, vec![OsString::from("1")]);

        let c = cfg(&["--kill-after", "2", "-s", "INT", "--fore", "1", "x"]);
        assert_eq!(c.kill_after, Some(Duration::from_secs(2)));
        assert_eq!(c.signal, signals::INT);
        assert!(c.foreground);
    }

    #[test]
    fn stops_at_first_non_option() {
        let c = cfg(&["1", "cmd", "-v", "--help"]);
        assert!(!c.verbose);
        assert_eq!(c.args, vec![OsString::from("-v"), OsString::from("--help")]);
        let c = cfg(&["--", "1", "cmd"]);
        assert_eq!(c.command, OsString::from("cmd"));
    }

    #[test]
    fn errors() {
        assert!(p(&[]).is_err());
        assert!(p(&["1"]).is_err());
        assert!(p(&["-z", "1", "x"]).is_err());
        assert!(p(&["--ver", "1", "x"]).is_err());
        assert!(p(&["--bogus", "1", "x"]).is_err());
        assert!(p(&["-k"]).is_err());
        assert!(p(&["--verbose=1", "1", "x"]).is_err());
        assert!(p(&["-s", "NOPE", "1", "x"]).is_err());
    }

    #[test]
    fn help_and_version() {
        assert!(matches!(p(&["--help"]), Ok(Action::Help)));
        assert!(matches!(p(&["--vers"]), Ok(Action::Version)));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_args_preserved() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![0xff, 0xfe]);
        let args = vec![OsString::from("1"), OsString::from("cmd"), bad.clone()];
        match parse(args) {
            Ok(Action::Run(c)) => assert_eq!(c.args, vec![bad]),
            _ => panic!("expected Run"),
        }
    }
}
