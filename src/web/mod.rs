//! Embedded web dashboard for terse.
//!
//! Provides a lightweight HTTP server (sync, via `tiny_http`) that serves:
//! - A single-page analytics dashboard with configuration editor
//! - JSON API endpoints for stats, trends, discovery, and config management
//!
//! Launched via `terse web` (default: `http://127.0.0.1:9746`).

mod api;
mod frontend;

use std::io::Cursor;

use anyhow::{Context, Result};
use tiny_http::{Header, Method, Response, Server, StatusCode};

// ---------------------------------------------------------------------------
// Server entry point
// ---------------------------------------------------------------------------

/// Start the web dashboard server on the given address.
///
/// Blocks the current thread. Handles requests sequentially (sufficient for
/// a local single-user dashboard). Gracefully handles errors per-request
/// without crashing the server.
pub fn serve(addr: &str) -> Result<()> {
    let server = Server::http(addr)
        .map_err(|e| anyhow::anyhow!("failed to start HTTP server on {addr}: {e}"))?;

    println!("terse dashboard running at http://{addr}");
    println!("Press Ctrl+C to stop.\n");

    // Try to open in default browser (best-effort)
    let url = format!("http://{addr}");
    let _ = open_browser(&url);

    for mut request in server.incoming_requests() {
        let method = request.method().clone();
        let url = request.url().to_string();

        // Read body up-front for methods that carry one
        let body = if matches!(method, Method::Put | Method::Post | Method::Patch) {
            let mut buf = String::new();
            let _ = request.as_reader().read_to_string(&mut buf);
            Some(buf)
        } else {
            None
        };

        let result = dispatch(&method, &url, body.as_deref());

        match result {
            Ok(resp) => {
                let _ = request.respond(resp);
            }
            Err(e) => {
                let body = serde_json::json!({ "error": e.to_string() }).to_string();
                let resp = Response::from_data(body.as_bytes().to_vec())
                    .with_header(content_type_json())
                    .with_status_code(StatusCode(500));
                let _ = request.respond(resp);
            }
        }

        // Brief access log
        println!(
            "{} {} {}",
            method,
            url,
            chrono::Local::now().format("%H:%M:%S")
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Dispatch an incoming request to the appropriate handler.
fn dispatch(method: &Method, url: &str, body: Option<&str>) -> Result<Response<Cursor<Vec<u8>>>> {
    // Strip query string for path matching
    let path = url.split('?').next().unwrap_or(url);

    match (method, path) {
        // Frontend
        (&Method::Get, "/") | (&Method::Get, "/index.html") => Ok(serve_frontend()),

        // API — Analytics
        (&Method::Get, "/api/stats") => api::get_stats(url),
        (&Method::Get, "/api/trends") => api::get_trends(url),
        (&Method::Get, "/api/discover") => api::get_discover(url),

        // API — Configuration
        (&Method::Get, "/api/config") => api::get_config(),
        (&Method::Put, "/api/config") => {
            let body = body.unwrap_or("{}");
            api::put_config(body)
        }
        (&Method::Post, "/api/config/reset") => api::post_config_reset(),

        // API — Health
        (&Method::Get, "/api/health") => api::get_health(),

        // 404
        _ => Ok(not_found()),
    }
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

/// Serve the embedded single-page frontend.
fn serve_frontend() -> Response<Cursor<Vec<u8>>> {
    let html = frontend::INDEX_HTML;
    Response::from_data(html.as_bytes().to_vec())
        .with_header(content_type_html())
        .with_status_code(StatusCode(200))
}

/// 404 response.
fn not_found() -> Response<Cursor<Vec<u8>>> {
    let body = r#"{"error": "not found"}"#;
    Response::from_data(body.as_bytes().to_vec())
        .with_header(content_type_json())
        .with_status_code(StatusCode(404))
}

/// JSON content type header.
pub(crate) fn content_type_json() -> Header {
    Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap()
}

/// HTML content type header.
fn content_type_html() -> Header {
    Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap()
}

/// Attempt to open a URL in the system default browser.
fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .spawn()
            .context("failed to open browser")?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .context("failed to open browser")?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("failed to open browser")?;
    }

    Ok(())
}
