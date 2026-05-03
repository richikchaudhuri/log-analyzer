//! Parallel log analyzer for NCSA Common Log Format.
//! Streams lines, parses with regex, aggregates stats across rayon worker threads.

use anyhow::{Context, Result};
use clap::Parser;
use rayon::prelude::*;
use regex::Regex;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Parser, Debug)]
#[command(name = "log-analyzer", about = "Parallel NCSA log file analyzer")]
struct Cli {
    /// Path to log file
    log_file: PathBuf,

    /// Show top N entries per category
    #[arg(long, default_value_t = 10)]
    top: usize,
}

#[derive(Default, Clone)]
struct Stats {
    requests: u64,
    bytes: u64,
    by_ip: HashMap<String, u64>,
    by_status: HashMap<u16, u64>,
    by_path: HashMap<String, u64>,
    by_hour: HashMap<u8, u64>,
}

impl Stats {
    fn record(&mut self, ip: String, hour: u8, path: String, status: u16, bytes: u64) {
        self.requests += 1;
        self.bytes += bytes;
        *self.by_ip.entry(ip).or_insert(0) += 1;
        *self.by_status.entry(status).or_insert(0) += 1;
        *self.by_path.entry(path).or_insert(0) += 1;
        *self.by_hour.entry(hour).or_insert(0) += 1;
    }

    fn merge(mut self, other: Stats) -> Stats {
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

fn parse_line(line: &str) -> Option<(String, u8, String, u16, u64)> {
    let caps = line_re().captures(line)?;
    let ip = caps.get(1)?.as_str().to_string();
    let ts = caps.get(2)?.as_str();
    let path = caps.get(4)?.as_str().to_string();
    let status: u16 = caps.get(5)?.as_str().parse().ok()?;
    let bytes: u64 = caps.get(6)?.as_str().parse().unwrap_or(0);

    // ts looks like "10/Oct/2025:13:55:36 -0700" — hour is the first colon-separated piece after the date
    let hour: u8 = ts.split(':').nth(1).and_then(|s| s.parse().ok())?;

    Some((ip, hour, path, status, bytes))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let file = File::open(&cli.log_file)
        .with_context(|| format!("opening {}", cli.log_file.display()))?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();

    let total_lines = lines.len();
    let parsed: usize = lines.par_iter().filter(|l| parse_line(l).is_some()).count();

    let stats: Stats = lines
        .par_iter()
        .filter_map(|l| parse_line(l))
        .fold(
            Stats::default,
            |mut acc, (ip, hour, path, status, bytes)| {
                acc.record(ip, hour, path, status, bytes);
                acc
            },
        )
        .reduce(Stats::default, Stats::merge);

    print_report(&stats, cli.top, total_lines, parsed);
    Ok(())
}

fn print_report(s: &Stats, top: usize, total: usize, parsed: usize) {
    println!("\n=== Log Analysis Report ===");
    println!("  Lines read:     {}", total);
    println!("  Lines parsed:   {} ({:.1}%)", parsed, parsed as f64 / total.max(1) as f64 * 100.0);
    println!("  Total requests: {}", s.requests);
    println!(
        "  Bytes served:   {} ({:.2} MB)",
        s.bytes,
        s.bytes as f64 / 1_048_576.0
    );

    println!("\n=== Status Codes ===");
    let mut statuses: Vec<_> = s.by_status.iter().collect();
    statuses.sort_by_key(|(c, _)| **c);
    for (code, count) in statuses {
        let pct = *count as f64 / s.requests.max(1) as f64 * 100.0;
        println!("  {} : {:>6}  ({:5.1}%)", code, count, pct);
    }

    println!("\n=== Top {} IPs ===", top);
    print_top_n(&s.by_ip, top);

    println!("\n=== Top {} Paths ===", top);
    print_top_n(&s.by_path, top);

    println!("\n=== Hourly Distribution ===");
    let mut hours: Vec<_> = s.by_hour.iter().collect();
    hours.sort_by_key(|(h, _)| **h);
    let max = hours.iter().map(|(_, v)| **v).max().unwrap_or(1);
    for (h, count) in hours {
        let bar_len = (*count as f64 / max as f64 * 40.0) as usize;
        println!("  {:02}:00  {:>5}  {}", h, count, "#".repeat(bar_len));
    }
}

fn print_top_n<K: std::fmt::Display + Eq + std::hash::Hash>(map: &HashMap<K, u64>, n: usize) {
    let mut items: Vec<_> = map.iter().collect();
    items.sort_by(|a, b| b.1.cmp(a.1));
    for (k, c) in items.iter().take(n) {
        println!("  {:>6}  {}", c, k);
    }
}
