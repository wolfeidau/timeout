# timeout

Small zero dependency timeout command to avoid needing to install all of coreutils.

## Install

Download the archive for your platform from [GitHub releases](https://github.com/wolfeidau/timeout/releases), extract it, and place `timeout` on your `PATH`.

## Usage

```
timeout [-f] [-k DURATION] [-p] [-s SIGNAL] [-v] DURATION COMMAND [ARG]...
```

- `DURATION` is a number with an optional suffix `s` (default), `m`, `h` or `d`. `0` disables the timeout.
- `-s, --signal` sends SIGNAL (name, `SIG` name or number) on timeout. Default `TERM`.
- `-k, --kill-after` also sends `KILL` if the command is still running this long after the first signal.
- `-p, --preserve-status` exits with the command's status even on timeout.
- `-f, --foreground` does not create a process group; only the command is signalled.
- `-v, --verbose` reports signals sent on stderr.

## Exit codes

| Code | Meaning |
| --- | --- |
| 124 | Command timed out (unless `-p`) |
| 125 | `timeout` itself failed, including usage errors |
| 126 | Command found but cannot be invoked |
| 127 | Command not found |
| 137 | Command was killed by `KILL` |
| other | Exit status of the command; `128+n` if it died from signal `n` |

## Differences from GNU

- Windows has no signals: only the direct child is killed (not its tree), `-s` is accepted but ignored, and `-k` is a no-op.
- `PR_SET_PDEATHSIG` is not used, so the command is not signalled if `timeout` is killed by `KILL`.
- If the command dies from a core-generating signal, `timeout` exits `128+n` rather than re-raising the signal.

## Built with AI

This project is built with AI-assisted development workflows using tools including [OpenAI Codex](https://developers.openai.com/codex/), [Claude Code](https://claude.ai/code), and [Cursor](https://cursor.com).

## License

Apache-2.0. See [LICENSE](LICENSE).
