//! An MCP server, so an agent can ask the graph what it already knows.
//!
//! The whole argument for this file is one measured number. On
//! [`documenso`](https://github.com/documenso/documenso),
//! `packages/prisma/index.ts` has **484** dependents. Grepping the
//! repository for that file's own path finds **0**: every one of those
//! imports is written `@documenso/prisma`, and a text search cannot connect
//! an alias to the file it names. An agent that greps concludes the file is
//! unused, and is wrong by all 484.
//!
//! The transport is newline-delimited JSON-RPC 2.0 over stdio, written by
//! hand against the specification rather than pulled in as a dependency: the
//! surface is `server/discover`, `initialize`, `tools/list` and
//! `tools/call`, and an async runtime to carry four methods would cost more
//! binary than the whole graph engine.
//!
//! ## Two eras on one socket
//!
//! MCP changed shape at revision `2026-07-28`. *Modern* clients declare
//! their protocol version in each request's `_meta` and never handshake;
//! *legacy* clients (`2025-11-25` and earlier) open with `initialize`. Most
//! clients in the wild are still legacy, and a server that picked one era
//! would be unreachable from half of them, so this one answers both and lets
//! the client's opening move decide which it gets.

use anyhow::{Context, Result};
use kog_graph::{render, Answer, Atlas, Question, DEFAULT_DEPTH, DEFAULT_LIMIT};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;

/// Revisions that carry their version in `_meta` and expect `resultType` on
/// every result. Newest first.
const MODERN_VERSIONS: &[&str] = &["2026-07-28"];

/// Revisions that open with an `initialize` handshake. Newest first: the
/// first entry is what an unrecognised request is answered with.
const LEGACY_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// The key a modern client states its protocol version under.
const VERSION_META: &str = "io.modelcontextprotocol/protocolVersion";

/// Told to the model once, before it has called anything. Its job is to stop
/// the agent reaching for `grep` when the graph has a better answer.
const INSTRUCTIONS: &str = "\
kog has scanned this repository into a file/import graph and answers structural \
questions about it. Imports are resolved through path aliases, workspace packages \
and language-specific module rules, so these answers include imports that a text \
search cannot find: on documenso, `packages/prisma/index.ts` has 484 dependents \
while grepping for that path finds 0, because every import of it is written \
`@documenso/prisma`. Ask here before grepping for what uses a file. Every answer \
carries an exact total and says so when a list was truncated.";

/// A JSON-RPC error that is about the request itself rather than about the
/// question it asked. Distinct from a tool execution error, which travels in
/// a successful result under `isError` so the model can correct itself.
struct MethodError {
    code: i64,
    message: String,
    data: Option<Value>,
}

/// Scan `root`, then answer questions about it on stdin until the stream
/// closes.
///
/// Progress goes to stderr, never stdout: stdout carries MCP messages and
/// nothing else, or the client's parser breaks on the first line of chatter.
pub fn serve(root: &Path) -> Result<()> {
    eprintln!("kog: scanning {}", root.display());
    let atlas = Atlas::scan(root);
    let totals = &atlas.workspace().totals;
    eprintln!(
        "kog: {} nodes, {} edges across {} project(s) — resolution rate {:.4}, source coverage {:.4}",
        totals.nodes, totals.edges, totals.projects, totals.resolution_rate, totals.source_coverage
    );

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.context("reading stdin")?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle(&atlas, &line) {
            writeln!(stdout, "{response}").context("writing to stdout")?;
            stdout.flush().context("flushing stdout")?;
        }
    }
    Ok(())
}

/// Answer one message, or `None` when the message is a notification and the
/// protocol forbids answering it.
fn handle(atlas: &Atlas, line: &str) -> Option<String> {
    let message: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        // A message that would not parse has no id to answer under, and
        // JSON-RPC says to use null rather than stay silent.
        Err(error) => {
            return Some(failure(
                Value::Null,
                MethodError {
                    code: -32700,
                    message: format!("parse error: {error}"),
                    data: None,
                },
            ))
        }
    };

    // No id means a notification (`notifications/initialized` and friends).
    // The specification is explicit that these are never answered.
    let id = message.get("id").cloned()?;
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));

    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Some(failure(
            id,
            MethodError {
                code: -32600,
                message: "invalid request: no method".to_string(),
                data: None,
            },
        ));
    };

    // A request carrying a version in `_meta` is a modern one and is served
    // under modern rules; anything else gets the legacy shape. This is the
    // whole of the era decision.
    let declared = params.get("_meta").and_then(|meta| meta.get(VERSION_META));
    if let Some(version) = declared.and_then(Value::as_str) {
        if !MODERN_VERSIONS.contains(&version) {
            return Some(failure(
                id,
                MethodError {
                    code: -32022,
                    message: "Unsupported protocol version".to_string(),
                    data: Some(json!({
                        "supported": supported_versions(),
                        "requested": version,
                    })),
                },
            ));
        }
    }
    let modern = declared.is_some();

    let outcome = match method {
        // `server/discover` belongs to the modern era whoever calls it, so
        // its result is always shaped that way.
        "server/discover" => Ok(with_result_type(discover(), true)),
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(with_result_type(json!({}), modern)),
        "tools/list" => Ok(with_result_type(json!({ "tools": tools() }), modern)),
        "tools/call" => call_tool(atlas, &params).map(|result| with_result_type(result, modern)),
        other => Err(MethodError {
            code: -32601,
            message: format!("unknown method: {other}"),
            data: None,
        }),
    };

    Some(match outcome {
        Ok(result) => success(id, result),
        Err(error) => failure(id, error),
    })
}

/// Every version this server speaks, modern first — the list a client reads
/// out of an `UnsupportedProtocolVersionError` to pick a version it can use.
fn supported_versions() -> Vec<&'static str> {
    MODERN_VERSIONS
        .iter()
        .chain(LEGACY_VERSIONS.iter())
        .copied()
        .collect()
}

fn server_info() -> Value {
    json!({ "name": "kog", "version": env!("CARGO_PKG_VERSION") })
}

fn discover() -> Value {
    json!({
        "supportedVersions": supported_versions(),
        "capabilities": { "tools": {} },
        "instructions": INSTRUCTIONS,
        "_meta": { "io.modelcontextprotocol/serverInfo": server_info() },
    })
}

/// The legacy handshake. The client's requested version is echoed when this
/// server speaks it; otherwise the newest one it does speak is offered, and
/// the client decides whether it can follow.
fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = match asked {
        Some(asked) if LEGACY_VERSIONS.contains(&asked) => asked,
        _ => LEGACY_VERSIONS[0],
    };
    json!({
        "protocolVersion": version,
        // `listChanged: false` is the honest answer: the graph is scanned
        // once at startup, so the tool list cannot change under the client.
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": server_info(),
        "instructions": INSTRUCTIONS,
    })
}

/// Modern results carry `resultType`; legacy ones must not.
fn with_result_type(mut result: Value, modern: bool) -> Value {
    if modern {
        if let Some(object) = result.as_object_mut() {
            object.insert("resultType".to_string(), json!("complete"));
        }
    }
    result
}

/// The five questions the graph can answer, in the order an agent should
/// meet them: what this repository is, then who uses what.
fn tools() -> Value {
    json!([
        {
            "name": "scan_summary",
            "title": "Summarise the scanned repository",
            "description": "What kog measured: files, nodes, edges, the resolution rate and the \
    source coverage, the rate per language, the file extensions kog could not read, and the \
    most depended-upon files. Call this first on an unfamiliar repository — the hubs it names \
    are where the code actually lives.",
            "inputSchema": { "type": "object", "additionalProperties": false }
        },
        {
            "name": "what_depends_on",
            "title": "What depends on this file",
            "description": "Every file that imports PATH, resolved through path aliases and \
    workspace packages. This is what grep cannot do: on documenso, packages/prisma/index.ts \
    answers 484 here and 0 to a text search for its own path. Returns the exact total even \
    when the list of names is capped.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path. A repository-relative path, an absolute path, or any unambiguous tail of one (`prisma/index.ts`). If it names more than one file, the candidates are returned instead of a guess." },
                    "limit": { "type": "integer", "minimum": 1, "description": "How many file names to list. The total is reported in full regardless. Default 50." }
                },
                "required": ["path"],
                "additionalProperties": false
            }
        },
        {
            "name": "what_does_x_depend_on",
            "title": "What this file depends on",
            "description": "Every file PATH imports, in the same resolved form. External \
    packages are not files and are not listed here — use files_touching_package for those.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path, in any of the forms what_depends_on accepts." },
                    "limit": { "type": "integer", "minimum": 1, "description": "How many file names to list. Default 50." }
                },
                "required": ["path"],
                "additionalProperties": false
            }
        },
        {
            "name": "blast_radius",
            "title": "What changing this file could reach",
            "description": "Everything that transitively imports PATH, out to DEPTH hops, with \
    a count per hop. Answers 'if I change this, what could break?'. Says explicitly when the \
    walk stopped because it hit the depth limit rather than the end of the graph, in which \
    case the number reported is a floor.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path, in any of the forms what_depends_on accepts." },
                    "depth": { "type": "integer", "minimum": 1, "maximum": 32, "description": "How many hops to walk. Default 3." },
                    "limit": { "type": "integer", "minimum": 1, "description": "How many file names to list. Default 50." }
                },
                "required": ["path"],
                "additionalProperties": false
            }
        },
        {
            "name": "files_touching_package",
            "title": "Which files import an external package",
            "description": "Every file that imports the external package NAME — the dependency \
    question rather than the file question. Use the package name exactly as it is written in \
    an import (`react`, `@documenso/prisma`); a name that matches nothing returns the close \
    names rather than an empty answer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Package name as written in imports." },
                    "limit": { "type": "integer", "minimum": 1, "description": "How many file names to list. Default 50." }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }
    ])
}

fn call_tool(atlas: &Atlas, params: &Value) -> Result<Value, MethodError> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err(MethodError {
            code: -32602,
            message: "invalid params: no tool name".to_string(),
            data: None,
        });
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // An unknown *tool* is a request the model got structurally wrong, so it
    // is a protocol error. A missing *argument* is something the model can
    // fix on the next call, so it travels back as a tool error instead.
    let question = match name {
        "scan_summary" => Question::Summary,
        "what_depends_on" => match text_argument(&arguments, "path") {
            Ok(path) => Question::Dependents { path },
            Err(message) => return Ok(tool_result(message, true)),
        },
        "what_does_x_depend_on" => match text_argument(&arguments, "path") {
            Ok(path) => Question::Dependencies { path },
            Err(message) => return Ok(tool_result(message, true)),
        },
        "blast_radius" => match text_argument(&arguments, "path") {
            Ok(path) => Question::BlastRadius {
                path,
                depth: whole_argument(&arguments, "depth", DEFAULT_DEPTH),
            },
            Err(message) => return Ok(tool_result(message, true)),
        },
        "files_touching_package" => match text_argument(&arguments, "name") {
            Ok(package) => Question::PackageUsers { package },
            Err(message) => return Ok(tool_result(message, true)),
        },
        other => {
            return Err(MethodError {
                code: -32602,
                message: format!("unknown tool: {other}"),
                data: None,
            })
        }
    };

    let answer = atlas.answer(
        &question,
        whole_argument(&arguments, "limit", DEFAULT_LIMIT),
    );
    // A path that named nothing, or named three files, is reported as a tool
    // error on purpose: the model can act on that and ask again, whereas a
    // plain success would read as "this file has no dependents".
    let recoverable = matches!(answer, Answer::Unlocated { .. });
    Ok(tool_result(render(&answer), recoverable))
}

fn text_argument(arguments: &Value, key: &str) -> Result<String, String> {
    match arguments.get(key).and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => Ok(value.to_string()),
        Some(_) => Err(format!("`{key}` was empty; it must name a file or package")),
        None => Err(format!("`{key}` is required and was not given")),
    }
}

/// A non-negative integer argument, falling back to `default` for anything
/// missing or nonsensical. Deliberately forgiving: a bad `limit` should not
/// cost the caller the answer it asked for.
fn whole_argument(arguments: &Value, key: &str, default: usize) -> usize {
    arguments
        .get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn tool_result(text: String, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    })
}

/// Serialise one message onto one line.
///
/// `serde_json` escapes every newline inside a string, so a rendered answer
/// containing fifty file paths still leaves the message a single line — which
/// the stdio transport requires and the tests below hold it to.
fn line(message: Value) -> String {
    serde_json::to_string(&message).unwrap_or_else(|error| {
        // Unreachable for values built out of `json!`, but a panic here would
        // take down a server mid-conversation over a formatting problem.
        format!(
            r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32603,"message":"could not serialise a response: {error}"}}}}"#
        )
    })
}

fn success(id: Value, result: Value) -> String {
    line(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn failure(id: Value, error: MethodError) -> String {
    let mut body = json!({ "code": error.code, "message": error.message });
    if let Some(data) = error.data {
        body["data"] = data;
    }
    line(json!({ "jsonrpc": "2.0", "id": id, "error": body }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    /// A repository whose only internal import is written as an alias, so
    /// the target's path appears in no other file — documenso's shape, small
    /// enough to assert on exactly.
    fn aliased_project() -> (TempDir, Atlas) {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"root"}"#);
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@acme/*": ["./packages/*"] } } }"#,
        );
        write(&dir, "packages/db.ts", "export const db = 1;");
        write(&dir, "apps/web.ts", r#"import { db } from "@acme/db";"#);
        write(&dir, "apps/api.ts", r#"import { db } from "@acme/db";"#);
        let atlas = Atlas::scan(dir.path());
        (dir, atlas)
    }

    fn ask(atlas: &Atlas, request: Value) -> Value {
        let line = handle(atlas, &request.to_string()).expect("a request must be answered");
        serde_json::from_str(&line).expect("a response must be valid JSON")
    }

    fn call(atlas: &Atlas, name: &str, arguments: Value) -> Value {
        ask(
            atlas,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": name, "arguments": arguments }
            }),
        )
    }

    fn text_of(response: &Value) -> &str {
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("a tool result must carry text content")
    }

    // --- The two eras ---

    #[test]
    fn a_legacy_client_is_answered_with_the_version_it_asked_for() {
        let (_dir, atlas) = aliased_project();
        let response = ask(
            &atlas,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "protocolVersion": "2025-06-18", "capabilities": {} }
            }),
        );

        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(response["result"]["serverInfo"]["name"], "kog");
        assert!(response["result"]["capabilities"]["tools"].is_object());
        assert!(
            response["result"].get("resultType").is_none(),
            "a legacy result must not carry the modern `resultType` field"
        );
    }

    #[test]
    fn a_legacy_client_asking_for_a_version_we_do_not_speak_is_offered_our_newest() {
        let (_dir, atlas) = aliased_project();
        let response = ask(
            &atlas,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "protocolVersion": "1900-01-01" }
            }),
        );
        assert_eq!(response["result"]["protocolVersion"], LEGACY_VERSIONS[0]);
    }

    #[test]
    fn server_discover_names_every_version_this_server_speaks() {
        let (_dir, atlas) = aliased_project();
        let response = ask(
            &atlas,
            json!({
                "jsonrpc": "2.0", "id": "d1", "method": "server/discover",
                "params": { "_meta": { VERSION_META: MODERN_VERSIONS[0] } }
            }),
        );

        let result = &response["result"];
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["supportedVersions"][0], MODERN_VERSIONS[0]);
        let versions: Vec<&str> = result["supportedVersions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(versions, supported_versions(), "modern first, then legacy");
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "kog"
        );
        assert!(result["capabilities"]["tools"].is_object());
    }

    /// The one field that separates the two eras on an ordinary call. Getting
    /// it wrong is silent: a modern client validates the result shape, a
    /// legacy one does not, so only one of the two ever complains.
    #[test]
    fn only_a_modern_request_gets_result_type_on_its_result() {
        let (_dir, atlas) = aliased_project();

        let modern = ask(
            &atlas,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/list",
                "params": { "_meta": { VERSION_META: MODERN_VERSIONS[0] } }
            }),
        );
        assert_eq!(modern["result"]["resultType"], "complete");

        let legacy = ask(
            &atlas,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
        );
        assert!(legacy["result"].get("resultType").is_none());
    }

    #[test]
    fn an_unsupported_modern_version_is_refused_with_the_list_that_would_work() {
        let (_dir, atlas) = aliased_project();
        let response = ask(
            &atlas,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/list",
                "params": { "_meta": { VERSION_META: "1900-01-01" } }
            }),
        );

        assert_eq!(response["error"]["code"], -32022);
        assert_eq!(response["error"]["data"]["requested"], "1900-01-01");
        assert_eq!(
            response["error"]["data"]["supported"][0], MODERN_VERSIONS[0],
            "the client needs a version it can retry with, not just a refusal"
        );
    }

    #[test]
    fn a_notification_is_never_answered() {
        let (_dir, atlas) = aliased_project();
        for notification in [
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 1 } }),
        ] {
            assert!(
                handle(&atlas, &notification.to_string()).is_none(),
                "a message with no id must produce no reply, got one for {notification}"
            );
        }
    }

    // --- The tools ---

    #[test]
    fn tools_list_publishes_every_tool_with_a_usable_schema() {
        let (_dir, atlas) = aliased_project();
        let response = ask(
            &atlas,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        );

        let tools = response["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "scan_summary",
                "what_depends_on",
                "what_does_x_depend_on",
                "blast_radius",
                "files_touching_package",
            ]
        );
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert!(
                !tool["description"].as_str().unwrap().is_empty(),
                "{} has no description, so a model cannot choose it",
                tool["name"]
            );
        }
    }

    /// The acceptance case, in miniature: the answer is a number, the number
    /// is right, and a text search for the target's path would have found
    /// nothing.
    #[test]
    fn what_depends_on_finds_the_importers_an_alias_hides() {
        let (dir, atlas) = aliased_project();
        for file in ["apps/web.ts", "apps/api.ts"] {
            let source = fs::read_to_string(dir.path().join(file)).unwrap();
            assert!(
                !source.contains("packages/db.ts"),
                "{file} must not name the target's path, or this proves nothing"
            );
        }

        let response = call(
            &atlas,
            "what_depends_on",
            json!({ "path": "packages/db.ts" }),
        );

        assert_eq!(response["result"]["isError"], false);
        let text = text_of(&response);
        assert!(text.contains("2 dependents"), "got {text:?}");
        assert!(text.contains("apps/api.ts") && text.contains("apps/web.ts"));
    }

    #[test]
    fn what_does_x_depend_on_answers_the_other_direction() {
        let (_dir, atlas) = aliased_project();
        let response = call(
            &atlas,
            "what_does_x_depend_on",
            json!({ "path": "apps/web.ts" }),
        );
        let text = text_of(&response);
        assert!(text.contains("1 dependencies"), "got {text:?}");
        assert!(text.contains("packages/db.ts"));
    }

    #[test]
    fn blast_radius_takes_a_depth_and_reports_the_one_it_used() {
        let (_dir, atlas) = aliased_project();
        let response = call(
            &atlas,
            "blast_radius",
            json!({ "path": "packages/db.ts", "depth": 2 }),
        );
        let text = text_of(&response);
        assert!(
            text.contains("2 files reached within depth 2"),
            "got {text:?}"
        );
    }

    #[test]
    fn files_touching_package_answers_the_dependency_question() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/a.ts", r#"import React from "react";"#);
        let atlas = Atlas::scan(dir.path());

        let response = call(&atlas, "files_touching_package", json!({ "name": "react" }));
        assert!(text_of(&response).contains("imported by 1 files"));
    }

    #[test]
    fn scan_summary_publishes_the_two_measured_numbers() {
        let (_dir, atlas) = aliased_project();
        let response = call(&atlas, "scan_summary", json!({}));
        let text = text_of(&response);
        assert!(text.contains("resolution rate"), "got {text:?}");
        assert!(text.contains("source coverage"), "got {text:?}");
        assert!(text.contains("most depended upon"), "got {text:?}");
    }

    #[test]
    fn a_limit_caps_the_names_and_never_the_total() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/lib.ts", "");
        for i in 0..8 {
            write(&dir, &format!("src/f{i}.ts"), r#"import "./lib";"#);
        }
        let atlas = Atlas::scan(dir.path());

        let response = call(
            &atlas,
            "what_depends_on",
            json!({ "path": "src/lib.ts", "limit": 2 }),
        );
        let text = text_of(&response);
        assert!(text.contains("8 dependents"), "got {text:?}");
        assert!(text.contains("6 more not shown"), "got {text:?}");
    }

    // --- Getting it wrong ---

    /// A path that names nothing comes back as a tool error, not a cheerful
    /// zero. The difference decides whether the model retries or concludes
    /// the file is unused.
    #[test]
    fn a_path_that_names_nothing_is_a_recoverable_tool_error() {
        let (_dir, atlas) = aliased_project();
        let response = call(&atlas, "what_depends_on", json!({ "path": "nowhere.ts" }));

        assert_eq!(response["result"]["isError"], true);
        assert!(text_of(&response).contains("nothing in the graph is named"));
    }

    #[test]
    fn an_ambiguous_path_returns_the_candidates_rather_than_a_guess() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/one/index.ts", "");
        write(&dir, "src/two/index.ts", "");
        let atlas = Atlas::scan(dir.path());

        let response = call(&atlas, "what_depends_on", json!({ "path": "index.ts" }));

        assert_eq!(response["result"]["isError"], true);
        let text = text_of(&response);
        assert!(text.contains("src/one/index.ts") && text.contains("src/two/index.ts"));
    }

    #[test]
    fn a_missing_argument_is_a_tool_error_that_names_the_argument() {
        let (_dir, atlas) = aliased_project();
        let response = call(&atlas, "what_depends_on", json!({}));

        assert_eq!(response["result"]["isError"], true);
        assert!(text_of(&response).contains("`path` is required"));
    }

    #[test]
    fn an_unknown_tool_is_a_protocol_error_not_a_tool_error() {
        let (_dir, atlas) = aliased_project();
        let response = call(&atlas, "drop_database", json!({}));
        assert_eq!(response["error"]["code"], -32602);
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("drop_database"));
    }

    #[test]
    fn an_unknown_method_is_refused_with_the_documented_code() {
        let (_dir, atlas) = aliased_project();
        let response = ask(
            &atlas,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" }),
        );
        assert_eq!(response["error"]["code"], -32601);
    }

    #[test]
    fn malformed_json_is_answered_under_a_null_id() {
        let (_dir, atlas) = aliased_project();
        let line = handle(&atlas, "{ this is not json").unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], Value::Null);
        assert_eq!(response["error"]["code"], -32700);
    }

    /// The stdio transport forbids embedded newlines, and every answer this
    /// server gives is multi-line text. `serde_json` escapes them; this holds
    /// it to that, because the failure mode is a client whose parser desyncs
    /// halfway through a conversation.
    #[test]
    fn a_response_is_always_exactly_one_line() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{"name":"app"}"#);
        write(&dir, "src/lib.ts", "");
        for i in 0..20 {
            write(&dir, &format!("src/f{i}.ts"), r#"import "./lib";"#);
        }
        let atlas = Atlas::scan(dir.path());

        let requests = [
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "server/discover" }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                    "params": { "name": "what_depends_on", "arguments": { "path": "src/lib.ts" } } }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                    "params": { "name": "scan_summary", "arguments": {} } }),
        ];
        for request in requests {
            let line = handle(&atlas, &request.to_string()).unwrap();
            assert!(
                !line.contains('\n'),
                "a response must not contain a raw newline, got {line:?}"
            );
            // And the newlines are still there, escaped, once parsed back.
            let response: Value = serde_json::from_str(&line).unwrap();
            assert!(response["result"].is_object());
        }
    }
}
