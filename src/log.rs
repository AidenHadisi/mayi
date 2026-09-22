//! Append-only JSONL decision log. Local only; never the action line.
//!
//! Path: `$XDG_STATE_HOME/mayi/decisions.jsonl` (default `~/.local/state/...`).
//! Writes are fail-open: a logging error must not change a hook verdict.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anstyle::{AnsiColor, Color, Style};
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::host::Agent;

/// One recorded gate decision. `verdict` is a string so unknown values stay countable.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub ts: u64,
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub error: bool,
}

impl Event {
    /// Append this event. Errors and panics are discarded.
    pub fn append(&self) {
        let _ = panic::catch_unwind(AssertUnwindSafe(|| {
            let Some(path) = path() else {
                return;
            };
            if let Some(dir) = path.parent()
                && !dir.exists()
            {
                if fs::create_dir_all(dir).is_err() {
                    return;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
                }
            }
            let mut opts = OpenOptions::new();
            opts.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let Ok(mut file) = opts.open(&path) else {
                return;
            };
            if serde_json::to_writer(&mut file, self).is_ok() {
                let _ = file.write_all(b"\n");
            }
        }));
    }
}

/// Totals over the local log, optionally filtered.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    pub checks: u64,
    pub allow: u64,
    /// Written by older releases; counted so old logs still add up.
    pub ask: u64,
    pub deny: u64,
    pub errors: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: Option<Latency>,
}

/// Nearest-rank percentiles of recorded classify wall time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latency {
    pub p50: u64,
    pub p95: u64,
}

impl Summary {
    /// Load and fold the log. Missing file is an empty summary. Malformed lines are skipped.
    pub fn load(agent: Option<Agent>, since: Option<&str>) -> Result<Self> {
        let cutoff = since.map(Self::parse_since).transpose()?.map(|dur| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_sub(dur.as_secs())
        });

        let Some(path) = path() else {
            return Ok(Self::default());
        };

        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("cannot read {}: {e}", path.display()).into()),
        };

        let agent = agent.map(|a| a.to_string());
        let events = BufReader::new(file)
            .lines()
            .filter_map(|line| serde_json::from_str::<Event>(&line.ok()?).ok())
            .filter(|event| cutoff.is_none_or(|min_ts| event.ts >= min_ts))
            .filter(|event| agent.as_ref().is_none_or(|want| event.agent == *want));

        let mut summary = Self::default();
        let mut latencies = Vec::new();
        for event in events {
            summary.checks += 1;
            match event.verdict.as_deref() {
                Some("allow") => summary.allow += 1,
                Some("ask") => summary.ask += 1,
                Some("deny") => summary.deny += 1,
                _ => {}
            }
            if event.error {
                summary.errors += 1;
            }
            summary.input_tokens += event.input_tokens.unwrap_or(0);
            summary.output_tokens += event.output_tokens.unwrap_or(0);
            if let Some(ms) = event.latency_ms {
                latencies.push(ms);
            }
        }

        if !latencies.is_empty() {
            latencies.sort_unstable();
            let last = latencies.len() - 1;
            let nearest = |pct: usize| latencies[last.saturating_mul(pct).saturating_add(50) / 100];
            summary.latency_ms = Some(Latency {
                p50: nearest(50),
                p95: nearest(95),
            });
        }
        Ok(summary)
    }

    /// Write the colored human summary to stdout.
    pub fn print(&self) {
        const ALLOW: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));
        const ASK: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow)));
        const DENY: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)));
        const DIM: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlack)));

        if self.checks == 0 {
            anstream::println!("{DIM}no decisions yet{DIM:#}");
            return;
        }

        anstream::println!("{} checks", self.checks);
        anstream::println!();
        anstream::println!("  {ALLOW}allow{ALLOW:#}  {:>5}", self.allow);
        anstream::println!("  {ASK}ask{ASK:#}    {:>5}", self.ask);
        if self.errors > 0 {
            anstream::println!(
                "  {DENY}deny{DENY:#}   {:>5}  {DIM}({} errors){DIM:#}",
                self.deny,
                self.errors
            );
        } else {
            anstream::println!("  {DENY}deny{DENY:#}   {:>5}", self.deny);
        }
        anstream::println!();
        anstream::println!(
            "  tokens   {} in / {} out",
            self.input_tokens,
            self.output_tokens
        );
        if let Some(lat) = &self.latency_ms {
            anstream::println!("  latency  p50 {}ms  p95 {}ms", lat.p50, lat.p95);
        }
    }

    /// Duration like `7d`, `12h`, or `30m` (case-insensitive).
    fn parse_since(spec: &str) -> Result<Duration> {
        let spec = spec.trim();
        let invalid =
            || format!("invalid --since {spec:?}; expected a number and d, h, or m (e.g. 7d)");
        if spec.len() < 2 {
            return Err(invalid().into());
        }

        let (num, unit) = spec.split_at(spec.len() - 1);
        let n: u64 = num.parse().map_err(|_| invalid())?;
        match unit {
            "d" | "D" => Ok(Duration::from_hours(n.saturating_mul(24))),
            "h" | "H" => Ok(Duration::from_hours(n)),
            "m" | "M" => Ok(Duration::from_mins(n)),
            _ => Err(invalid().into()),
        }
    }
}

/// `$XDG_STATE_HOME/mayi/decisions.jsonl`, else `~/.local/state/mayi/decisions.jsonl`.
fn path() -> Option<PathBuf> {
    let base = match env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => env::home_dir()?.join(".local/state"),
    };
    Some(base.join("mayi/decisions.jsonl"))
}

#[cfg(test)]
mod tests {
    use super::Summary;
    use std::time::Duration;

    #[test]
    fn parse_since_units() {
        assert_eq!(
            Summary::parse_since("7d").expect("7d"),
            Duration::from_hours(7 * 24)
        );
        assert_eq!(
            Summary::parse_since("24h").expect("24h"),
            Duration::from_hours(24)
        );
        assert_eq!(
            Summary::parse_since("30m").expect("30m"),
            Duration::from_mins(30)
        );
        assert_eq!(
            Summary::parse_since("1D").expect("1D"),
            Duration::from_hours(24)
        );
        assert!(Summary::parse_since("7").is_err());
        assert!(Summary::parse_since("7s").is_err());
        assert!(Summary::parse_since("").is_err());
    }
}
