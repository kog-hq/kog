//! The `view` command's HTTP layer: the page embedded from `app/dist`,
//! plus a `/graph.json` route serving whatever was just scanned.
//!
//! Routing is a pure function (`route`) precisely so it can be tested
//! without opening a socket — see the tests below and the design brief
//! this module implements.

use rust_embed::RustEmbed;

/// `app/dist`, embedded at compile time. `build.rs` fails the build with a
/// pointed message if this directory doesn't exist yet, so by the time this
/// macro runs, the folder is guaranteed to be there.
#[derive(RustEmbed)]
#[folder = "../../app/dist"]
pub struct Asset;

/// A routing decision, deliberately plain data rather than a
/// `tiny_http::Response` — the latter is generic over its reader and awkward
/// to assert on, whereas this can be compared directly in tests.
pub struct RouteResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// Resolve one request path to a response body.
///
/// `/graph.json` is intercepted *before* the embedded assets are even
/// consulted: `app/dist` is produced by `vite build`, which copies
/// `app/public/*` verbatim, and a stray `graph.json` left over in `public/`
/// from a previous manual scan would otherwise get embedded right alongside
/// `index.html`. The graph this server reports must always be the one just
/// scanned in memory for the current `view` invocation, never a stale
/// embedded copy.
pub fn route(path: &str, graph_json: &str) -> RouteResponse {
    // Requests can carry a query string (e.g. cache-busting); routing only
    // cares about the path.
    let path = path.split('?').next().unwrap_or(path);
    let asset_path = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    if asset_path == "graph.json" {
        return RouteResponse {
            status: 200,
            content_type: "application/json",
            body: graph_json.as_bytes().to_vec(),
        };
    }

    match Asset::get(asset_path) {
        Some(file) => RouteResponse {
            status: 200,
            content_type: content_type_for(asset_path),
            body: file.data.into_owned(),
        },
        None => RouteResponse {
            status: 404,
            content_type: "text/plain; charset=utf-8",
            body: format!("not found: {asset_path}").into_bytes(),
        },
    }
}

fn content_type_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// Turn a routing decision into an actual HTTP response and send it.
/// Kept separate from `route` so the interesting logic stays testable
/// without a `tiny_http::Request` in hand.
pub fn respond(request: tiny_http::Request, decision: RouteResponse) -> std::io::Result<()> {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], decision.content_type)
        .expect("content-type values are always valid ASCII");
    let response = tiny_http::Response::from_data(decision.body)
        .with_status_code(decision.status)
        .with_header(header);
    request.respond(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRAPH: &str = r#"{"nodes":[],"edges":[],"stats":{}}"#;

    #[test]
    fn the_graph_route_returns_the_freshly_scanned_json_body() {
        let response = route("/graph.json", GRAPH);
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json");
        assert_eq!(response.body, GRAPH.as_bytes());
    }

    #[test]
    fn the_graph_route_wins_over_any_embedded_graph_json() {
        // Even if `app/dist` happened to contain its own `graph.json` (a
        // stale copy from `app/public`), the live scan must be what gets
        // served — this is the whole reason `/graph.json` is special-cased
        // ahead of the embedded-asset lookup.
        let response = route("/graph.json?x=1", GRAPH);
        assert_eq!(response.body, GRAPH.as_bytes());
    }

    #[test]
    fn root_serves_the_embedded_index_html() {
        let response = route("/", GRAPH);
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "text/html; charset=utf-8");
        assert!(!response.body.is_empty());
    }

    #[test]
    fn an_unknown_path_is_a_404_not_a_panic() {
        let response = route("/this/does/not/exist.xyz", GRAPH);
        assert_eq!(response.status, 404);
    }

    #[test]
    fn the_embedded_asset_lookup_finds_index_html() {
        assert!(
            Asset::get("index.html").is_some(),
            "app/dist/index.html should be embedded; run `cd app && bun run build`"
        );
    }
}
