//! Local HTTP fixture server for the driller `example/` plans.
//!
//! A small [axum](https://github.com/tokio-rs/axum) reimplementation of the
//! former Node/Express `server.js`. It serves the same routes and the same
//! static fixtures under `responses/`, so every `example/*.yml` plan runs
//! against it unchanged -- which lets CI exercise the examples without a Node
//! toolchain.
//!
//! Routes (parity with the old `server.js`):
//! - `GET /` -> `{"status":":D"}`
//! - `DELETE /` -> reset the session counter, `{"counter":1}`
//! - `GET /login?user=example&password=3x4mpl3` -> set a session cookie, `"Welcome!"` (else `403`)
//! - `GET /counter` -> increment and return the per-session counter (else `403`)
//! - `POST /api/users` -> occasional `500` (~1/51) to exercise failure counting
//! - `POST /api/transactions` -> `{"status":":D"}` when `a + b` equals `"123"` (JS `+` semantics)
//! - any other `GET` -> serve `responses/<path>` (404 when the file is missing), after an optional delay

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use clap::Parser;
use serde_json::Value;

/// Command-line / environment configuration.
#[derive(Parser)]
#[command(about = "HTTP fixture server for the driller example plans")]
struct Args {
  /// Port to listen on (the example plans target 9000).
  #[arg(long, default_value_t = 9000)]
  port: u16,

  /// Artificial per-response delay for the static fixture routes, in
  /// milliseconds. Mirrors the old server's `DELAY_MS`.
  #[arg(long, env = "DELAY_MS", default_value_t = 0)]
  delay_ms: u64,

  /// Directory holding the static fixture files served under `/api/...`.
  #[arg(long, default_value = "responses")]
  responses_dir: PathBuf,
}

/// Shared application state.
#[derive(Clone)]
struct AppState {
  responses_dir: PathBuf,
  delay: Duration,
  /// Per-session counters, keyed by the opaque `sid` cookie value.
  sessions: Arc<Mutex<HashMap<String, i64>>>,
  /// Monotonic source of unique session ids (no `rand` dependency).
  seq: Arc<AtomicU64>,
  /// Request counter driving the deterministic ~1/51 failure rate of
  /// `POST /api/users` (matches the old `Math.round(random*50) == 20`).
  hits: Arc<AtomicU64>,
}

#[tokio::main]
async fn main() {
  let args = Args::parse();
  let state = AppState {
    responses_dir: args.responses_dir,
    delay: Duration::from_millis(args.delay_ms),
    sessions: Arc::new(Mutex::new(HashMap::new())),
    seq: Arc::new(AtomicU64::new(0)),
    hits: Arc::new(AtomicU64::new(0)),
  };

  let app = Router::new().route("/", get(root).delete(reset)).route("/login", get(login)).route("/counter", get(counter)).route("/api/users", post(random_users)).route("/api/transactions", post(transactions)).fallback(static_file).with_state(state);

  let listener = tokio::net::TcpListener::bind(("0.0.0.0", args.port)).await.expect("bind");
  println!("Listening on port {}...", args.port);
  axum::serve(listener, app).await.expect("serve");
}

/// `GET /` -- liveness ping used by several plans.
async fn root() -> Response {
  json(StatusCode::OK, r#"{"status":":D"}"#)
}

/// `DELETE /` -- reset the caller's session counter to 1.
///
/// Matches the old server, which set `req.session.counter = 1` (creating a
/// session if none existed). A fresh session id is minted and returned when
/// the request carries no `sid` cookie.
async fn reset(State(state): State<AppState>, headers: HeaderMap) -> Response {
  let sid = cookie_sid(&headers).unwrap_or_else(|| new_sid(&state));
  state.sessions.lock().unwrap().insert(sid.clone(), 1);
  with_session_cookie(&sid, json(StatusCode::OK, r#"{"counter":1}"#))
}

/// `GET /login` -- authenticate and start a session.
async fn login(State(state): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
  let ok = q.get("user").map(String::as_str) == Some("example") && q.get("password").map(String::as_str) == Some("3x4mpl3");
  if ok {
    let sid = new_sid(&state);
    state.sessions.lock().unwrap().insert(sid.clone(), 1);
    with_session_cookie(&sid, "Welcome!".into_response())
  } else {
    (StatusCode::FORBIDDEN, "Forbidden").into_response()
  }
}

/// `GET /counter` -- increment and return the per-session counter.
async fn counter(State(state): State<AppState>, headers: HeaderMap) -> Response {
  if let Some(sid) = cookie_sid(&headers) {
    let mut sessions = state.sessions.lock().unwrap();
    if let Some(value) = sessions.get_mut(&sid) {
      *value += 1;
      let value = *value;
      return json(StatusCode::OK, &format!(r#"{{"counter":{value}}}"#));
    }
  }
  (StatusCode::FORBIDDEN, "Forbidden").into_response()
}

/// `POST /api/users` -- succeed most of the time, fail ~1/51 of requests.
///
/// Deterministic (counter-based) rather than random so CI output is stable,
/// while still exercising driller's failed-request accounting.
async fn random_users(State(state): State<AppState>) -> Response {
  if state.hits.fetch_add(1, Ordering::Relaxed) % 51 == 20 {
    json(StatusCode::INTERNAL_SERVER_ERROR, r#"{"status":":/"}"#)
  } else {
    json(StatusCode::OK, r#"{"status":":D"}"#)
  }
}

/// `POST /api/transactions` -- echo success when `a + b == "123"`.
///
/// Reproduces the old server's JavaScript `body.a + body.b === '123'`: numeric
/// operands are added, otherwise the operands are string-concatenated.
async fn transactions(body: Bytes) -> Response {
  let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
  let a = parsed.get("a").unwrap_or(&Value::Null);
  let b = parsed.get("b").unwrap_or(&Value::Null);
  if js_plus(a, b) == "123" {
    json(StatusCode::OK, r#"{"status":":D"}"#)
  } else {
    json(StatusCode::INTERNAL_SERVER_ERROR, r#"{"status":":/"}"#)
  }
}

/// Fallback handler: serve `responses/<request-path>` as a static file,
/// returning 404 when the file is absent. Honors the configured delay, like
/// the old server's `logger_handler`.
async fn static_file(State(state): State<AppState>, uri: Uri) -> Response {
  if !state.delay.is_zero() {
    tokio::time::sleep(state.delay).await;
  }

  let rel = uri.path().trim_start_matches('/');
  // Refuse path traversal; fixture paths are simple `api/...` segments.
  if rel.split('/').any(|seg| seg == "..") {
    return StatusCode::NOT_FOUND.into_response();
  }

  match tokio::fs::read(state.responses_dir.join(rel)).await {
    Ok(bytes) => ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
    Err(_) => StatusCode::NOT_FOUND.into_response(),
  }
}

/// Build a JSON response with the given status and body.
fn json(status: StatusCode, body: &str) -> Response {
  (status, [(header::CONTENT_TYPE, "application/json")], body.to_owned()).into_response()
}

/// Mint a new opaque session id.
fn new_sid(state: &AppState) -> String {
  format!("s{}", state.seq.fetch_add(1, Ordering::Relaxed))
}

/// Attach a `Set-Cookie: sid=...` header to a response.
fn with_session_cookie(sid: &str, mut response: Response) -> Response {
  if let Ok(value) = HeaderValue::from_str(&format!("sid={sid}; Path=/")) {
    response.headers_mut().insert(header::SET_COOKIE, value);
  }
  response
}

/// Extract the `sid` value from the request's `Cookie` header, if present.
///
/// The value may arrive double-quoted -- RFC 6265 permits the quoted-string
/// form, and reqwest's cookie store (used by driller) re-sends it as
/// `sid="..."` -- so surrounding quotes are trimmed.
fn cookie_sid(headers: &HeaderMap) -> Option<String> {
  let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
  cookies.split(';').map(str::trim).find_map(|pair| pair.strip_prefix("sid=").map(|v| v.trim_matches('"').to_owned()))
}

/// Render `a + b` the way JavaScript's `+` operator would for the value types
/// the example uses: add when both are numbers, otherwise string-concatenate.
fn js_plus(a: &Value, b: &Value) -> String {
  if a.is_number() && b.is_number() {
    let sum = a.as_f64().unwrap_or(0.0) + b.as_f64().unwrap_or(0.0);
    if sum.fract() == 0.0 {
      format!("{}", sum as i64)
    } else {
      format!("{sum}")
    }
  } else {
    format!("{}{}", js_str(a), js_str(b))
  }
}

/// Render a single JSON value as JavaScript's `String()` would.
fn js_str(value: &Value) -> String {
  match value {
    Value::String(s) => s.clone(),
    Value::Null => "null".to_string(),
    other => other.to_string(),
  }
}
