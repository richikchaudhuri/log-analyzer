//! End-to-end integration test for the HTTP server.
//!
//! Boots the server binary on a random port, hits /api/health and /api/analyze,
//! verifies status, security headers, and that the parsed Stats round-trip
//! through multipart upload + JSON response.

use std::{
    net::TcpListener,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct ServerProcess {
    child: Child,
    base: String,
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_server() -> ServerProcess {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let bind = format!("127.0.0.1:{port}");

    // Per-test cache dir so the disk layer doesn't leak between runs.
    let cache_dir = std::env::temp_dir().join(format!("log-analyzer-test-{port}"));
    let _ = std::fs::remove_dir_all(&cache_dir);

    let bin = env!("CARGO_BIN_EXE_log-analyzer-server");
    let child = Command::new(bin)
        .env("BIND_ADDR", &bind)
        .env("CORS_ALLOWED_ORIGINS", "*")
        .env("CACHE_DIR", cache_dir)
        // Set to empty so dotenvy's no-overwrite semantics keep it empty
        // even if .env on disk has a real key. The server treats empty == unset.
        .env("GROQ_API_KEY", "")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn server binary");

    // Wrap the child immediately so the Drop impl kills + waits on it
    // even if the readiness loop times out and we panic below.
    let server = ServerProcess {
        child,
        base: format!("http://{bind}"),
    };

    let deadline = Instant::now() + Duration::from_secs(15);
    let client = reqwest::blocking::Client::new();
    while Instant::now() < deadline {
        if client
            .get(format!("{}/api/health", server.base))
            .timeout(Duration::from_millis(250))
            .send()
            .ok()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return server;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    panic!("server did not become ready within 15s");
}

#[test]
fn health_returns_ok_with_security_headers() {
    let server = start_server();
    let resp = reqwest::blocking::get(format!("{}/api/health", server.base)).unwrap();
    assert!(resp.status().is_success());

    let h = resp.headers();
    assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff");
    assert_eq!(h.get("x-frame-options").unwrap(), "DENY");
    assert_eq!(h.get("referrer-policy").unwrap(), "no-referrer");
    assert!(h.get("content-security-policy").is_some());

    let body = resp.text().unwrap();
    assert_eq!(body, "ok");
}

#[test]
fn analyze_round_trips_log_through_multipart() {
    let server = start_server();

    let log = "\
10.0.0.1 - - [03/May/2026:14:22:10 -0700] \"GET /index.html HTTP/1.1\" 200 4567\n\
10.0.0.2 - - [03/May/2026:14:22:11 -0700] \"GET /admin HTTP/1.1\" 401 0\n\
10.0.0.1 - - [03/May/2026:15:00:00 -0700] \"POST /api/users HTTP/1.1\" 201 88\n";

    let form = reqwest::blocking::multipart::Form::new()
        .text("log", log.to_string());

    let resp = reqwest::blocking::Client::new()
        .post(format!("{}/api/analyze", server.base))
        .multipart(form)
        .send()
        .unwrap();
    assert!(resp.status().is_success(), "status: {}", resp.status());

    let body: serde_json::Value = resp.json().unwrap();
    let stats = &body["stats"];
    assert_eq!(stats["total_lines"], 3);
    assert_eq!(stats["parsed_lines"], 3);
    assert_eq!(stats["requests"], 3);
    assert_eq!(stats["by_status"]["200"], 1);
    assert_eq!(stats["by_status"]["401"], 1);
    assert_eq!(stats["by_status"]["201"], 1);
    assert_eq!(stats["by_ip"]["10.0.0.1"], 2);
}

#[test]
fn ai_summary_returns_503_when_key_missing() {
    let server = start_server();

    let stats = serde_json::json!({
        "total_lines": 0, "parsed_lines": 0, "requests": 0, "bytes": 0,
        "by_ip": {}, "by_status": {}, "by_path": {}, "by_hour": {}
    });

    let resp = reqwest::blocking::Client::new()
        .post(format!("{}/api/ai-summary", server.base))
        .json(&stats)
        .send()
        .unwrap();

    assert_eq!(resp.status().as_u16(), 503);
}
