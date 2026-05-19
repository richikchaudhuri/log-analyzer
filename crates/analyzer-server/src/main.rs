//! HTTP API for the log analyzer.
//!
//! Endpoints:
//!   GET  /api/health             -> "ok"
//!   GET  /api/sample[?name=...]  -> stats for a bundled sample (default | heavy | attack | sparse)
//!   POST /api/analyze            -> multipart upload, field "log" -> stats JSON  (rate-limited per IP)
//!   POST /api/ai-summary         -> Groq-generated summary, JSON-cached         (rate-limited per IP)
//!
//! Configuration (read from environment, optionally via .env at startup):
//!   GROQ_API_KEY            required for /api/ai-summary
//!   CORS_ALLOWED_ORIGINS    comma-separated, default: http://localhost:5173,http://127.0.0.1:5173
//!   AI_RATE_PER_MIN         default: 5
//!   ANALYZE_RATE_PER_MIN    default: 30
//!   BIND_ADDR               default: 127.0.0.1:8080

mod ai;
mod cache;
mod ratelimit;

use analyzer_core::{analyze_str, Stats};
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Multipart, Query, State},
    http::{header, HeaderName, HeaderValue, Method, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use cache::SummaryCache;
use ratelimit::RateLimiter;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, time::Duration};
use tokio::net::TcpListener;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    set_header::SetResponseHeaderLayer,
    timeout::TimeoutLayer,
};

const SAMPLE_DEFAULT: &str = include_str!("../../../sample.log");
const SAMPLE_HEAVY:   &str = include_str!("../../../samples/heavy.log");
const SAMPLE_ATTACK:  &str = include_str!("../../../samples/attack.log");
const SAMPLE_SPARSE:  &str = include_str!("../../../samples/sparse.log");

const MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct AppState {
    cache: SummaryCache,
}

#[derive(Serialize)]
struct AnalyzeResponse {
    stats: Stats,
    source: String,
}

#[derive(Deserialize)]
struct SampleQuery {
    name: Option<String>,
}

#[tokio::main]
async fn main() {
    load_env();

    // BIND_ADDR wins if set explicitly. Otherwise honour Render/Fly/Heroku
    // style $PORT (binding 0.0.0.0) so the same binary runs on a managed
    // host with no config. Local default stays 127.0.0.1:8080.
    let bind_addr = std::env::var("BIND_ADDR").ok()
        .or_else(|| std::env::var("PORT").ok().map(|p| format!("0.0.0.0:{p}")))
        .unwrap_or_else(|| "127.0.0.1:8080".into());

    // STATIC_DIR points at a directory of built frontend assets (e.g.
    // web/dist from Vite). If set and exists, the router will serve those
    // files for any non-/api/* path, with SPA index.html fallback.
    let static_dir: Option<String> = std::env::var("STATIC_DIR")
        .ok()
        .filter(|d| !d.is_empty() && std::path::Path::new(d).is_dir());
    let ai_rate = parse_rate("AI_RATE_PER_MIN", 5);
    let analyze_rate = parse_rate("ANALYZE_RATE_PER_MIN", 30);
    let trust_proxy = std::env::var("TRUST_PROXY").ok().as_deref() == Some("1");
    let cors = build_cors();

    let state = AppState {
        cache: SummaryCache::open(),
    };

    let ai_limiter = RateLimiter::new(ai_rate, trust_proxy);
    let analyze_limiter = RateLimiter::new(analyze_rate, trust_proxy);

    let ai_routes = Router::new()
        .route("/api/ai-summary", post(ai_summary_handler))
        .layer(axum::middleware::from_fn_with_state(
            ai_limiter,
            ratelimit::middleware,
        ));

    let analyze_routes = Router::new()
        .route("/api/analyze", post(analyze_handler))
        .layer(axum::middleware::from_fn_with_state(
            analyze_limiter,
            ratelimit::middleware,
        ))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES));

    let mut app = Router::new()
        .route("/api/health", get(health))
        .route("/api/sample", get(sample_handler))
        .merge(ai_routes)
        .merge(analyze_routes)
        .with_state(state);

    // If a frontend build is available, serve it as the fallback for any
    // non-/api/* request. SPA routing (e.g. /about) falls back to index.html.
    if let Some(dir) = static_dir.as_deref() {
        use tower_http::services::{ServeDir, ServeFile};
        let serve = ServeDir::new(dir)
            .not_found_service(ServeFile::new(format!("{dir}/index.html")));
        app = app.fallback_service(serve);
    }

    // CSP differs depending on whether we're serving the SPA or running
    // as an API-only server. The strict `default-src 'none'` would block
    // every Vite-built script and style; loosen it just enough when the
    // frontend is bundled, keep the lockdown when this is API-only.
    let csp = if static_dir.is_some() {
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'"
    } else {
        "default-src 'none'; frame-ancestors 'none'"
    };

    let app = app
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_str(csp).expect("csp header value"),
        ))
        .layer(cors);

    let addr: SocketAddr = bind_addr.parse().expect("BIND_ADDR must be host:port");
    let listener = TcpListener::bind(addr).await.expect("bind");
    println!("log-analyzer-server listening on http://{}", addr);
    if let Some(dir) = static_dir.as_deref() {
        println!("  static frontend → {dir}");
    } else {
        println!("  (no STATIC_DIR — running in API-only mode)");
    }
    println!("  GET  /api/health");
    println!("  GET  /api/sample[?name=default|heavy|attack|sparse]");
    println!("  POST /api/analyze     (multipart, field=log; {analyze_rate} req/min/IP)");
    println!("  POST /api/ai-summary  (JSON Stats body; {ai_rate} req/min/IP; needs GROQ_API_KEY)");
    if trust_proxy {
        println!("  TRUST_PROXY=1 — using X-Forwarded-For for client IP (set this only behind a trusted proxy)");
    }
    if std::env::var("GROQ_API_KEY").ok().filter(|k| !k.trim().is_empty()).is_none() {
        println!();
        println!("  WARNING: GROQ_API_KEY not set — /api/ai-summary will return 503.");
        println!("  Put your key in .env (gitignored). Get one free at console.groq.com.");
    }

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

/// Try several .env locations so the server works whether you run it from
/// the workspace root, the log-analyzer directory, or via `cargo --manifest-path`.
fn load_env() {
    for candidate in [".env", "log-analyzer/.env", "../.env"] {
        if dotenvy::from_filename(candidate).is_ok() {
            return;
        }
    }
}

fn parse_rate(var: &str, default: u32) -> u32 {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n: &u32| n > 0)
        .unwrap_or(default)
}

fn build_cors() -> CorsLayer {
    let raw = std::env::var("CORS_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:5173,http://127.0.0.1:5173".into());

    let base = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE]);

    if raw.trim() == "*" {
        return base.allow_origin(AllowOrigin::any());
    }

    let origins: Vec<HeaderValue> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| HeaderValue::from_str(s).ok())
        .collect();

    base.allow_origin(AllowOrigin::list(origins))
}

async fn ai_summary_handler(
    State(state): State<AppState>,
    Json(stats): Json<Stats>,
) -> Result<Json<ai::AiSummary>, (StatusCode, String)> {
    let key = SummaryCache::key_for(&stats, ai::MODEL);

    if let Some(mut hit) = state.cache.get(&key) {
        hit.cached = true;
        return Ok(Json(hit));
    }

    match ai::summarize(&stats).await {
        Ok(summary) => {
            state.cache.put(&key, &summary);
            Ok(Json(summary))
        }
        Err(e @ ai::AiError::NoApiKey) => Err((StatusCode::SERVICE_UNAVAILABLE, e.to_string())),
        Err(e @ ai::AiError::Network(_)) => Err((StatusCode::BAD_GATEWAY, e.to_string())),
        Err(ai::AiError::Api { status, body }) => Err((
            StatusCode::BAD_GATEWAY,
            format!("Groq API {status}: {body}"),
        )),
        Err(e @ ai::AiError::Parse(_)) => Err((StatusCode::BAD_GATEWAY, e.to_string())),
    }
}

async fn health() -> &'static str {
    "ok"
}

async fn sample_handler(Query(q): Query<SampleQuery>) -> Json<AnalyzeResponse> {
    let key = q.name.as_deref().unwrap_or("default");
    let (text, source): (&'static str, &'static str) = match key {
        "heavy"  => (SAMPLE_HEAVY,  "heavy.log · 5,000 lines · varied traffic"),
        "attack" => (SAMPLE_ATTACK, "attack.log · suspicious activity pattern"),
        "sparse" => (SAMPLE_SPARSE, "sparse.log · low-traffic profile"),
        _        => (SAMPLE_DEFAULT, "sample.log · 500 lines · balanced demo"),
    };

    let stats = tokio::task::spawn_blocking(move || analyze_str(text))
        .await
        .expect("spawn_blocking");

    Json(AnalyzeResponse {
        stats,
        source: source.to_string(),
    })
}

async fn analyze_handler(
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    mut multipart: Multipart,
) -> Result<Json<AnalyzeResponse>, (StatusCode, String)> {
    let mut content: Option<String> = None;
    let mut filename = String::from("uploaded");

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("multipart error: {e}")))?
    {
        if field.name() == Some("log") {
            if let Some(name) = field.file_name() {
                filename = sanitize_filename(name);
            }
            let bytes = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("read field: {e}")))?;
            content = Some(String::from_utf8_lossy(&bytes).into_owned());
            break;
        }
    }

    let content = content.ok_or((
        StatusCode::BAD_REQUEST,
        "missing 'log' field in multipart upload".to_string(),
    ))?;

    let stats = tokio::task::spawn_blocking(move || analyze_str(&content))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {e}")))?;

    Ok(Json(AnalyzeResponse {
        stats,
        source: filename,
    }))
}

/// Strip path separators and control chars from user-supplied filenames
/// so they can never escape into a path or break terminal output when echoed.
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
        .take(128)
        .collect();
    if cleaned.is_empty() {
        "uploaded".into()
    } else {
        cleaned
    }
}
