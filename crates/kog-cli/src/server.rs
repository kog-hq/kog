//! The `view` command's HTTP layer: the page embedded from `app/dist`,
//! plus a `/graph.json` route serving whatever was just scanned.
//!
//! Routing is a pure function (`route`) precisely so it can be tested
//! without opening a socket — see the tests below and the design brief
//! this module implements.

use kog_graph::Workspace;
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
pub fn route(path: &str, graph_json: &str, workspace: &Workspace) -> RouteResponse {
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

    // Exports are generated here, by the same code `kog scan --graphml`
    // calls, rather than rebuilt in TypeScript for the page. A second
    // implementation would drift, and only one of the two would be tested.
    // Generated per request: the graph is static for the life of the process,
    // so holding four serialisations in memory for a download nobody may ask
    // for is the wrong trade.
    if let Some(format) = asset_path.strip_prefix("export/") {
        let (content_type, body) = match format {
            "graph.graphml" => ("application/xml", kog_graph::graphml(workspace)),
            "graph.cypher" => ("text/plain; charset=utf-8", kog_graph::cypher(workspace)),
            "graph.yaml" => ("text/yaml; charset=utf-8", kog_graph::yaml(workspace)),
            "report.md" => (
                "text/markdown; charset=utf-8",
                kog_graph::markdown(workspace),
            ),
            _ => {
                return RouteResponse {
                    status: 404,
                    content_type: "text/plain; charset=utf-8",
                    body: format!("no such export: {format}").into_bytes(),
                }
            }
        };
        return RouteResponse {
            status: 200,
            content_type,
            body: body.into_bytes(),
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
    use kog_graph::scan_workspace;

    /// A scan of nothing, for the routes that do not care about the graph.
    fn empty() -> Workspace {
        scan_workspace(tempfile::TempDir::new().unwrap().path())
    }

    /// The page downloads its exports from the binary rather than rebuilding
    /// them in TypeScript: one implementation, and it is the one with tests.
    #[test]
    fn an_export_route_is_generated_by_the_same_code_as_the_cli() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"app"}"#).unwrap();
        std::fs::write(dir.path().join("src/lib.ts"), "export const x = 1;").unwrap();
        std::fs::write(dir.path().join("src/a.ts"), r#"import { x } from "./lib";"#).unwrap();
        let workspace = scan_workspace(dir.path());

        for (path, needle, content_type) in [
            ("/export/graph.graphml", "<graphml", "application/xml"),
            (
                "/export/graph.cypher",
                "MERGE (f:File",
                "text/plain; charset=utf-8",
            ),
            (
                "/export/graph.yaml",
                "projects:",
                "text/yaml; charset=utf-8",
            ),
            (
                "/export/report.md",
                "# Scan of",
                "text/markdown; charset=utf-8",
            ),
        ] {
            let response = route(path, "{}", &workspace);
            assert_eq!(response.status, 200, "{path}");
            assert_eq!(response.content_type, content_type, "{path}");
            let body = String::from_utf8(response.body).unwrap();
            assert!(body.contains(needle), "{path} produced {body:.120}");
        }

        let missing = route("/export/nope.txt", "{}", &workspace);
        assert_eq!(missing.status, 404);
    }

    use super::*;

    const GRAPH: &str = r#"{"nodes":[],"edges":[],"stats":{}}"#;

    #[test]
    fn the_graph_route_returns_the_freshly_scanned_json_body() {
        let response = route("/graph.json", GRAPH, &empty());
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
        let response = route("/graph.json?x=1", GRAPH, &empty());
        assert_eq!(response.body, GRAPH.as_bytes());
    }

    #[test]
    fn root_serves_the_embedded_index_html() {
        let response = route("/", GRAPH, &empty());
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "text/html; charset=utf-8");
        assert!(!response.body.is_empty());
    }

    #[test]
    fn an_unknown_path_is_a_404_not_a_panic() {
        let response = route("/this/does/not/exist.xyz", GRAPH, &empty());
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
