# 🔎 logsleuth

A fast command-line tool, written in Rust, that reads a log file and tells you **what went wrong, how often, and when**.

It groups lines that differ only in ids, numbers, IPs or timestamps, so 3,000 copies of the same timeout turn into one line with `x3000`.

[![CI](https://github.com/lprvy/logsleuth/actions/workflows/ci.yml/badge.svg)](https://github.com/lprvy/logsleuth/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/lprvy/logsleuth)](https://github.com/lprvy/logsleuth/releases/latest) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

```text
$ logsleuth examples/app.log
12 lines · 4 INFO · 3 WARN · 4 ERROR · 1 FATAL

 1. [FATAL] x1  connection pool exhausted (max <n>)
    e.g. line 11: 2026-11-10 09:05:01 FATAL connection pool exhausted (max 20)

 2. [ERROR] x3  payment provider timeout for order <uuid> after <n>
    e.g. line 4: 2026-11-10 09:01:15 ERROR payment provider timeout for order 3f2b8c1e-… after 30000ms
    seen 2026-11-10 09:01:15 → 2026-11-10 09:05:00 (lines 4–10)

 3. [ERROR] x1  invalid coupon "<str>" for user <n>
 ...
```

## Install

Prebuilt binaries for Linux (x86_64), macOS (Apple silicon) and Windows (x86_64) are on the [releases page](https://github.com/lprvy/logsleuth/releases/latest): download, unpack and put `logsleuth` on your `PATH`.

Or build it with Cargo:

```bash
cargo install --git https://github.com/lprvy/logsleuth
```

## Usage

```bash
logsleuth app.log                         # problems at WARN and above
logsleuth app.log other.log -l error      # several files, ERROR and above
kubectl logs my-pod | logsleuth -n 5      # from stdin, top 5 groups
logsleuth app.log -f json                 # machine-readable output
logsleuth app.log --fail-over 0           # exit 1 if there is any ERROR/FATAL line (CI gate)
```

| Option | Default | |
|---|---|---|
| `-l, --level` | `warn` | lowest level to group: `debug`, `info`, `warn`, `error`, `fatal` |
| `-n, --top` | `10` | max groups to show |
| `-f, --format` | `text` | `text` or `json` |
| `--fail-over N` | – | exit code 1 when more than N lines are ERROR or worse |

Exit codes: `0` ok · `1` `--fail-over` limit exceeded · `2` input error (e.g. missing file).

## What it understands

- **Levels** in plain (`ERROR`), bracketed (`[WARN]`), key-value (`level=info`), JSON (`"level":"fatal"`), syslog (`warning:`) and Rust panic (`panicked`) styles, case-insensitive. Words that merely contain a level (`terrorism`) are ignored.
- **Timestamps:** ISO 8601 (`2026-11-15T10:00:01Z`, `2026-11-15 10:00:01,123`) and syslog (`Nov 15 10:00:05`), used for the "seen … → …" range.
- **Kubernetes / Go klog lines** (`E1115 10:00:07.123456 12345 controller.go:114] …`): the level comes from the first letter (`I`, `W`, `E`, `F`) and the time from the prefix.
- **Masked values:** timestamps, UUIDs, emails, IPv4 addresses (with port), hex values, long hashes, quoted strings, numbers with units (`350ms`, `20MB`, `99%`), and ids that mix letters and digits (pod suffixes like `api-7d9f8`, host names like `web01` → `<id>`).
- **Context in brackets** such as `[worker-3]` stays whole (`[worker-<n>]`); only the brackets left empty by removing the level or time are dropped.
- **Messy input:** invalid UTF-8 bytes and Windows line endings don't stop it.

## Development

```bash
cargo test     # 14 tests: 9 unit + 5 end-to-end that run the real binary
cargo clippy --all-targets -- -D warnings
```

CI runs fmt, clippy and the tests on Linux, Windows and macOS.

## License

MIT
