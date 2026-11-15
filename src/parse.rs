//! Line-level parsing: detect the log level and normalize the message so that
//! lines that differ only in ids, numbers or timestamps group together.

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        match s.to_ascii_lowercase().as_str() {
            "trace" => Some(Level::Trace),
            "debug" => Some(Level::Debug),
            "info" | "notice" => Some(Level::Info),
            "warn" | "warning" => Some(Level::Warn),
            "error" | "err" | "severe" => Some(Level::Error),
            "fatal" | "critical" | "crit" | "panic" | "panicked" | "emerg" | "alert" => {
                Some(Level::Fatal)
            }
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
            Level::Fatal => "FATAL",
        }
    }
}

static LEVEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:^|[\s\[|("]|level=|"level":\s*")(trace|debug|info|notice|warn(?:ing)?|err(?:or)?|severe|fatal|crit(?:ical)?|panic(?:ked)?|emerg|alert)(?:$|[\s\]|):,"])"#,
    )
    .unwrap()
});

/// ISO 8601 (`2026-11-15T10:00:01Z`, `2026-11-15 10:00:01,123`) or syslog (`Nov 15 10:00:05`).
const TIMESTAMP_PATTERN: &str = r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:[.,]\d+)?(?:Z|[+-]\d{2}:?\d{2})?|\b(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) [ \d]\d \d{2}:\d{2}:\d{2}\b";

static TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(TIMESTAMP_PATTERN).unwrap());

/// klog / glog (Kubernetes, Go): `E1115 10:00:07.123456`, the level is the first letter.
static KLOG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([IWEF])(\d{4} \d{2}:\d{2}:\d{2}\.\d+)\s+").unwrap());

/// Tokens mixing letters and digits (`7d9f8`, `a1b2c3`, `web01`): generated ids and host names.
static ALNUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9]{5,}\b").unwrap());

/// Ordered replacements: most specific patterns first.
static MASKS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (TIMESTAMP_PATTERN, ""),
        (
            r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b",
            "<uuid>",
        ),
        (r"[\w.+-]+@[\w-]+\.[\w.]+", "<email>"),
        (r"\b\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?\b", "<ip>"),
        (r"(?i)\b0x[0-9a-f]+\b", "<hex>"),
        (r"(?i)\b[0-9a-f]{16,}\b", "<hash>"),
        (r#""[^"]*""#, "\"<str>\""),
        (r"\b\d+(?:\.\d+)?(?:ms|s|kb|mb|gb|%)?\b", "<n>"),
    ]
    .into_iter()
    .map(|(p, r)| (Regex::new(p).unwrap(), r))
    .collect()
});

static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub level: Option<Level>,
    pub timestamp: Option<String>,
    /// The line with the level and timestamp removed.
    pub message: String,
    /// The message with variable parts masked; equal signatures are grouped.
    pub signature: String,
}

pub fn parse_line(line: &str) -> Entry {
    if let Some(klog) = KLOG.captures(line) {
        // The level and time are in the prefix; a level word later in the line is just text.
        let level = match &klog[1] {
            "I" => Level::Info,
            "W" => Level::Warn,
            "E" => Level::Error,
            _ => Level::Fatal,
        };
        let message = tidy(&line[klog.get(0).unwrap().end()..]);
        let signature = normalize(&message);
        return Entry {
            level: Some(level),
            timestamp: Some(klog[2].to_string()),
            message,
            signature,
        };
    }

    let level_match = LEVEL.captures(line).and_then(|c| c.get(1));
    let level = level_match.and_then(|m| Level::parse(m.as_str()));
    let timestamp = TIMESTAMP.find(line).map(|m| m.as_str().to_string());

    let mut message = line.to_string();
    if let Some(m) = level_match {
        message.replace_range(m.range(), "");
    }
    if let Some(ts) = &timestamp {
        message = message.replacen(ts.as_str(), "", 1);
    }
    let message = tidy(&message);
    let signature = normalize(&message);
    Entry {
        level,
        timestamp,
        message,
        signature,
    }
}

pub fn normalize(message: &str) -> String {
    let mut out = message.to_string();
    for (re, replacement) in MASKS.iter() {
        out = re.replace_all(&out, *replacement).into_owned();
    }
    out = ALNUM
        .replace_all(&out, |c: &regex::Captures| {
            let token = &c[0];
            let digits = token.chars().filter(|ch| ch.is_ascii_digit()).count();
            let letters = token.chars().filter(|ch| ch.is_ascii_alphabetic()).count();
            if digits >= 2 && letters >= 2 {
                "<id>".to_string()
            } else {
                token.to_string()
            }
        })
        .into_owned();
    tidy(&out)
}

static EMPTY_BRACKETS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\s*\]|\(\s*\)").unwrap());
static REPEATED_COLONS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":(\s*:)+").unwrap());

/// Cleans up what removing the level and timestamp leaves behind: `[] `, `: :`, extra spaces and
/// separators at the ends. Brackets that still hold something (`[worker-3]`) are kept whole.
fn tidy(s: &str) -> String {
    let s = EMPTY_BRACKETS.replace_all(s, " ");
    let s = REPEATED_COLONS.replace_all(&s, ":");
    let collapsed = SPACES.replace_all(s.trim(), " ");
    collapsed
        .trim_matches(|c: char| c == ':' || c == '|' || c == '-' || c.is_whitespace())
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_level_formats() {
        let cases = [
            ("2026-01-01 10:00:00 ERROR db timeout", Level::Error),
            ("[WARN] disk almost full", Level::Warn),
            ("level=info msg=started", Level::Info),
            (r#"{"level":"fatal","msg":"boom"}"#, Level::Fatal),
            ("Jan 01 host app[42]: warning: retrying", Level::Warn),
            ("thread 'main' panicked at src/main.rs:3", Level::Fatal),
        ];
        for (line, expected) in cases {
            assert_eq!(parse_line(line).level, Some(expected), "{line}");
        }
    }

    #[test]
    fn no_level_for_plain_text() {
        assert_eq!(parse_line("just some output").level, None);
        // "error" inside a word is not a level
        assert_eq!(parse_line("terrorism_filter loaded").level, None);
    }

    #[test]
    fn extracts_timestamp_and_strips_it() {
        let e = parse_line("2026-03-04T05:06:07.123Z ERROR payment failed");
        assert_eq!(e.timestamp.as_deref(), Some("2026-03-04T05:06:07.123Z"));
        assert_eq!(e.message, "payment failed");
    }

    #[test]
    fn similar_lines_share_a_signature() {
        let a = parse_line("ERROR user 1042 failed login from 10.0.0.7 after 350ms");
        let b = parse_line("ERROR user 77 failed login from 192.168.1.20 after 12ms");
        assert_eq!(a.signature, b.signature);
        assert_eq!(a.signature, "user <n> failed login from <ip> after <n>");
    }

    #[test]
    fn reads_syslog_timestamps() {
        let a = parse_line(
            "Nov 15 10:00:05 web01 sshd[2211]: error: kex_exchange_identification: Connection closed",
        );
        let b = parse_line(
            "Nov  3 09:01:44 web02 sshd[99]: error: kex_exchange_identification: Connection closed",
        );
        assert_eq!(a.level, Some(Level::Error));
        assert_eq!(a.timestamp.as_deref(), Some("Nov 15 10:00:05"));
        assert_eq!(b.timestamp.as_deref(), Some("Nov  3 09:01:44"));
        assert_eq!(a.signature, b.signature);
        assert!(!a.signature.contains(": :"), "{}", a.signature);
        assert!(!a.signature.contains("Nov"), "{}", a.signature);
    }

    #[test]
    fn reads_klog_prefix_for_level_and_time() {
        let a = parse_line(
            "E1115 10:00:07.123456   12345 controller.go:114] error syncing pod default/api-7d9f8: timeout",
        );
        let b = parse_line(
            "E1115 10:00:08.654321   12345 controller.go:114] error syncing pod default/api-5c2a1: timeout",
        );
        assert_eq!(a.level, Some(Level::Error));
        assert_eq!(a.timestamp.as_deref(), Some("1115 10:00:07.123456"));
        assert_eq!(a.signature, b.signature);
        assert!(
            a.message
                .starts_with("12345 controller.go:114] error syncing"),
            "{}",
            a.message
        );

        let w = parse_line("W1115 10:00:09.000001 1 reflector.go:42] watch closed");
        assert_eq!(w.level, Some(Level::Warn));
        let i = parse_line("I1115 10:00:09.000001 1 main.go:7] started");
        assert_eq!(i.level, Some(Level::Info));
    }

    #[test]
    fn masks_mixed_letter_and_digit_ids() {
        assert_eq!(
            normalize("pod api-7d9f8 on node-a1b2c3 restarted"),
            normalize("pod api-5c2a1 on node-x9y8z7 restarted"),
        );
        // plain words and short tokens stay readable
        assert_eq!(normalize("utf8 decoder ready"), "utf8 decoder ready");
    }

    #[test]
    fn keeps_bracketed_context_intact() {
        let e = parse_line("2026-11-15 10:00:01,123 ERROR [worker-3] Task 8812 failed");
        assert_eq!(e.signature, "[worker-<n>] Task <n> failed");
        // an emptied level bracket still disappears
        assert_eq!(
            parse_line("[WARN] disk almost full").message,
            "disk almost full"
        );
        assert_eq!(
            parse_line("[2026-11-15T10:00:09Z] WARN  cache miss").message,
            "cache miss"
        );
    }

    #[test]
    fn masks_ids_hashes_and_strings() {
        let s = normalize(
            r#"order 3f2b8c1e-9a4d-4b6e-8f00-123456789abc for a@b.com key="secret" ptr 0xdeadbeef"#,
        );
        assert_eq!(s, r#"order <uuid> for <email> key="<str>" ptr <hex>"#);
    }
}
