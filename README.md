# log-analyzer

A web-server log parser written in Rust to learn data-parallel patterns with [`rayon`](https://docs.rs/rayon). Parses NCSA Common Log Format (Apache / nginx access logs), aggregates per-IP / per-path / per-status / hourly stats, and exposes the result through a small `axum` HTTP API and a React frontend. Optional AI summary endpoint calls Groq's free-tier Llama 3.3 70B in JSON mode to translate the stats into both a technical and a plain-English read.

> **What this is, honestly**: a portfolio piece. The interesting Rust is ~90 lines in [`analyzer-core/src/lib.rs`](crates/analyzer-core/src/lib.rs) — a single-pass parallel `fold` / `reduce` over the lines, a `OnceLock`-cached regex, and a commutative `Stats::merge` that lets the rollup run on any number of cores. The rest is integration glue.

## Throughput

End-to-end parse + parallel rollup, measured with the release CLI on a 5M-line synthetic NCSA log:

| Lines     | File size | Time  | Throughput |
| --------- | --------- | ----- | ---------- |
| 5,000,000 | 656 MB    | 3.57s | 184 MB/s   |

That's wall-clock time including file read, regex parse, parallel `fold` over 16 logical cores, and the final `Stats::merge`. Reproduce on your machine:

```bash
cargo build --release
./target/release/log-analyzer --gen 5000000 > samples/big.log
time ./target/release/log-analyzer samples/big.log
```

For controlled per-iteration numbers, run `cargo bench -p analyzer-core` — criterion's HTML reports land under `target/criterion/`.

Hardware for the table above: AMD Ryzen 7 7435HS · 8 cores / 16 threads · Windows 11.

## Architecture

```
                   ┌──────────────────────┐
                   │  React + Vite (5173) │  ← user uploads .log file
                   │  recharts dashboard  │
                   └──────────┬───────────┘
                              │  POST /api/analyze (multipart)
                              ▼
                   ┌──────────────────────┐
                   │   axum server (8080) │
                   │  • rate limiter      │
                   │  • CORS allow-list   │
                   │  • CSP, XFO, etc.    │
                   └──────────┬───────────┘
                              │
                              ▼
                   ┌──────────────────────┐         ┌─────────────────┐
                   │  analyzer-core       │         │  Groq API       │
                   │  • regex line parse  │   ┌────►│  Llama 3.3 70B  │
                   │  • rayon fold/reduce │   │     │  JSON mode      │
                   │  • Stats::merge      │   │     └─────────────────┘
                   └──────────┬───────────┘   │
                              │  Stats        │
                              ▼               │
                   ┌──────────────────────┐   │
                   │  /api/ai-summary     │───┘
                   │  • SHA-256 cache     │
                   │  • 30s timeout       │
                   │  • prompt sanitize   │
                   └──────────────────────┘
```

## Project layout

```
log-analyzer/
├── crates/
│   ├── analyzer-core/     ← the actual Rust showcase: parse + rayon rollup
│   ├── analyzer-cli/      ← `clap`-based CLI: cat any.log | analyzer-cli
│   └── analyzer-server/   ← axum HTTP wrapper around the core
├── web/                   ← Vite + React + recharts dashboard
├── samples/               ← bundled demo logs (default, heavy, attack, sparse)
├── benches/               ← criterion benchmarks
└── .env.example           ← copy to .env and fill in GROQ_API_KEY
```

## Run it locally

You need [Rust 1.75+](https://rustup.rs) and [Node 20+](https://nodejs.org).

```bash
# 1. clone
git clone https://github.com/<your-handle>/log-analyzer
cd log-analyzer

# 2. (optional) get a free Groq key for AI summaries — sign up at console.groq.com
cp .env.example .env
# edit .env, paste your key into GROQ_API_KEY=

# 3. backend (terminal 1)
cargo run --release -p analyzer-server
# → listening on http://127.0.0.1:8080

# 4. frontend (terminal 2)
cd web
npm install
npm run dev
# → http://localhost:5173
```

Drop any `*.log` in NCSA Common Log Format on the page, or click one of the bundled samples.

## CLI

```bash
cargo run --release -p analyzer-cli -- /path/to/access.log
```

Pipe stdin:

```bash
cat /var/log/nginx/access.log | cargo run --release -p analyzer-cli
```

## Endpoints

| Method | Path                | Notes                                                                |
| ------ | ------------------- | -------------------------------------------------------------------- |
| GET    | `/api/health`       | liveness                                                             |
| GET    | `/api/sample`       | `?name=default\|heavy\|attack\|sparse` — bundled demo stats          |
| POST   | `/api/analyze`      | multipart, field `log`, max 50 MB. Rate-limited 30 req/min/IP        |
| POST   | `/api/ai-summary`   | JSON `Stats` body. Cached. Rate-limited 5 req/min/IP. Needs Groq key |

All responses carry `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`, and a strict `Content-Security-Policy`. CORS reads from `CORS_ALLOWED_ORIGINS`.

## Error handling

- **Malformed log lines** are silently dropped from the rollup; `Stats.parsed_lines` vs `total_lines` lets you see the parse rate.
- **Missing `GROQ_API_KEY`** → `/api/ai-summary` returns `503 Service Unavailable` with a clear message.
- **Upstream Groq failure** → `502 Bad Gateway` with the Groq-side status code surfaced for debugging. Hard 30s timeout on the upstream call.
- **Oversized upload** → `413 Payload Too Large` (max 50 MB).
- **Rate limit hit** → `429 Too Many Requests` with `Retry-After: 60`.

## Security notes

- API key stays in `.env` (gitignored) — never in source.
- Token-bucket rate limiter, per real client IP. Behind a trusted proxy, set `TRUST_PROXY=1` to read `X-Forwarded-For`; otherwise the header is ignored (it's spoofable on direct binds).
- Path / IP fields from the log are stripped of control chars and truncated before they enter the LLM prompt — defends against prompt-injection payloads embedded in attacker-crafted log lines.
- CORS allow-list is explicit (no `*`) by default.

## Benchmarking

```bash
# generate a synthetic large log (writes samples/big.log, ~750 MB)
cargo run --release -p analyzer-cli -- --gen 5000000 > samples/big.log

# run the criterion bench
cargo bench -p analyzer-core
```

Criterion writes HTML reports under `target/criterion/`.

## What I'd do differently

- **Start with a parser combinator** (`nom` / `winnow`) instead of one regex — would teach more transferable Rust and handle malformed input more cleanly.
- **Stream from disk** via `BufReader::lines()` chunks rather than loading the whole file into a `Vec<String>`. The current shape can't process files larger than RAM.
- **Split this repo in two**: a Rust-only showcase (`analyzer-core` + `analyzer-cli` + criterion bench), and a separate full-stack demo. Recruiters scan one repo for one signal — bundling the React app dilutes the Rust focus.
- **Add a `flate2` decompression layer** so `.log.gz` works without manual gunzip.

## License

MIT — see [LICENSE](LICENSE).
