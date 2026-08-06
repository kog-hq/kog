# KOG v0 — implementation plan

> The project was called `mycelium` throughout this plan's execution; it was renamed
> `kog` after the v0 merge. Crate, binary and path names have been updated here to stay
> usable, but the original instructions referred to `mycelium`.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** parse a TypeScript project into a file/import graph, write it to
`graph.json` with its measured resolution rate, and render it in WebGL.

**Architecture:** a two-crate Rust workspace. `kog-graph` exposes an `Extractor`
trait and a single TypeScript implementation (tree-sitter + tsconfig);
`kog-cli` orchestrates traversal, extraction, resolution and serialization. A
Vite+sigma page consumes the JSON. No Tauri layer in v0.

**Tech stack:** Rust 2021 · tree-sitter 0.26.11 · tree-sitter-typescript 0.23.2 ·
jsonc-parser 0.33.1 · ignore · serde/serde_json · clap 4 · Vite · React · TypeScript ·
sigma 3.0.3 · graphology 0.26.0

## Global constraints

- The reference spec is `docs/design/v0-design.md`. In case of contradiction, the
  spec governs.
- Code, comments, identifiers, product strings and commit messages: **English**.
  Documentation: **English**.
- Conventional commits (`feat:`, `fix:`, `test:`, `docs:`, `chore:`, `ci:`).
- **Never override the git identity.** No `-c user.email`, no local `user.*`: the
  machine's global config governs.
- `~/.cargo/bin` is absent from the non-interactive PATH. Every cargo command must be
  preceded by `export PATH="$HOME/.cargo/bin:$PATH"`.
- No filter should *fail open*: a filter that cannot apply excludes.
- Every task ends with `cargo fmt`, `cargo clippy -- -D warnings` and
  `cargo test` green before commit.
- Scope frozen: no Tauri, no Leiden, no multi-project, no AI, no overlays, no
  session graph. See spec §10.

### Reference projects (absolute paths, development machine)

| Role | Path | Volume |
| --- | --- | --- |
| Quick fixture | `~/apps/lueur` | 93 files, `@/*` alias |
| Acceptance target | `~/Mastore/mastore-saas` | 727 files, Turborepo monorepo |

---

## File structure

| File | Responsibility |
| --- | --- |
| `crates/kog-graph/src/model.rs` | `Graph`, `Node`, `Edge`, `Stats`, `Failure`. Serde. Language-agnostic, zero logic |
| `crates/kog-graph/src/extractor.rs` | `Extractor` trait, `Specifier`, `Resolution`, `ExtractError` |
| `crates/kog-graph/src/tsconfig.rs` | JSONC loading, `extends` chain, per-directory mapping index |
| `crates/kog-graph/src/extractors/typescript.rs` | tree-sitter grammar, import query, TS resolution rules |
| `crates/kog-graph/src/discover.rs` | Traversal respecting `.gitignore`, filtering by extension |
| `crates/kog-graph/src/graph.rs` | Assembly, deduplication, statistics |
| `crates/kog-graph/src/lib.rs` | Public re-exports |
| `crates/kog-cli/src/main.rs` | clap CLI, orchestration, JSON output and summary |
| `app/src/main.tsx` | Loads `graph.json`, graphology, sigma |

---

## Task 1 — workspace scaffolding

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `LICENSE-MIT`,
  `LICENSE-APACHE`, `.gitleaks.toml`, `.github/workflows/ci.yml`,
  `crates/kog-graph/Cargo.toml`, `crates/kog-graph/src/lib.rs`,
  `crates/kog-cli/Cargo.toml`, `crates/kog-cli/src/main.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: a workspace that compiles, `kog` as a binary.

- [ ] **Step 1: pull the reusable scaffolding from dejavu**

```bash
cd ~/apps/mycelium
for f in LICENSE-MIT LICENSE-APACHE .gitleaks.toml rust-toolchain.toml .gitignore; do
  cp ~/apps/dejavu/"$f" .
done
mkdir -p .github/workflows
cp ~/apps/dejavu/.github/workflows/ci.yml .github/workflows/
cp ~/apps/dejavu/.github/{CODE_OF_CONDUCT.md,CONTRIBUTING.md,SECURITY.md,PULL_REQUEST_TEMPLATE.md} .github/
cp -r ~/apps/dejavu/.github/ISSUE_TEMPLATE .github/
```

Then reread every copied file and replace every occurrence of `dejavu` with
`mycelium` — the project's name as of this date. Verify:

```bash
rg -i "dejavu" . --glob '!.git' || echo "no residual occurrence"
```

- [ ] **Step 2: write the workspace manifest**

`Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/kog-graph", "crates/kog-cli"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
repository = "https://github.com/bstcoc/kog"

[workspace.dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
ignore = "0.4"
jsonc-parser = { version = "0.33", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
streaming-iterator = "0.1"
tree-sitter = "0.26"
tree-sitter-typescript = "0.23"
```

`crates/kog-graph/Cargo.toml`:

```toml
[package]
name = "kog-graph"
description = "Turns a codebase into a file/import graph"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
anyhow.workspace = true
ignore.workspace = true
jsonc-parser.workspace = true
serde.workspace = true
serde_json.workspace = true
streaming-iterator.workspace = true
tree-sitter.workspace = true
tree-sitter-typescript.workspace = true

[dev-dependencies]
tempfile = "3"
```

`crates/kog-cli/Cargo.toml`:

```toml
[package]
name = "kog-cli"
description = "Command line interface for kog"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[[bin]]
name = "kog"
path = "src/main.rs"

[dependencies]
anyhow.workspace = true
clap.workspace = true
kog-graph = { path = "../kog-graph" }
serde_json.workspace = true
```

- [ ] **Step 3: source skeletons**

`crates/kog-graph/src/lib.rs`:

```rust
//! Turns a codebase into a file/import graph.
```

`crates/kog-cli/src/main.rs`:

```rust
fn main() {
    println!("kog");
}
```

- [ ] **Step 4: verify the workspace compiles**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo build 2>&1 | tail -5
```

Expected: `Finished`, no errors.

- [ ] **Step 5: commit**

```bash
git add -A
git commit -m "chore: set up the cargo workspace and open source scaffolding"
```

---

## Task 2 — the graph model

**Files:**
- Create: `crates/kog-graph/src/model.rs`
- Modify: `crates/kog-graph/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `Graph`, `Node`, `Edge`, `EdgeKind`, `Stats`, `Failure`. Every following
  task depends on them. Fields exactly as below — spec §5 fixes the JSON format, it
  must not drift.

- [ ] **Step 1: write the failing test**

`crates/kog-graph/src/model.rs`, at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_computes_the_resolution_rate_over_internal_specifiers_only() {
        let stats = Stats {
            specifiers_internal: 3209,
            resolved: 3140,
            unresolved: 69,
            ..Default::default()
        };
        assert!((stats.resolution_rate() - 0.9785).abs() < 1e-4);
    }

    #[test]
    fn resolution_rate_is_one_when_there_is_nothing_to_resolve() {
        let stats = Stats::default();
        assert_eq!(stats.resolution_rate(), 1.0);
    }

    #[test]
    fn a_graph_serialises_to_the_documented_shape() {
        let graph = Graph {
            nodes: vec![Node {
                id: "src/a.ts".into(),
                path: "src/a.ts".into(),
                lang: "typescript".into(),
                loc: 12,
                external_deps: vec!["react".into()],
            }],
            edges: vec![Edge {
                source: "src/a.ts".into(),
                target: "src/b.ts".into(),
                kind: EdgeKind::Import,
            }],
            stats: Stats::default(),
        };
        let json = serde_json::to_value(&graph).unwrap();
        assert_eq!(json["nodes"][0]["external_deps"][0], "react");
        assert_eq!(json["edges"][0]["kind"], "import");
    }
}
```

- [ ] **Step 2: run the test and verify it fails**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p kog-graph 2>&1 | tail -15
```

Expected: compile errors, `cannot find type Stats`, `Graph`, `Node`, `Edge`.

- [ ] **Step 3: write the minimal implementation**

At the top of `crates/kog-graph/src/model.rs`:

```rust
use serde::{Deserialize, Serialize};

/// One source file in the scanned project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Project-relative path, also used as the graph identity.
    pub id: String,
    pub path: String,
    pub lang: String,
    pub loc: usize,
    /// External package names this file imports, never graph edges.
    pub external_deps: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Import,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
}

/// A file that could not be read or parsed. Never fatal, always reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub files_discovered: usize,
    pub files_parsed: usize,
    pub specifiers_total: usize,
    pub specifiers_internal: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub resolution_rate: f64,
    pub external_specifiers: usize,
    pub external_packages_distinct: usize,
    pub failures: Vec<Failure>,
}

impl Stats {
    /// Resolved over internal specifiers. External specifiers are excluded:
    /// `import react` has no file to point at.
    pub fn resolution_rate(&self) -> f64 {
        if self.specifiers_internal == 0 {
            return 1.0;
        }
        self.resolved as f64 / self.specifiers_internal as f64
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub stats: Stats,
}
```

Note: `Stats` carries both the `resolution_rate` field (serialized, filled in by
`graph.rs` in task 7) and the `resolution_rate()` method that computes it. The field
exists so the JSON is readable without recomputation; the method is the source of truth.

In `lib.rs`:

```rust
//! Turns a codebase into a file/import graph.

pub mod model;

pub use model::{Edge, EdgeKind, Failure, Graph, Node, Stats};
```

- [ ] **Step 4: run the tests and verify they pass**

```bash
cargo test -p kog-graph 2>&1 | tail -8
```

Expected: `test result: ok. 3 passed`.

- [ ] **Step 5: commit**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
git add -A && git commit -m "feat(model): add the language-agnostic graph model"
```

---

## Task 3 — the Extractor trait

**Files:**
- Create: `crates/kog-graph/src/extractor.rs`
- Modify: `crates/kog-graph/src/lib.rs`

**Interfaces:**
- Consumes: nothing from the model.
- Produces: `Specifier`, `Resolution`, `ExtractError`, the `Extractor` trait. Task 6
  implements it, task 7 consumes it. Adding Go in v0.2 must reduce to a new
  implementation of this trait.

- [ ] **Step 1: write the failing test**

At the end of `crates/kog-graph/src/extractor.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    struct StubExtractor;

    impl Extractor for StubExtractor {
        fn lang(&self) -> &'static str {
            "stub"
        }
        fn extensions(&self) -> &'static [&'static str] {
            &["stub"]
        }
        fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
            Ok(source
                .lines()
                .enumerate()
                .filter(|(_, l)| !l.is_empty())
                .map(|(i, l)| Specifier {
                    raw: l.to_string(),
                    line: i + 1,
                })
                .collect())
        }
        fn resolve(&self, raw: &str, _importer: &Path) -> Resolution {
            if raw.starts_with('.') {
                Resolution::Internal(PathBuf::from(raw))
            } else {
                Resolution::External(raw.to_string())
            }
        }
    }

    #[test]
    fn an_extractor_reports_its_language_and_extensions() {
        let e = StubExtractor;
        assert_eq!(e.lang(), "stub");
        assert_eq!(e.extensions(), &["stub"]);
    }

    #[test]
    fn extraction_carries_the_line_number() {
        let specs = StubExtractor.extract("./a\n./b").unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[1].line, 2);
    }

    #[test]
    fn resolution_separates_internal_from_external() {
        let e = StubExtractor;
        let here = Path::new("src/x.stub");
        assert!(matches!(e.resolve("./a", here), Resolution::Internal(_)));
        assert!(matches!(e.resolve("react", here), Resolution::External(_)));
    }

    #[test]
    fn a_boxed_extractor_stays_usable() {
        let boxed: Box<dyn Extractor> = Box::new(StubExtractor);
        assert_eq!(boxed.lang(), "stub");
    }
}
```

- [ ] **Step 2: run the test and verify it fails**

```bash
cargo test -p kog-graph extractor 2>&1 | tail -12
```

Expected: `cannot find trait Extractor`.

- [ ] **Step 3: write the minimal implementation**

At the top of `crates/kog-graph/src/extractor.rs`:

```rust
use std::path::{Path, PathBuf};

/// One import specifier as written in the source, before any resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Specifier {
    /// The literal text between the quotes, e.g. `@/lib/api` or `react`.
    pub raw: String,
    /// One-based line number, for diagnostics.
    pub line: usize,
}

/// What a specifier turned out to point at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A file inside the scanned project. Path is absolute.
    Internal(PathBuf),
    /// A third-party package. Recorded on the node, never an edge.
    External(String),
    /// Looks internal but no file was found. Counted, never dropped silently.
    Unresolved,
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("failed to load the {0} grammar")]
    Grammar(&'static str),
    #[error("failed to parse the source")]
    Parse,
}

/// A language front end. One implementation per supported language.
///
/// A language ships only once it passes its own resolution gate — see the design
/// document, section 3.3.
pub trait Extractor {
    /// Stable language identifier, written into `Node::lang`.
    fn lang(&self) -> &'static str;

    /// File extensions this extractor claims, without the leading dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Pull every static import specifier out of one source file.
    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError>;

    /// Turn one specifier into a resolution, given the file that wrote it.
    fn resolve(&self, raw: &str, importer: &Path) -> Resolution;
}
```

Add `thiserror = "2"` to the root `Cargo.toml`'s `workspace.dependencies` and
`thiserror.workspace = true` to `kog-graph`'s dependencies.

In `lib.rs`:

```rust
pub mod extractor;

pub use extractor::{ExtractError, Extractor, Resolution, Specifier};
```

- [ ] **Step 4: run the tests and verify they pass**

```bash
cargo test -p kog-graph 2>&1 | tail -8
```

Expected: `test result: ok. 7 passed`.

- [ ] **Step 5: commit**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
git add -A && git commit -m "feat(extractor): add the language front end trait"
```

---

## Task 4 — loading tsconfig

**Files:**
- Create: `crates/kog-graph/src/tsconfig.rs`
- Modify: `crates/kog-graph/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `TsConfigIndex::build(root) -> TsConfigIndex` and
  `TsConfigIndex::mappings_for(&self, importer: &Path) -> &[PathMapping]`, with
  `PathMapping { pattern: String, targets: Vec<PathBuf> }` whose `targets` are already
  **absolute**. Consumed by task 6.

**Why this is the most important task in the plan.** Measured on the acceptance
target: 2,651 of the 3,209 internal imports go through an alias. Without this task,
82.6 % of the edges disappear. And **3 of the 6 tsconfigs contain comments**
(`//`) — a plain `serde_json::from_str` would fail on half of them.

- [ ] **Step 1: write the failing test**

At the end of `crates/kog-graph/src/tsconfig.rs`:

```rust
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

    #[test]
    fn a_tsconfig_with_comments_still_parses() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{
                // TS 6 changes the implicit default, pin it.
                "compilerOptions": {
                    "moduleResolution": "bundler",
                    "paths": { "@common/*": ["./src/common/*"] }
                }
            }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        let mappings = index.mappings_for(&dir.path().join("src/x.ts"));
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].pattern, "@common/*");
        assert_eq!(mappings[0].targets[0], dir.path().join("src/common/*"));
    }

    #[test]
    fn paths_are_relative_to_the_tsconfig_when_there_is_no_base_url() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "apps/web/tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        let mappings = index.mappings_for(&dir.path().join("apps/web/src/page.tsx"));
        assert_eq!(mappings[0].targets[0], dir.path().join("apps/web/src/*"));
    }

    #[test]
    fn base_url_shifts_the_target_root() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "baseUrl": "./src", "paths": { "@/*": ["lib/*"] } } }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        let mappings = index.mappings_for(&dir.path().join("src/a.ts"));
        assert_eq!(mappings[0].targets[0], dir.path().join("src/lib/*"));
    }

    #[test]
    fn an_extends_chain_is_followed_and_the_child_wins() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.base.json",
            r#"{ "compilerOptions": { "paths": { "@base/*": ["./base/*"] } } }"#,
        );
        write(
            &dir,
            "apps/api/tsconfig.json",
            r#"{
                "extends": "../../tsconfig.base.json",
                "compilerOptions": { "paths": { "@modules/*": ["./src/modules/*"] } }
            }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        let mappings = index.mappings_for(&dir.path().join("apps/api/src/a.ts"));
        let patterns: Vec<&str> = mappings.iter().map(|m| m.pattern.as_str()).collect();
        assert!(patterns.contains(&"@modules/*"));
        assert!(patterns.contains(&"@base/*"));
    }

    #[test]
    fn the_nearest_tsconfig_wins_over_an_ancestor() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./root/*"] } } }"#,
        );
        write(
            &dir,
            "apps/web/tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        let mappings = index.mappings_for(&dir.path().join("apps/web/src/page.tsx"));
        assert_eq!(mappings[0].targets[0], dir.path().join("apps/web/src/*"));
    }

    #[test]
    fn an_exact_non_wildcard_mapping_is_kept() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": {
                "@mastore/shared-types": ["./packages/shared-types/src/index.ts"]
            } } }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        let mappings = index.mappings_for(&dir.path().join("a.ts"));
        assert_eq!(mappings[0].pattern, "@mastore/shared-types");
        assert!(!mappings[0].pattern.contains('*'));
    }

    #[test]
    fn an_unreadable_tsconfig_is_skipped_without_panicking() {
        let dir = TempDir::new().unwrap();
        write(&dir, "tsconfig.json", "{ this is not json at all ");
        let index = TsConfigIndex::build(dir.path());
        assert!(index.mappings_for(&dir.path().join("a.ts")).is_empty());
    }

    #[test]
    fn a_missing_extends_target_does_not_lose_the_local_paths() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{
                "extends": "./does-not-exist.json",
                "compilerOptions": { "paths": { "@/*": ["./src/*"] } }
            }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        assert_eq!(index.mappings_for(&dir.path().join("a.ts")).len(), 1);
    }
}
```

- [ ] **Step 2: run the test and verify it fails**

```bash
cargo test -p kog-graph tsconfig 2>&1 | tail -12
```

Expected: `cannot find type TsConfigIndex`.

- [ ] **Step 3: write the minimal implementation**

At the top of `crates/kog-graph/src/tsconfig.rs`:

```rust
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawCompilerOptions {
    base_url: Option<String>,
    #[serde(default)]
    paths: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawTsConfig {
    extends: Option<String>,
    #[serde(default)]
    compiler_options: RawCompilerOptions,
}

/// One `paths` entry, with its targets already made absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMapping {
    /// The alias as written, e.g. `@common/*` or `@mastore/shared-types`.
    pub pattern: String,
    /// Absolute targets. A `*` inside is a placeholder, kept verbatim.
    pub targets: Vec<PathBuf>,
}

/// Every tsconfig in the project, keyed by the directory it governs.
#[derive(Debug, Default)]
pub struct TsConfigIndex {
    /// Sorted by descending directory depth, so the nearest config wins.
    scopes: Vec<(PathBuf, Vec<PathMapping>)>,
}

impl TsConfigIndex {
    /// Walk the project, load every `tsconfig*.json`, resolve `extends` chains.
    ///
    /// An unreadable or malformed config is skipped, never fatal: the subtree it
    /// governs falls back to relative-only resolution.
    pub fn build(root: &Path) -> Self {
        let mut scopes: Vec<(PathBuf, Vec<PathMapping>)> = Vec::new();

        let walker = ignore::WalkBuilder::new(root)
            .hidden(false)
            .git_ignore(true)
            .build();

        for entry in walker.flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !(name.starts_with("tsconfig") && name.ends_with(".json")) {
                continue;
            }
            let dir = match path.parent() {
                Some(d) => d.to_path_buf(),
                None => continue,
            };
            let mappings = Self::mappings_from_chain(path, 0);
            if !mappings.is_empty() {
                scopes.push((dir, mappings));
            }
        }

        // Deepest directory first: `mappings_for` returns on the first prefix hit.
        scopes.sort_by_key(|(dir, _)| std::cmp::Reverse(dir.components().count()));
        Self { scopes }
    }

    /// Collect mappings from this config and everything it extends.
    /// The child's own mappings come first so they take precedence.
    fn mappings_from_chain(config_path: &Path, depth: usize) -> Vec<PathMapping> {
        // Guards against a cyclic or absurdly deep `extends` chain.
        if depth > 16 {
            return Vec::new();
        }
        let text = match std::fs::read_to_string(config_path) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        let raw: RawTsConfig = match jsonc_parser::parse_to_serde_value::<RawTsConfig>(
            &text,
            &jsonc_parser::ParseOptions::default(),
        ) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        let config_dir = match config_path.parent() {
            Some(d) => d,
            None => return Vec::new(),
        };
        // Without `baseUrl`, targets are relative to the config's own directory.
        let base = match &raw.compiler_options.base_url {
            Some(b) => config_dir.join(b),
            None => config_dir.to_path_buf(),
        };

        let mut mappings: Vec<PathMapping> = raw
            .compiler_options
            .paths
            .iter()
            .map(|(pattern, targets)| PathMapping {
                pattern: pattern.clone(),
                targets: targets.iter().map(|t| normalise(&base.join(t))).collect(),
            })
            .collect();

        if let Some(parent_ref) = &raw.extends {
            // Only relative extends are supported in v0; a bare package name
            // would need node_modules resolution, which v0 does not do.
            if parent_ref.starts_with('.') {
                let parent_path = normalise(&config_dir.join(parent_ref));
                mappings.extend(Self::mappings_from_chain(&parent_path, depth + 1));
            }
        }

        mappings
    }

    /// Mappings that apply to a file, nearest enclosing tsconfig first.
    pub fn mappings_for(&self, importer: &Path) -> &[PathMapping] {
        for (dir, mappings) in &self.scopes {
            if importer.starts_with(dir) {
                return mappings;
            }
        }
        &[]
    }
}

/// Collapse `.` and `..` lexically. The targets may not exist yet, so
/// `canonicalize` is not usable here.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
```

In `lib.rs`:

```rust
pub mod tsconfig;

pub use tsconfig::{PathMapping, TsConfigIndex};
```

- [ ] **Step 4: run the tests and verify they pass**

```bash
cargo test -p kog-graph 2>&1 | tail -8
```

Expected: `test result: ok. 15 passed`.

- [ ] **Step 5: verify against the target's real tsconfigs**

```bash
cat >> crates/kog-graph/src/tsconfig.rs <<'RUST'

#[cfg(test)]
mod real_project_tests {
    use super::*;
    use std::path::Path;

    /// Ignored by default: depends on a checkout that only exists on the
    /// development machine. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn the_reference_monorepo_exposes_its_aliases() {
        let root = Path::new(env!("HOME")).join("Mastore/mastore-saas");
        if !root.exists() {
            eprintln!("reference checkout missing, skipping");
            return;
        }
        let index = TsConfigIndex::build(&root);

        let front = index.mappings_for(&root.join("apps/frontend/src/app/page.tsx"));
        assert!(front.iter().any(|m| m.pattern == "@/*"));

        let back = index.mappings_for(&root.join("apps/backend/src/main.ts"));
        let patterns: Vec<&str> = back.iter().map(|m| m.pattern.as_str()).collect();
        assert!(patterns.contains(&"@common/*"));
        assert!(patterns.contains(&"@modules/*"));
        assert!(patterns.contains(&"@lib/*"));
    }
}
RUST
cargo test -p kog-graph -- --ignored 2>&1 | tail -8
```

Expected: `test result: ok. 1 passed`. If this test fails, task 10's gate cannot
pass — fix it before continuing.

- [ ] **Step 6: commit**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
git add -A && git commit -m "feat(tsconfig): resolve path aliases across extends chains"
```

---

## Task 5 — file discovery

**Files:**
- Create: `crates/kog-graph/src/discover.rs`
- Modify: `crates/kog-graph/src/lib.rs`

**Interfaces:**
- Consumes: `Extractor::extensions()` from task 3.
- Produces: `discover(root: &Path, extensions: &[&str]) -> Vec<PathBuf>`, absolute,
  sorted paths. Consumed by task 7.

- [ ] **Step 1: write the failing test**

At the end of `crates/kog-graph/src/discover.rs`:

```rust
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

    #[test]
    fn only_the_requested_extensions_are_kept() {
        let dir = TempDir::new().unwrap();
        write(&dir, "a.ts", "");
        write(&dir, "b.tsx", "");
        write(&dir, "c.md", "");
        write(&dir, "d.png", "");
        let found = discover(dir.path(), &["ts", "tsx"]);
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn gitignored_files_are_skipped() {
        let dir = TempDir::new().unwrap();
        write(&dir, ".gitignore", "dist/\n");
        write(&dir, "src/a.ts", "");
        write(&dir, "dist/b.ts", "");
        let found = discover(dir.path(), &["ts"]);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("src/a.ts"));
    }

    #[test]
    fn heavy_build_directories_are_always_skipped() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "node_modules/pkg/index.ts", "");
        write(&dir, ".next/build.ts", "");
        write(&dir, "target/debug/x.ts", "");
        let found = discover(dir.path(), &["ts"]);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn the_result_is_sorted_so_runs_are_reproducible() {
        let dir = TempDir::new().unwrap();
        write(&dir, "z.ts", "");
        write(&dir, "a.ts", "");
        write(&dir, "m.ts", "");
        let found = discover(dir.path(), &["ts"]);
        let mut sorted = found.clone();
        sorted.sort();
        assert_eq!(found, sorted);
    }

    #[test]
    fn a_missing_root_yields_nothing_rather_than_panicking() {
        let found = discover(Path::new("/definitely/not/a/real/path"), &["ts"]);
        assert!(found.is_empty());
    }
}
```

- [ ] **Step 2: run the test and verify it fails**

```bash
cargo test -p kog-graph discover 2>&1 | tail -12
```

Expected: `cannot find function discover`.

- [ ] **Step 3: write the minimal implementation**

At the top of `crates/kog-graph/src/discover.rs`:

```rust
use std::path::{Path, PathBuf};

/// Directories that are never source, whatever `.gitignore` says.
const ALWAYS_SKIP: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".turbo",
    ".git",
    "vendor",
    "coverage",
];

/// Every source file under `root` matching one of `extensions`.
///
/// Respects `.gitignore`. Returns absolute paths, sorted, so two runs on an
/// unchanged tree produce byte-identical output.
pub fn discover(root: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .filter_entry(|entry| {
            entry
                .file_name()
                .to_str()
                .is_none_or(|name| !ALWAYS_SKIP.contains(&name))
        })
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| extensions.contains(&e))
        })
        .collect();

    found.sort();
    found
}
```

In `lib.rs`:

```rust
pub mod discover;

pub use discover::discover;
```

- [ ] **Step 4: run the tests and verify they pass**

```bash
cargo test -p kog-graph 2>&1 | tail -8
```

Expected: `test result: ok. 20 passed`.

- [ ] **Step 5: commit**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
git add -A && git commit -m "feat(discover): walk a project for source files"
```

---

## Task 6 — the TypeScript extractor

**Files:**
- Create: `crates/kog-graph/src/extractors/mod.rs`,
  `crates/kog-graph/src/extractors/typescript.rs`
- Modify: `crates/kog-graph/src/lib.rs`

**Interfaces:**
- Consumes: `Extractor`, `Specifier`, `Resolution`, `ExtractError` (task 3);
  `TsConfigIndex` (task 4).
- Produces: `TypeScriptExtractor::new(root: &Path) -> Self`, implementing `Extractor`.
  Consumed by task 7.

**tree-sitter API verified by compilation.** `LANGUAGE_TYPESCRIPT` and `LANGUAGE_TSX`
are `LanguageFn`: `(&lang.into())` gives a `Language`. `QueryCursor::matches`
returns a `StreamingIterator`, so `while let Some(m) = matches.next()` with
`use streaming_iterator::StreamingIterator;` in scope — a `for` loop does not compile.

- [ ] **Step 1: write the failing test**

`crates/kog-graph/src/extractors/mod.rs`:

```rust
pub mod typescript;

pub use typescript::TypeScriptExtractor;
```

At the end of `crates/kog-graph/src/extractors/typescript.rs`:

```rust
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

    fn raws(source: &str) -> Vec<String> {
        let dir = TempDir::new().unwrap();
        TypeScriptExtractor::new(dir.path())
            .extract(source)
            .unwrap()
            .into_iter()
            .map(|s| s.raw)
            .collect()
    }

    #[test]
    fn plain_imports_are_extracted() {
        let got = raws(r#"import React from "react";"#);
        assert_eq!(got, vec!["react"]);
    }

    #[test]
    fn type_only_imports_are_extracted_too() {
        let got = raws(r#"import type { T } from "@/types/user";"#);
        assert_eq!(got, vec!["@/types/user"]);
    }

    #[test]
    fn re_exports_are_extracted() {
        let got = raws(r#"export { x } from "@common/utils";
export * from "./barrel";"#);
        assert_eq!(got, vec!["@common/utils", "./barrel"]);
    }

    #[test]
    fn single_quotes_are_handled() {
        let got = raws("import a from './local';");
        assert_eq!(got, vec!["./local"]);
    }

    #[test]
    fn dynamic_imports_are_out_of_scope_in_v0() {
        let got = raws(r#"const m = await import("./dynamic");"#);
        assert!(got.is_empty());
    }

    #[test]
    fn a_syntactically_broken_file_still_yields_what_it_can() {
        // tree-sitter recovers from errors; extraction must not blow up.
        let got = raws(r#"import a from "./ok"; function ( { unclosed"#);
        assert_eq!(got, vec!["./ok"]);
    }

    #[test]
    fn the_line_number_is_one_based() {
        let dir = TempDir::new().unwrap();
        let specs = TypeScriptExtractor::new(dir.path())
            .extract("\n\nimport a from \"./x\";")
            .unwrap();
        assert_eq!(specs[0].line, 3);
    }

    #[test]
    fn a_relative_import_resolves_next_to_the_importer() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "src/b.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("./b", &dir.path().join("src/a.ts"));
        assert_eq!(got, Resolution::Internal(dir.path().join("src/b.ts")));
    }

    #[test]
    fn a_parent_relative_import_resolves() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/deep/a.ts", "");
        write(&dir, "src/b.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("../b", &dir.path().join("src/deep/a.ts"));
        assert_eq!(got, Resolution::Internal(dir.path().join("src/b.ts")));
    }

    #[test]
    fn a_directory_import_falls_back_to_index() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "src/lib/index.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("./lib", &dir.path().join("src/a.ts"));
        assert_eq!(got, Resolution::Internal(dir.path().join("src/lib/index.ts")));
    }

    #[test]
    fn a_js_specifier_falls_back_to_the_ts_file() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "src/b.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("./b.js", &dir.path().join("src/a.ts"));
        assert_eq!(got, Resolution::Internal(dir.path().join("src/b.ts")));
    }

    #[test]
    fn a_wildcard_alias_resolves_through_tsconfig() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#,
        );
        write(&dir, "src/lib/api.ts", "");
        write(&dir, "src/app/page.tsx", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("@/lib/api", &dir.path().join("src/app/page.tsx"));
        assert_eq!(got, Resolution::Internal(dir.path().join("src/lib/api.ts")));
    }

    #[test]
    fn an_exact_alias_resolves_to_its_single_target() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": {
                "@mastore/shared-types": ["./packages/shared-types/src/index.ts"]
            } } }"#,
        );
        write(&dir, "packages/shared-types/src/index.ts", "");
        write(&dir, "apps/web/a.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve("@mastore/shared-types", &dir.path().join("apps/web/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(dir.path().join("packages/shared-types/src/index.ts"))
        );
    }

    #[test]
    fn a_bare_package_name_is_external() {
        let dir = TempDir::new().unwrap();
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("react", &dir.path().join("a.ts")),
            Resolution::External("react".into())
        );
        assert_eq!(
            e.resolve("@nestjs/common", &dir.path().join("a.ts")),
            Resolution::External("@nestjs/common".into())
        );
    }

    #[test]
    fn an_alias_whose_target_is_missing_is_unresolved_not_external() {
        // Mirrors `@prisma/generated` on the reference monorepo: the mapping
        // exists but the generated directory has never been built.
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": {
                "@prisma/generated": ["./src/generated/prisma/browser"]
            } } }"#,
        );
        write(&dir, "src/a.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("@prisma/generated", &dir.path().join("src/a.ts")),
            Resolution::Unresolved
        );
    }

    #[test]
    fn a_relative_import_pointing_nowhere_is_unresolved() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("./ghost", &dir.path().join("src/a.ts")),
            Resolution::Unresolved
        );
    }

    #[test]
    fn an_asset_import_is_unresolved_and_counted() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("./logo.png", &dir.path().join("src/a.ts")),
            Resolution::Unresolved
        );
    }

    #[test]
    fn the_extractor_declares_its_language_and_extensions() {
        let dir = TempDir::new().unwrap();
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(e.lang(), "typescript");
        assert_eq!(e.extensions(), &["ts", "tsx", "js", "jsx", "mts", "cts"]);
    }
}
```

- [ ] **Step 2: run the test and verify it fails**

```bash
cargo test -p kog-graph typescript 2>&1 | tail -12
```

Expected: `cannot find type TypeScriptExtractor`.

- [ ] **Step 3: write the minimal implementation**

At the top of `crates/kog-graph/src/extractors/typescript.rs`:

```rust
use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::tsconfig::TsConfigIndex;
use std::path::{Path, PathBuf};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

/// Captures the string literal of every static import and re-export.
/// Dynamic `import()` and `require()` are deliberately out of scope in v0.
const IMPORT_QUERY: &str = r#"
(import_statement source: (string) @spec)
(export_statement source: (string) @spec)
"#;

/// Extension probes, in order, for a specifier that has none.
const EXTENSION_ORDER: &[&str] = &["ts", "tsx", "js", "jsx", "mts", "cts"];

pub struct TypeScriptExtractor {
    root: PathBuf,
    tsconfig: TsConfigIndex,
}

impl TypeScriptExtractor {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            tsconfig: TsConfigIndex::build(root),
        }
    }

    /// Probe a resolution candidate: exact file, then each extension,
    /// then the directory's index file.
    fn probe(&self, candidate: &Path) -> Option<PathBuf> {
        if candidate.is_file() {
            return Some(candidate.to_path_buf());
        }

        // `./b.js` is written by ESM/NodeNext style but `./b.ts` is on disk.
        if let Some(stem) = candidate
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| matches!(*e, "js" | "jsx" | "mjs" | "cjs"))
            .map(|_| candidate.with_extension(""))
        {
            for ext in EXTENSION_ORDER {
                let probed = stem.with_extension(ext);
                if probed.is_file() {
                    return Some(probed);
                }
            }
        }

        for ext in EXTENSION_ORDER {
            let mut probed = candidate.to_path_buf();
            let name = probed.file_name()?.to_str()?.to_string();
            probed.set_file_name(format!("{name}.{ext}"));
            if probed.is_file() {
                return Some(probed);
            }
        }

        if candidate.is_dir() {
            for ext in EXTENSION_ORDER {
                let probed = candidate.join(format!("index.{ext}"));
                if probed.is_file() {
                    return Some(probed);
                }
            }
        }

        None
    }

    /// Expand one alias mapping against a specifier, if it matches.
    fn expand(pattern: &str, target: &Path, raw: &str) -> Option<PathBuf> {
        match pattern.split_once('*') {
            // Wildcard mapping: `@/*` -> `./src/*`.
            Some((prefix, suffix)) => {
                let rest = raw.strip_prefix(prefix)?.strip_suffix(suffix)?;
                let target_str = target.to_str()?;
                Some(PathBuf::from(target_str.replace('*', rest)))
            }
            // Exact mapping: `@mastore/shared-types` -> one file.
            None if pattern == raw => Some(target.to_path_buf()),
            None => None,
        }
    }
}

impl Extractor for TypeScriptExtractor {
    fn lang(&self) -> &'static str {
        "typescript"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ts", "tsx", "js", "jsx", "mts", "cts"]
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        // TSX is a superset for import syntax, so one grammar covers both.
        let language: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TSX.into();

        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|_| ExtractError::Grammar("typescript"))?;
        let tree = parser.parse(source, None).ok_or(ExtractError::Parse)?;

        let query =
            Query::new(&language, IMPORT_QUERY).map_err(|_| ExtractError::Grammar("typescript"))?;

        let mut cursor = QueryCursor::new();
        // `matches` is a StreamingIterator: a `for` loop does not compile.
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

        let mut out = Vec::new();
        while let Some(m) = matches.next() {
            for capture in m.captures {
                let text = &source[capture.node.byte_range()];
                let raw = text.trim_matches(|c| c == '"' || c == '\'' || c == '`');
                if raw.is_empty() {
                    continue;
                }
                out.push(Specifier {
                    raw: raw.to_string(),
                    line: capture.node.start_position().row + 1,
                });
            }
        }
        Ok(out)
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        // 1. Relative.
        if raw.starts_with('.') {
            let base = match importer.parent() {
                Some(d) => d,
                None => return Resolution::Unresolved,
            };
            let candidate = crate::tsconfig::normalise_public(&base.join(raw));
            return match self.probe(&candidate) {
                Some(path) => Resolution::Internal(path),
                None => Resolution::Unresolved,
            };
        }

        // 2. tsconfig alias. A matching pattern means the author meant an
        //    internal file, so a miss is Unresolved, never External.
        let mut matched_an_alias = false;
        for mapping in self.tsconfig.mappings_for(importer) {
            for target in &mapping.targets {
                if let Some(candidate) = Self::expand(&mapping.pattern, target, raw) {
                    matched_an_alias = true;
                    if let Some(path) = self.probe(&candidate) {
                        return Resolution::Internal(path);
                    }
                }
            }
        }
        if matched_an_alias {
            return Resolution::Unresolved;
        }

        // 3. Anything else is a third-party package.
        let _ = &self.root;
        Resolution::External(raw.to_string())
    }
}
```

Expose `normalise` from `tsconfig.rs` by adding, below the private function:

```rust
/// Lexical normalisation, reused by the extractors.
pub fn normalise_public(path: &Path) -> PathBuf {
    normalise(path)
}
```

In `lib.rs`:

```rust
pub mod extractors;

pub use extractors::TypeScriptExtractor;
```

- [ ] **Step 4: run the tests and verify they pass**

```bash
cargo test -p kog-graph 2>&1 | tail -8
```

Expected: `test result: ok. 38 passed`.

- [ ] **Step 5: commit**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
git add -A && git commit -m "feat(typescript): extract and resolve import specifiers"
```

---

## Task 7 — graph assembly

**Files:**
- Create: `crates/kog-graph/src/graph.rs`
- Modify: `crates/kog-graph/src/lib.rs`

**Interfaces:**
- Consumes: `discover` (task 5), `Extractor` (task 3), `model` (task 2).
- Produces: `build_graph(root: &Path, extractor: &dyn Extractor) -> Graph`. Consumed
  by task 8.

- [ ] **Step 1: write the failing test**

At the end of `crates/kog-graph/src/graph.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractors::TypeScriptExtractor;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn build(dir: &TempDir) -> Graph {
        let extractor = TypeScriptExtractor::new(dir.path());
        build_graph(dir.path(), &extractor)
    }

    #[test]
    fn every_discovered_file_becomes_a_node() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import b from "./b";"#);
        write(&dir, "src/b.ts", "export const b = 1;");
        let graph = build(&dir);
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    fn node_ids_are_project_relative_and_slash_separated() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/deep/a.ts", "");
        let graph = build(&dir);
        assert_eq!(graph.nodes[0].id, "src/deep/a.ts");
    }

    #[test]
    fn external_packages_land_on_the_node_not_in_the_edges() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import React from "react";"#);
        let graph = build(&dir);
        assert_eq!(graph.edges.len(), 0);
        assert_eq!(graph.nodes[0].external_deps, vec!["react".to_string()]);
    }

    #[test]
    fn duplicate_edges_are_collapsed() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "src/a.ts",
            r#"import { x } from "./b";
import { y } from "./b";"#,
        );
        write(&dir, "src/b.ts", "");
        let graph = build(&dir);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    fn stats_count_internal_and_external_separately() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "src/a.ts",
            r#"import React from "react";
import b from "./b";
import ghost from "./ghost";"#,
        );
        write(&dir, "src/b.ts", "");
        let graph = build(&dir);
        assert_eq!(graph.stats.specifiers_total, 3);
        assert_eq!(graph.stats.specifiers_internal, 2);
        assert_eq!(graph.stats.resolved, 1);
        assert_eq!(graph.stats.unresolved, 1);
        assert_eq!(graph.stats.external_specifiers, 1);
        assert_eq!(graph.stats.external_packages_distinct, 1);
        assert!((graph.stats.resolution_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn loc_counts_the_lines_of_each_file() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "one\ntwo\nthree");
        let graph = build(&dir);
        assert_eq!(graph.nodes[0].loc, 3);
    }

    #[test]
    fn an_edge_pointing_outside_the_project_is_dropped_and_counted() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import x from "../../outside/x";"#);
        let graph = build(&dir);
        assert_eq!(graph.edges.len(), 0);
    }

    #[test]
    fn nodes_are_sorted_so_runs_are_reproducible() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/z.ts", "");
        write(&dir, "src/a.ts", "");
        let graph = build(&dir);
        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["src/a.ts", "src/z.ts"]);
    }
}
```

- [ ] **Step 2: run the test and verify it fails**

```bash
cargo test -p kog-graph graph 2>&1 | tail -12
```

Expected: `cannot find function build_graph`.

- [ ] **Step 3: write the minimal implementation**

At the top of `crates/kog-graph/src/graph.rs`:

```rust
use crate::discover::discover;
use crate::extractor::{Extractor, Resolution};
use crate::model::{Edge, EdgeKind, Failure, Graph, Node, Stats};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;

/// Project-relative, slash-separated identity for a file.
fn node_id(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Scan a project into a graph. Never panics on a bad file: it is recorded in
/// `stats.failures` and the scan continues.
pub fn build_graph(root: &Path, extractor: &dyn Extractor) -> Graph {
    let files = discover(root, extractor.extensions());

    let mut stats = Stats {
        files_discovered: files.len(),
        ..Default::default()
    };
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: HashSet<Edge> = HashSet::new();
    let mut external_names: BTreeSet<String> = BTreeSet::new();
    // A specifier may resolve to a file that was filtered out of the walk.
    let known: HashSet<String> = files
        .iter()
        .filter_map(|p| node_id(root, p))
        .collect();

    for file in &files {
        let id = match node_id(root, file) {
            Some(id) => id,
            None => continue,
        };

        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                stats.failures.push(Failure {
                    path: id.clone(),
                    reason: format!("read failed: {e}"),
                });
                continue;
            }
        };

        let specifiers = match extractor.extract(&source) {
            Ok(s) => s,
            Err(e) => {
                stats.failures.push(Failure {
                    path: id.clone(),
                    reason: format!("extract failed: {e}"),
                });
                continue;
            }
        };
        stats.files_parsed += 1;

        let mut node_externals: BTreeSet<String> = BTreeSet::new();

        for specifier in &specifiers {
            stats.specifiers_total += 1;
            match extractor.resolve(&specifier.raw, file) {
                Resolution::External(package) => {
                    stats.external_specifiers += 1;
                    external_names.insert(package.clone());
                    node_externals.insert(package);
                }
                Resolution::Internal(target) => {
                    stats.specifiers_internal += 1;
                    match node_id(root, &target).filter(|t| known.contains(t)) {
                        Some(target_id) => {
                            stats.resolved += 1;
                            if target_id != id {
                                edges.insert(Edge {
                                    source: id.clone(),
                                    target: target_id,
                                    kind: EdgeKind::Import,
                                });
                            }
                        }
                        // Resolved on disk but outside the scanned set.
                        None => stats.unresolved += 1,
                    }
                }
                Resolution::Unresolved => {
                    stats.specifiers_internal += 1;
                    stats.unresolved += 1;
                }
            }
        }

        nodes.push(Node {
            id: id.clone(),
            path: id,
            lang: extractor.lang().to_string(),
            loc: source.lines().count(),
            external_deps: node_externals.into_iter().collect(),
        });
    }

    stats.external_packages_distinct = external_names.len();
    stats.resolution_rate = stats.resolution_rate();

    let mut edges: Vec<Edge> = edges.into_iter().collect();
    edges.sort_by(|a, b| (&a.source, &a.target).cmp(&(&b.source, &b.target)));
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    Graph {
        nodes,
        edges,
        stats,
    }
}
```

In `lib.rs`:

```rust
pub mod graph;

pub use graph::build_graph;
```

- [ ] **Step 4: run the tests and verify they pass**

```bash
cargo test -p kog-graph 2>&1 | tail -8
```

Expected: `test result: ok. 46 passed`.

- [ ] **Step 5: commit**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
git add -A && git commit -m "feat(graph): assemble nodes, edges and resolution stats"
```

---

## Task 8 — the CLI

**Files:**
- Modify: `crates/kog-cli/src/main.rs`

**Interfaces:**
- Consumes: `build_graph`, `TypeScriptExtractor`, `Graph` (tasks 2, 6, 7).
- Produces: the `kog` binary with the `scan` subcommand.

- [ ] **Step 1: write the implementation**

`crates/kog-cli/src/main.rs`:

```rust
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use kog_graph::{build_graph, TypeScriptExtractor};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "kog", version, about = "Map a codebase into a graph")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan a project and emit its file/import graph.
    Scan {
        /// Project root to scan.
        root: PathBuf,
        /// Where to write the graph. Defaults to `graph.json`.
        #[arg(short, long, default_value = "graph.json")]
        output: PathBuf,
        /// Print the statistics only, write no file.
        #[arg(long)]
        stats_only: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan {
            root,
            output,
            stats_only,
        } => scan(root, output, stats_only),
    }
}

fn scan(root: PathBuf, output: PathBuf, stats_only: bool) -> Result<()> {
    // Fail loudly on an unreadable root: everything else is recoverable.
    if !root.is_dir() {
        bail!("{} is not a readable directory", root.display());
    }
    let root = root
        .canonicalize()
        .with_context(|| format!("cannot canonicalize {}", root.display()))?;

    let extractor = TypeScriptExtractor::new(&root);
    let graph = build_graph(&root, &extractor);
    let stats = &graph.stats;

    println!("root                   {}", root.display());
    println!("files discovered       {}", stats.files_discovered);
    println!("files parsed           {}", stats.files_parsed);
    println!("specifiers total       {}", stats.specifiers_total);
    println!("  internal             {}", stats.specifiers_internal);
    println!("    resolved           {}", stats.resolved);
    println!("    unresolved         {}", stats.unresolved);
    println!(
        "  external             {} ({} distinct packages)",
        stats.external_specifiers, stats.external_packages_distinct
    );
    println!("nodes                  {}", graph.nodes.len());
    println!("edges                  {}", graph.edges.len());
    println!("resolution rate        {:.4}", stats.resolution_rate);

    if !stats.failures.is_empty() {
        println!("failures               {}", stats.failures.len());
        for failure in stats.failures.iter().take(10) {
            println!("  {} — {}", failure.path, failure.reason);
        }
        if stats.failures.len() > 10 {
            println!("  … {} more", stats.failures.len() - 10);
        }
    }

    if !stats_only {
        let json = serde_json::to_string_pretty(&graph)?;
        std::fs::write(&output, json)
            .with_context(|| format!("cannot write {}", output.display()))?;
        println!("written                {}", output.display());
    }

    Ok(())
}
```

- [ ] **Step 2: verify the behaviour on the quick fixture**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo run -q -p kog-cli -- scan ~/apps/lueur --stats-only
```

Expected: `files discovered` close to 93, `resolution rate` displayed, no panic.

- [ ] **Step 3: verify a missing root fails hard**

```bash
cargo run -q -p kog-cli -- scan /definitely/not/real --stats-only; echo "exit=$?"
```

Expected: explicit error message and `exit=1`.

- [ ] **Step 4: commit**

```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
git add -A && git commit -m "feat(cli): add the scan command"
```

---

## Task 9 — the rendering page

**Files:**
- Create: `app/package.json`, `app/vite.config.ts`, `app/index.html`,
  `app/tsconfig.json`, `app/src/main.tsx`, `app/.gitignore`

**Interfaces:**
- Consumes: `app/public/graph.json`, produced by task 8.
- Produces: a page that renders the graph in WebGL.

- [ ] **Step 1: bootstrap the frontend**

```bash
cd ~/apps/mycelium
bun create vite app --template react-ts
cd app
bun install
bun add sigma graphology graphology-layout-forceatlas2
printf 'node_modules\ndist\npublic/graph.json\n' > .gitignore
```

- [ ] **Step 2: write the rendering**

`app/src/main.tsx`:

```tsx
import Graph from "graphology";
import forceAtlas2 from "graphology-layout-forceatlas2";
import Sigma from "sigma";

type KogNode = {
  id: string;
  path: string;
  lang: string;
  loc: number;
  external_deps: string[];
};

type KogEdge = { source: string; target: string; kind: string };

type KogGraph = {
  nodes: KogNode[];
  edges: KogEdge[];
  stats: { resolution_rate: number; files_discovered: number };
};

/** Stable colour per top-level directory, so clusters read at a glance. */
function colourFor(id: string): string {
  const top = id.split("/")[0];
  let hash = 0;
  for (let i = 0; i < top.length; i++) {
    hash = (hash * 31 + top.charCodeAt(i)) >>> 0;
  }
  return `hsl(${hash % 360}, 65%, 55%)`;
}

async function main(): Promise<void> {
  const container = document.getElementById("root");
  if (!container) throw new Error("missing #root");

  const response = await fetch("/graph.json");
  if (!response.ok) throw new Error(`graph.json: ${response.status}`);
  const data: KogGraph = await response.json();

  const graph = new Graph();
  for (const node of data.nodes) {
    graph.addNode(node.id, {
      label: node.id.split("/").pop() ?? node.id,
      size: 2,
      color: colourFor(node.id),
      x: Math.random(),
      y: Math.random(),
    });
  }
  for (const edge of data.edges) {
    if (graph.hasNode(edge.source) && graph.hasNode(edge.target)) {
      graph.mergeEdge(edge.source, edge.target, { size: 0.4, color: "#3a3a3a" });
    }
  }

  // Size by degree: hubs stand out without inflating the layout.
  graph.forEachNode((node) => {
    graph.setNodeAttribute(node, "size", 2 + Math.sqrt(graph.degree(node)) * 1.6);
  });

  forceAtlas2.assign(graph, {
    iterations: 300,
    settings: { ...forceAtlas2.inferSettings(graph), gravity: 0.6 },
  });

  new Sigma(graph, container as HTMLElement, {
    renderEdgeLabels: false,
    defaultEdgeColor: "#333",
  });

  const badge = document.createElement("div");
  badge.style.cssText =
    "position:fixed;top:12px;left:12px;font:13px ui-monospace,monospace;color:#ddd;background:#111c;padding:8px 12px;border-radius:6px";
  badge.textContent = `${graph.order} nodes · ${graph.size} edges · resolution ${(
    data.stats.resolution_rate * 100
  ).toFixed(1)}%`;
  document.body.appendChild(badge);
}

main().catch((error) => {
  document.body.innerHTML = `<pre style="color:#f66;padding:24px">${String(error)}</pre>`;
});
```

`app/index.html` — replace the body with:

```html
<body style="margin:0;background:#0d0d0f">
  <div id="root" style="width:100vw;height:100vh"></div>
  <script type="module" src="/src/main.tsx"></script>
</body>
```

Delete `app/src/App.tsx`, `app/src/App.css`, `app/src/index.css` and
`app/src/assets`: the page doesn't need them.

- [ ] **Step 3: produce a graph and display it**

```bash
cd ~/apps/mycelium
export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p app/public
cargo run -q -p kog-cli -- scan ~/apps/lueur -o app/public/graph.json
cd app && bun run dev
```

Open the displayed URL. Expected: a graph coloured by folder, the badge in the
top-left corner showing nodes, edges and the resolution rate.

- [ ] **Step 4: commit**

```bash
cd ~/apps/mycelium
git add -A && git commit -m "feat(app): render the graph with sigma"
```

---

## Task 10 — the acceptance gate

**Files:**
- Create: `docs/measurements/2026-08-06-v0-gate.md`

**Interfaces:**
- Consumes: everything above.
- Produces: the measured proof that v0 is done, or the list of what's missing.

- [ ] **Step 1: measure on the acceptance target**

```bash
cd ~/apps/mycelium
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release -q
time ./target/release/kog scan ~/Mastore/mastore-saas -o /tmp/saas-graph.json
```

Record: `files discovered`, `resolution rate`, `nodes`, `edges`, duration.

- [ ] **Step 2: check the gate**

```bash
jq -r '.stats | "rate=\(.resolution_rate)  internal=\(.specifiers_internal)  resolved=\(.resolved)  unresolved=\(.unresolved)"' /tmp/saas-graph.json
jq -e '.stats.resolution_rate >= 0.95' /tmp/saas-graph.json >/dev/null \
  && echo "GATE PASSED" || echo "GATE FAILED"
```

**If the gate fails**, do not adjust the threshold. Extract the unresolved specifiers
and categorize them before any fix:

```bash
./target/release/kog scan ~/Mastore/mastore-saas --stats-only 2>&1 | tail -20
```

Expected causes, in order of likelihood: an alias not picked up in the `extends`
chain; a misapplied `baseUrl`; a missing extension in `EXTENSION_ORDER`;
`@prisma/generated`, whose target doesn't exist on disk — this one is legitimate and
must not be "fixed".

- [ ] **Step 3: verify the rendering on the target**

```bash
cp /tmp/saas-graph.json app/public/graph.json
cd app && bun run dev
```

Expected: the ~727-file graph displays and stays smooth on pan and zoom.

- [ ] **Step 4: verify CI**

```bash
cd ~/apps/mycelium
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
gitleaks detect --no-banner 2>&1 | tail -3
```

Expected: all four commands succeed.

- [ ] **Step 5: record the measurement**

Write `docs/measurements/2026-08-06-v0-gate.md` with the **recorded** figures, never
estimated: project, files discovered, files parsed, specifiers total / internal /
resolved / unresolved, rate, nodes, edges, duration, and the categorized list of
unresolved specifiers. This document is what makes the public figure defensible.

- [ ] **Step 6: commit**

```bash
git add -A && git commit -m "docs: record the v0 acceptance measurement"
```

---

## Plan self-review

**Spec coverage:**

| Spec section | Task |
| --- | --- |
| §4 architecture, modules | 1–8 |
| §5 data model | 2 |
| §6 resolution rules | 4, 6 |
| §7 errors never silent | 4 (unreadable tsconfig), 7 (read/extract), 8 (missing root) |
| §8 tests per rule | 4, 5, 6, 7 |
| §9 acceptance gate | 10 |
| §3.3 a language's entry rule | 3 (trait), 10 (measurement) |
| §12 reuse from dejavu | 1 |

**Type consistency:** `Specifier { raw, line }`, `Resolution::{Internal, External,
Unresolved}`, `PathMapping { pattern, targets }`, `Stats::resolution_rate()` and the
`resolution_rate` field are used identically from tasks 2 through 8. `TsConfigIndex::build`
and `mappings_for` keep the same signature in tasks 4 and 6. `build_graph(root, &dyn
Extractor)` is identical in tasks 7 and 8.

**Points to watch during execution:**

1. `normalise_public` in task 6 is a workaround for a visibility detail. If the
   implementer prefers moving `normalise` into a shared `path` module, that's an
   acceptable improvement.
2. The expected test counts (3, 7, 15, 20, 38, 46) assume no additional test was
   added. A discrepancy isn't an error; a missing failure is.
3. The total of 46 tests stays under dejavu's 51. If task 10 reveals resolution gaps,
   the corresponding regression tests get added in task 6.
