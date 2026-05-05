use analyzer_core::{analyze, Stats};
use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "log-analyzer", about = "Parallel NCSA log file analyzer")]
struct Cli {
    /// Path to the log file. Omit to read from stdin, or use --gen.
    log_file: Option<PathBuf>,

    /// Show top N entries per category
    #[arg(long, default_value_t = 10)]
    top: usize,

    /// Generate N synthetic NCSA log lines to stdout, then exit.
    /// Useful for benchmarking: `analyzer --gen 5000000 > samples/big.log`
    #[arg(long)]
    gen: Option<usize>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(n) = cli.gen {
        return generate(n);
    }

    let lines: Vec<String> = match cli.log_file {
        Some(path) => {
            let file = File::open(&path)
                .with_context(|| format!("opening {}", path.display()))?;
            BufReader::new(file).lines().map_while(Result::ok).collect()
        }
        None => {
            let stdin = io::stdin();
            if stdin.is_terminal() {
                anyhow::bail!(
                    "no log file given and stdin is a terminal — \
                     pass a path, pipe a log into stdin, or use --gen N"
                );
            }
            stdin.lock().lines().map_while(Result::ok).collect()
        }
    };

    let stats = analyze(&lines);
    print_report(&stats, cli.top);
    Ok(())
}

/// Emit `n` synthetic NCSA Common Log Format lines to stdout. Deterministic — no
/// external rand crate; uses a simple LCG so benchmarks are reproducible.
fn generate(n: usize) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let paths = [
        "/", "/index.html", "/about", "/api/users", "/api/health", "/static/app.js",
        "/static/app.css", "/products", "/products/42", "/cart", "/checkout",
        "/login", "/admin", "/.env", "/wp-login.php", "/phpmyadmin", "/.git/config",
        "/blog", "/blog/post-1", "/blog/post-2", "/search?q=widgets", "/contact",
        "/favicon.ico", "/robots.txt", "/sitemap.xml", "/api/products/12",
    ];
    let methods = ["GET", "GET", "GET", "GET", "POST", "GET", "HEAD"];
    let statuses = [200u16, 200, 200, 200, 200, 304, 301, 404, 404, 500, 401, 403];
    let user_agents = [
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
        "curl/8.4.0",
        "Googlebot/2.1 (+http://www.google.com/bot.html)",
        "python-requests/2.31.0",
    ];

    let mut state: u64 = 0xdeadbeefcafebabe;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        state
    };

    let mut buf = String::with_capacity(256);
    for _ in 0..n {
        let ip = format!(
            "{}.{}.{}.{}",
            (next() % 224) + 1,
            next() % 256,
            next() % 256,
            next() % 256,
        );
        let day = (next() % 28) + 1;
        let hour = next() % 24;
        let min = next() % 60;
        let sec = next() % 60;
        let method = methods[(next() as usize) % methods.len()];
        let path = paths[(next() as usize) % paths.len()];
        let status = statuses[(next() as usize) % statuses.len()];
        let bytes = (next() % 50_000) + 100;
        let ua = user_agents[(next() as usize) % user_agents.len()];

        buf.clear();
        use std::fmt::Write;
        let _ = writeln!(
            buf,
            r#"{ip} - - [{day:02}/Jan/2026:{hour:02}:{min:02}:{sec:02} -0700] "{method} {path} HTTP/1.1" {status} {bytes} "-" "{ua}""#,
        );
        out.write_all(buf.as_bytes())?;
    }

    Ok(())
}

fn print_report(s: &Stats, top: usize) {
    println!("\n=== Log Analysis Report ===");
    println!("  Lines read:     {}", s.total_lines);
    println!(
        "  Lines parsed:   {} ({:.1}%)",
        s.parsed_lines,
        s.parsed_lines as f64 / s.total_lines.max(1) as f64 * 100.0
    );
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
