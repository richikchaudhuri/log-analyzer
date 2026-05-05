//! Core log analysis library.
//! Parses NCSA Common Log Format and aggregates per-IP, per-path, per-status, hourly stats.

use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Stats {
    pub total_lines: u64,
    pub parsed_lines: u64,
    pub requests: u64,
    pub bytes: u64,
    pub by_ip: HashMap<String, u64>,
    pub by_status: HashMap<u16, u64>,
    pub by_path: HashMap<String, u64>,
    pub by_hour: HashMap<u8, u64>,
}

impl Stats {
    pub fn record(&mut self, ip: String, hour: u8, path: String, status: u16, bytes: u64) {
        self.requests += 1;
        self.bytes += bytes;
        *self.by_ip.entry(ip).or_insert(0) += 1;
        *self.by_status.entry(status).or_insert(0) += 1;
        *self.by_path.entry(path).or_insert(0) += 1;
        *self.by_hour.entry(hour).or_insert(0) += 1;
    }

    pub fn merge(mut self, other: Stats) -> Stats {
        self.total_lines += other.total_lines;
        self.parsed_lines += other.parsed_lines;
        self.requests += other.requests;
        self.bytes += other.bytes;
        for (k, v) in other.by_ip {
            *self.by_ip.entry(k).or_insert(0) += v;
        }
        for (k, v) in other.by_status {
            *self.by_status.entry(k).or_insert(0) += v;
        }
        for (k, v) in other.by_path {
            *self.by_path.entry(k).or_insert(0) += v;
        }
        for (k, v) in other.by_hour {
            *self.by_hour.entry(k).or_insert(0) += v;
        }
        self
    }
}

static LINE_RE: OnceLock<Regex> = OnceLock::new();

fn line_re() -> &'static Regex {
    LINE_RE.get_or_init(|| {
        Regex::new(r#"^(\S+) \S+ \S+ \[([^\]]+)\] "(\S+) (\S+) [^"]*" (\d{3}) (\S+)"#).unwrap()
    })
}

pub fn parse_line(line: &str) -> Option<(String, u8, String, u16, u64)> {
    let caps = line_re().captures(line)?;
    let ip = caps.get(1)?.as_str().to_string();
    let ts = caps.get(2)?.as_str();
    let path = caps.get(4)?.as_str().to_string();
    let status: u16 = caps.get(5)?.as_str().parse().ok()?;
    let bytes: u64 = caps.get(6)?.as_str().parse().unwrap_or(0);
    let hour: u8 = ts.split(':').nth(1).and_then(|s| s.parse().ok())?;
    Some((ip, hour, path, status, bytes))
}

/// Analyze a slice of log lines in parallel using rayon.
///
/// Single pass: every line is parsed at most once. `parsed_lines` equals
/// the count of records produced by `record()` (i.e. `stats.requests`),
/// so we don't need a separate filter pass to count parses.
pub fn analyze<S: AsRef<str> + Sync>(lines: &[S]) -> Stats {
    let total = lines.len() as u64;

    let mut stats: Stats = lines
        .par_iter()
        .filter_map(|l| parse_line(l.as_ref()))
        .fold(
            Stats::default,
            |mut acc, (ip, hour, path, status, bytes)| {
                acc.record(ip, hour, path, status, bytes);
                acc
            },
        )
        .reduce(Stats::default, Stats::merge);

    stats.total_lines = total;
    stats.parsed_lines = stats.requests;
    stats
}

/// Convenience: analyze a string of newline-separated log lines.
pub fn analyze_str(input: &str) -> Stats {
    let lines: Vec<&str> = input.lines().collect();
    analyze(&lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_basic_line() {
        let line = r#"10.0.0.1 - - [03/May/2026:14:22:10 -0700] "GET /index.html HTTP/1.1" 200 4567"#;
        let (ip, hour, path, status, bytes) = parse_line(line).unwrap();
        assert_eq!(ip, "10.0.0.1");
        assert_eq!(hour, 14);
        assert_eq!(path, "/index.html");
        assert_eq!(status, 200);
        assert_eq!(bytes, 4567);
    }

    #[test]
    fn analyze_rolls_up() {
        let log = r#"10.0.0.1 - - [03/May/2026:14:00:00 -0700] "GET /a HTTP/1.1" 200 100
10.0.0.2 - - [03/May/2026:14:00:01 -0700] "GET /a HTTP/1.1" 404 50
10.0.0.1 - - [03/May/2026:15:00:02 -0700] "GET /b HTTP/1.1" 200 200"#;
        let s = analyze_str(log);
        assert_eq!(s.total_lines, 3);
        assert_eq!(s.parsed_lines, 3);
        assert_eq!(s.requests, 3);
        assert_eq!(s.bytes, 350);
        assert_eq!(s.by_ip.get("10.0.0.1"), Some(&2));
        assert_eq!(s.by_status.get(&200), Some(&2));
        assert_eq!(s.by_status.get(&404), Some(&1));
        assert_eq!(s.by_path.get("/a"), Some(&2));
        assert_eq!(s.by_hour.get(&14), Some(&2));
    }
}
