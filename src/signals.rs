//! Signal names and numbers. Numbers differ between Linux and macOS.
//! Linux x86_64 and aarch64 share the same numbering.

pub const HUP: i32 = 1;
pub const INT: i32 = 2;
pub const QUIT: i32 = 3;
pub const ILL: i32 = 4;
pub const TRAP: i32 = 5;
pub const ABRT: i32 = 6;
pub const FPE: i32 = 8;
pub const KILL: i32 = 9;
pub const SEGV: i32 = 11;
pub const PIPE: i32 = 13;
pub const ALRM: i32 = 14;
pub const TERM: i32 = 15;
pub const TTIN: i32 = 21;
pub const TTOU: i32 = 22;
pub const XCPU: i32 = 24;
pub const XFSZ: i32 = 25;
pub const VTALRM: i32 = 26;
pub const PROF: i32 = 27;
pub const WINCH: i32 = 28;

#[cfg(not(target_os = "macos"))]
mod os {
    pub const BUS: i32 = 7;
    pub const USR1: i32 = 10;
    pub const USR2: i32 = 12;
    pub const CHLD: i32 = 17;
    pub const CONT: i32 = 18;
    pub const STOP: i32 = 19;
    pub const TSTP: i32 = 20;
    pub const URG: i32 = 23;
    pub const SYS: i32 = 31;
    pub const EXTRA: &[(&str, i32)] = &[("STKFLT", 16), ("IO", 29), ("PWR", 30), ("POLL", 29)];
}

#[cfg(target_os = "macos")]
mod os {
    pub const BUS: i32 = 10;
    pub const USR1: i32 = 30;
    pub const USR2: i32 = 31;
    pub const CHLD: i32 = 20;
    pub const CONT: i32 = 19;
    pub const STOP: i32 = 17;
    pub const TSTP: i32 = 18;
    pub const URG: i32 = 16;
    pub const SYS: i32 = 12;
    pub const EXTRA: &[(&str, i32)] = &[("EMT", 7), ("IO", 23), ("INFO", 29)];
}

#[cfg(unix)]
pub use os::{CHLD, CONT, USR1, USR2};

/// Canonical names first, so reverse lookup prefers them.
const COMMON: &[(&str, i32)] = &[
    ("HUP", HUP),
    ("INT", INT),
    ("QUIT", QUIT),
    ("ILL", ILL),
    ("TRAP", TRAP),
    ("ABRT", ABRT),
    ("FPE", FPE),
    ("KILL", KILL),
    ("SEGV", SEGV),
    ("PIPE", PIPE),
    ("ALRM", ALRM),
    ("TERM", TERM),
    ("TTIN", TTIN),
    ("TTOU", TTOU),
    ("XCPU", XCPU),
    ("XFSZ", XFSZ),
    ("VTALRM", VTALRM),
    ("PROF", PROF),
    ("WINCH", WINCH),
    ("BUS", os::BUS),
    ("USR1", os::USR1),
    ("USR2", os::USR2),
    ("CHLD", os::CHLD),
    ("CONT", os::CONT),
    ("STOP", os::STOP),
    ("TSTP", os::TSTP),
    ("URG", os::URG),
    ("SYS", os::SYS),
];

/// Looks up a signal by name (without the `SIG` prefix).
pub fn by_name(name: &str) -> Option<i32> {
    COMMON
        .iter()
        .chain(os::EXTRA)
        .find(|(n, _)| *n == name)
        .map(|(_, v)| *v)
}

#[cfg(unix)]
/// Returns the name for a signal number, if known.
pub fn name_of(num: i32) -> Option<&'static str> {
    COMMON
        .iter()
        .chain(os::EXTRA)
        .find(|(_, v)| *v == num)
        .map(|(n, _)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_roundtrip() {
        assert_eq!(by_name("TERM"), Some(15));
        assert_eq!(name_of(KILL), Some("KILL"));
        assert_eq!(by_name("NOPE"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_numbers() {
        assert_eq!((CHLD, CONT, USR1, USR2), (17, 18, 10, 12));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_numbers() {
        assert_eq!((CHLD, CONT, USR1, USR2), (20, 19, 30, 31));
    }
}
