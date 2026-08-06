use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::tsconfig::{SkippedConfig, TsConfigIndex};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
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

/// Directory index probes, in order. Narrower than `EXTENSION_ORDER` on
/// purpose: design doc §6 specifies `<dir>/index.{ts,tsx,js,jsx}` for the
/// index-file fallback, no `mts`/`cts`.
const INDEX_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx"];

/// A local package declared in the root `package.json`'s `workspaces`
/// field (design doc §6, resolution rule 3).
struct WorkspacePackage {
    /// The package's own directory.
    dir: PathBuf,
    /// Best entry-point candidate for a bare import of the package. See
    /// [`TypeScriptExtractor::entry_point`] for the source-preferring
    /// priority order: this is either a real file already found on disk
    /// among the package's declared `types`/`exports`/`main` fields, or —
    /// if none of them resolved to anything — the bare package directory,
    /// so `probe`'s directory-index fallback still applies when `resolve`
    /// uses this field.
    entry: PathBuf,
}

pub struct TypeScriptExtractor {
    tsconfig: TsConfigIndex,
    workspace_packages: BTreeMap<String, WorkspacePackage>,
}

impl TypeScriptExtractor {
    pub fn new(root: &Path) -> Self {
        // `build_graph` canonicalises its own root before computing node ids
        // (mirroring `discover`'s internal canonicalisation), so this must
        // canonicalise the same way: every absolute path this extractor
        // hands back (alias targets, workspace-package entries) has to carry
        // the same canonical prefix, or `node_id`'s `strip_prefix` silently
        // fails and a genuinely resolved import is miscounted. Falls back to
        // the given path if canonicalisation fails (e.g. the root does not
        // exist yet) rather than panicking; downstream resolution then
        // simply finds nothing, same as today.
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let root = root.as_path();
        Self {
            tsconfig: TsConfigIndex::build(root),
            workspace_packages: Self::discover_workspace_packages(root),
        }
    }

    /// Probe a resolution candidate: exact file, then each extension,
    /// then the directory's index file. Never touches `self` — an
    /// associated function rather than a method so `entry_point` can call
    /// it while building `workspace_packages`, before `Self` exists.
    fn probe(candidate: &Path) -> Option<PathBuf> {
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
            for ext in INDEX_EXTENSIONS {
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

    /// Read and parse one JSON file. Any failure (missing, unreadable,
    /// malformed) is treated as "nothing usable here", never an error —
    /// mirrors how a broken tsconfig degrades its subtree instead of
    /// aborting the scan (design doc §7).
    fn read_json(path: &Path) -> Option<Value> {
        let text = fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Build the workspace package table from the root `package.json`'s
    /// `workspaces` field, once, at construction time. A missing or
    /// malformed root `package.json`, or one with no usable `workspaces`
    /// field, simply means there are no local workspace packages.
    fn discover_workspace_packages(root: &Path) -> BTreeMap<String, WorkspacePackage> {
        let mut packages = BTreeMap::new();

        let manifest = match Self::read_json(&root.join("package.json")) {
            Some(m) => m,
            None => return packages,
        };

        for pattern in Self::workspace_patterns(&manifest) {
            for dir in Self::expand_pattern(root, &pattern) {
                let package_manifest = match Self::read_json(&dir.join("package.json")) {
                    Some(m) => m,
                    None => continue,
                };
                let name = match package_manifest.get("name").and_then(Value::as_str) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let entry = Self::entry_point(&dir, &package_manifest);
                packages.insert(name, WorkspacePackage { dir, entry });
            }
        }

        packages
    }

    /// Both `workspaces` shapes: a bare array of glob patterns (npm/Yarn
    /// classic), or an object with a `packages` array (Yarn modern).
    /// Negation patterns (`!excluded`) are dropped rather than expanded,
    /// since we only ever add candidates, never remove them. Anything
    /// else, or a missing field, yields no patterns.
    fn workspace_patterns(manifest: &Value) -> Vec<String> {
        let field = match manifest.get("workspaces") {
            Some(f) => f,
            None => return Vec::new(),
        };
        let array = field
            .as_array()
            .or_else(|| field.get("packages").and_then(Value::as_array));
        match array {
            Some(entries) => entries
                .iter()
                .filter_map(Value::as_str)
                .filter(|p| !p.starts_with('!'))
                .map(str::to_string)
                .collect(),
            None => Vec::new(),
        }
    }

    /// Expand one glob pattern (a single `*` per path segment, e.g.
    /// `apps/*` or `packages/*`) to the directories it matches, confined to
    /// `root`.
    ///
    /// `workspaces` is JSON that ships with the scanned repository, so
    /// unlike the crate's other traversal (always through
    /// `ignore::WalkBuilder::new(root)`, which is structurally incapable of
    /// escaping), plain path joining here has no such guarantee: a literal
    /// `..` segment (or, in principle, an absolute one) would otherwise
    /// walk outside the project. Containment is checked after every
    /// segment, not just at the end, so an escaped intermediate directory
    /// is never even handed to a later wildcard segment's `read_dir`. A
    /// candidate that escapes is dropped silently — it is not recorded
    /// anywhere, because nothing was legitimately skipped: the pattern
    /// simply does not designate part of this project.
    fn expand_pattern(root: &Path, pattern: &str) -> Vec<PathBuf> {
        let root = crate::tsconfig::normalise_public(root);
        let mut current = vec![root.clone()];
        for segment in pattern.split('/').filter(|s| !s.is_empty()) {
            let mut next = Vec::new();
            for base in &current {
                if segment.contains('*') {
                    let entries = match fs::read_dir(base) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    for entry in entries.flatten() {
                        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                            continue;
                        };
                        if Self::is_excluded_workspace_dir(&name) {
                            continue;
                        }
                        if entry.file_type().is_ok_and(|t| t.is_dir())
                            && Self::segment_matches(segment, &name)
                        {
                            next.push(entry.path());
                        }
                    }
                } else if !Self::is_excluded_workspace_dir(segment) {
                    let candidate = base.join(segment);
                    if candidate.is_dir() {
                        next.push(candidate);
                    }
                }
            }
            current = next
                .into_iter()
                .map(|p| crate::tsconfig::normalise_public(&p))
                .filter(|p| p.starts_with(&root))
                .collect();
        }
        current
    }

    /// A single `*` wildcard within one path segment, e.g. `pkg-*`.
    fn segment_matches(pattern: &str, name: &str) -> bool {
        match pattern.split_once('*') {
            Some((prefix, suffix)) => {
                name.len() >= prefix.len() + suffix.len()
                    && name.starts_with(prefix)
                    && name.ends_with(suffix)
            }
            None => pattern == name,
        }
    }

    /// `node_modules`, or an actual hidden directory. Deliberately does
    /// *not* match `.`/`..`: those are governed by the containment check
    /// in `expand_pattern`, which stays the single mechanism responsible
    /// for rejecting path traversal.
    fn is_excluded_workspace_dir(name: &str) -> bool {
        name == "node_modules" || (name.starts_with('.') && name != "." && name != "..")
    }

    /// A package's best entry-point candidate.
    ///
    /// Kora graphs source, not build output — the original design ("resolved
    /// to its `main`/`exports`, else `index.ts`") is correct for a runtime
    /// resolver but wrong here: on the acceptance target,
    /// `@mastore/shared-types` declares `main` and `exports["."].import`
    /// both pointing at a gitignored `dist/index.js`, while
    /// `exports["."].types` (and the older top-level `types`) point at the
    /// real TypeScript source. Following `exports`/`main` literally landed
    /// 44 real imports of that package outside the scanned node set.
    ///
    /// Candidates are tried in this order, each falling through to the next
    /// when the shape is not understood *or* the candidate does not
    /// resolve to a real file on disk — a `types`/`typings` entry pointing
    /// at a `.d.ts` file is still source-tree-shaped (better than `dist/`),
    /// so it is taken as-is, no special-casing:
    /// 1. `exports["."].types` — the conditional-exports types entry.
    /// 2. top-level `types`, then `typings` (older spelling, same meaning).
    /// 3. `exports["."]` `import`/`default`, or the whole field as a
    ///    string — the shapes this extractor already understood.
    /// 4. top-level `main`.
    /// 5. `<dir>/index.{ts,tsx,js,jsx}`: not tried here — every explicit
    ///    candidate above missing on disk falls through to the bare
    ///    package directory, and `resolve`'s later `probe` call finds the
    ///    index file the same way it always has.
    fn entry_point(dir: &Path, manifest: &Value) -> PathBuf {
        let exports = manifest.get("exports");
        let candidates = [
            Self::exports_types_target(exports),
            Self::top_level_string(manifest, "types"),
            Self::top_level_string(manifest, "typings"),
            Self::exports_target(exports),
            Self::top_level_string(manifest, "main"),
        ];
        for candidate in candidates.into_iter().flatten() {
            let joined = crate::tsconfig::normalise_public(&dir.join(candidate));
            if let Some(resolved) = Self::probe(&joined) {
                return resolved;
            }
        }
        dir.to_path_buf()
    }

    /// `exports["."].types`: only that one shape (`exports["."]` an object
    /// with a `types` key whose value is a string). Anything else — no
    /// `exports`, no `.` key, `exports["."]` not an object, `types` not a
    /// string — yields `None` so `entry_point` falls through.
    fn exports_types_target(exports: Option<&Value>) -> Option<String> {
        exports?
            .get(".")?
            .get("types")?
            .as_str()
            .map(str::to_string)
    }

    /// A top-level string field, e.g. `types`, `typings`, or `main`.
    fn top_level_string(manifest: &Value, field: &str) -> Option<String> {
        manifest
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    /// Only the simple `exports` shapes: a string, or an object with a
    /// `.` key whose value is a string or has `import`/`default`.
    /// Anything more elaborate yields `None` — `entry_point` falls through
    /// to `main` rather than this function guessing wrong. Never mixed
    /// with `types`/`typings`: those are tried first, by `entry_point`,
    /// specifically because this shape prefers `import`/`default`
    /// (build output) over `types` (source) when both are present in the
    /// same `exports["."]` object.
    fn exports_target(exports: Option<&Value>) -> Option<String> {
        match exports? {
            Value::String(s) => Some(s.clone()),
            Value::Object(map) => match map.get(".")? {
                Value::String(s) => Some(s.clone()),
                Value::Object(inner) => inner
                    .get("import")
                    .or_else(|| inner.get("default"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                _ => None,
            },
            _ => None,
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
            return match Self::probe(&candidate) {
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
                    if let Some(path) = Self::probe(&candidate) {
                        return Resolution::Internal(path);
                    }
                }
            }
        }
        if matched_an_alias {
            return Resolution::Unresolved;
        }

        // 3. Workspace package (design doc §6, rule 3). A specifier equal
        //    to a local package name, or a deep import into one, means the
        //    author named a package that exists in this workspace, so a
        //    miss is Unresolved, never External — same fail-closed shape
        //    as the alias branch above.
        let mut matched_a_workspace_package = false;
        for (name, package) in &self.workspace_packages {
            let is_exact = raw == name;
            let deep_rest = raw
                .strip_prefix(name.as_str())
                .and_then(|rest| rest.strip_prefix('/'));
            if !is_exact && deep_rest.is_none() {
                continue; // `raw` has nothing to do with this package.
            }
            // The prefix matched syntactically, so this specifier is
            // committed to naming a local package from here on — a probe
            // miss below still means Unresolved, never falling through to
            // rule 4.
            matched_a_workspace_package = true;

            let candidate = if is_exact {
                Some(package.entry.clone())
            } else {
                // A deep import is not a relative import: confine it to
                // the package directory rather than letting `..` walk out
                // of it (unlike rule 1, which permits `..` in relative
                // imports by design). A candidate that escapes is treated
                // as a miss, not silently probed anyway.
                deep_rest
                    .map(|rest| crate::tsconfig::normalise_public(&package.dir.join(rest)))
                    .filter(|candidate| candidate.starts_with(&package.dir))
            };

            if let Some(candidate) = candidate {
                if let Some(path) = Self::probe(&candidate) {
                    return Resolution::Internal(path);
                }
            }
        }
        if matched_a_workspace_package {
            return Resolution::Unresolved;
        }

        // 4. Anything else is a third-party package.
        Resolution::External(raw.to_string())
    }

    fn skipped_configs(&self) -> &[SkippedConfig] {
        self.tsconfig.skipped()
    }
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

    /// `TypeScriptExtractor::new` canonicalises its root internally (fix
    /// round 1, finding 1), so any test that asserts an absolute
    /// `Resolution::Internal` path, or builds an `importer` that must match
    /// a tsconfig-alias scope via `mappings_for`'s prefix check, needs to
    /// work in those same canonical terms. `dir.path()` alone is not
    /// guaranteed to be canonical — on macOS `/var` symlinks to
    /// `/private/var`, and every `TempDir` inherits that — so tests that
    /// compare against or build on top of a resolved path use this instead.
    fn canonical(dir: &TempDir) -> PathBuf {
        dir.path().canonicalize().unwrap()
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
        let got = raws(
            r#"export { x } from "@common/utils";
export * from "./barrel";"#,
        );
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
        assert_eq!(
            got,
            Resolution::Internal(dir.path().join("src/lib/index.ts"))
        );
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
        let root = canonical(&dir);
        let got = e.resolve("@/lib/api", &root.join("src/app/page.tsx"));
        assert_eq!(got, Resolution::Internal(root.join("src/lib/api.ts")));
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
        let root = canonical(&dir);
        let got = e.resolve("@mastore/shared-types", &root.join("apps/web/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(root.join("packages/shared-types/src/index.ts"))
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
        // `importer` must be canonical too: `mappings_for` matches it
        // against the (now canonical) tsconfig scope via `starts_with`, and
        // a non-canonical importer would simply miss the scope entirely
        // rather than exercise the "alias matched, target missing" case
        // this test is actually about.
        let root = canonical(&dir);
        assert_eq!(
            e.resolve("@prisma/generated", &root.join("src/a.ts")),
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

    // --- Resolution rule 3: workspace packages (design doc §6) ---

    #[test]
    fn a_workspace_package_name_resolves_via_its_declared_main() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{ "workspaces": ["packages/*"] }"#);
        write(
            &dir,
            "packages/shared-types/package.json",
            r#"{ "name": "@mastore/shared-types", "main": "./src/index.ts" }"#,
        );
        write(&dir, "packages/shared-types/src/index.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let root = canonical(&dir);
        let got = e.resolve("@mastore/shared-types", &root.join("apps/web/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(root.join("packages/shared-types/src/index.ts"))
        );
    }

    #[test]
    fn exports_types_wins_over_exports_import_pointing_at_dist() {
        // The real shape on the acceptance target: `main` and
        // `exports["."].import` both point at a gitignored `dist/`, while
        // `exports["."].types` points at the real TypeScript source. Kora
        // graphs source, not build output, so `types` must win. **Load-
        // bearing**: see the fix report for the break/restore proof.
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{ "workspaces": ["packages/*"] }"#);
        write(
            &dir,
            "packages/shared-types/package.json",
            r#"{
                "name": "@mastore/shared-types",
                "main": "./dist/index.js",
                "exports": { ".": {
                    "types": "./src/index.ts",
                    "require": "./dist/index.js",
                    "import": "./dist/index.js"
                } }
            }"#,
        );
        write(&dir, "packages/shared-types/src/index.ts", "");
        write(&dir, "packages/shared-types/dist/index.js", "");
        let e = TypeScriptExtractor::new(dir.path());
        let root = canonical(&dir);
        let got = e.resolve("@mastore/shared-types", &root.join("apps/backend/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(root.join("packages/shared-types/src/index.ts"))
        );
    }

    #[test]
    fn a_top_level_types_field_wins_over_main_pointing_at_dist() {
        // Same intent, older/simpler shape: no `exports` at all, just a
        // top-level `types` alongside a `main` that points at build output.
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{ "workspaces": ["packages/*"] }"#);
        write(
            &dir,
            "packages/shared-types/package.json",
            r#"{
                "name": "@mastore/shared-types",
                "main": "./dist/index.js",
                "types": "./src/index.ts"
            }"#,
        );
        write(&dir, "packages/shared-types/src/index.ts", "");
        write(&dir, "packages/shared-types/dist/index.js", "");
        let e = TypeScriptExtractor::new(dir.path());
        let root = canonical(&dir);
        let got = e.resolve("@mastore/shared-types", &root.join("apps/backend/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(root.join("packages/shared-types/src/index.ts"))
        );
    }

    #[test]
    fn a_deep_import_into_a_workspace_package_resolves_to_the_file_inside_it() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{ "workspaces": ["packages/*"] }"#);
        write(
            &dir,
            "packages/shared-types/package.json",
            r#"{ "name": "@mastore/shared-types" }"#,
        );
        write(&dir, "packages/shared-types/utils.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let root = canonical(&dir);
        let got = e.resolve("@mastore/shared-types/utils", &root.join("apps/web/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(root.join("packages/shared-types/utils.ts"))
        );
    }

    #[test]
    fn a_workspace_package_without_a_declared_entry_falls_back_to_its_index_file() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{ "workspaces": ["packages/*"] }"#);
        write(
            &dir,
            "packages/shared-types/package.json",
            r#"{ "name": "@mastore/shared-types" }"#,
        );
        write(&dir, "packages/shared-types/index.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let root = canonical(&dir);
        let got = e.resolve("@mastore/shared-types", &root.join("apps/web/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(root.join("packages/shared-types/index.ts"))
        );
    }

    #[test]
    fn a_workspace_package_whose_entry_is_missing_is_unresolved_not_external() {
        // The package is declared and named, but its `main` was never
        // built — mirrors an unbuilt local package on a real monorepo.
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{ "workspaces": ["packages/*"] }"#);
        write(
            &dir,
            "packages/shared-types/package.json",
            r#"{ "name": "@mastore/shared-types", "main": "./dist/index.js" }"#,
        );
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("@mastore/shared-types", &dir.path().join("apps/web/a.ts")),
            Resolution::Unresolved
        );
    }

    #[test]
    fn a_bare_specifier_that_is_not_a_workspace_package_is_still_external() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{ "workspaces": ["packages/*"] }"#);
        write(
            &dir,
            "packages/shared-types/package.json",
            r#"{ "name": "@mastore/shared-types" }"#,
        );
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("react", &dir.path().join("apps/web/a.ts")),
            Resolution::External("react".into())
        );
    }

    #[test]
    fn a_tsconfig_alias_wins_over_a_workspace_package_when_both_cover_the_specifier() {
        // On the acceptance target, cross-package imports like
        // `@mastore/shared-types` are covered by *both* a tsconfig alias
        // and a workspace package; rule 2 (alias) must keep winning over
        // rule 3 (workspace package) — this is the ordering guarantee the
        // acceptance target depends on.
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": {
                "@mastore/shared-types": ["./packages/shared-types/src/index.ts"]
            } } }"#,
        );
        write(&dir, "packages/shared-types/src/index.ts", "");
        write(&dir, "package.json", r#"{ "workspaces": ["packages/*"] }"#);
        write(
            &dir,
            "packages/shared-types/package.json",
            r#"{ "name": "@mastore/shared-types", "main": "./dist/index.js" }"#,
        );
        write(&dir, "packages/shared-types/dist/index.js", "");
        let e = TypeScriptExtractor::new(dir.path());
        let root = canonical(&dir);
        let got = e.resolve("@mastore/shared-types", &root.join("apps/web/a.ts"));
        // The tsconfig alias target, not the workspace package's `main`.
        assert_eq!(
            got,
            Resolution::Internal(root.join("packages/shared-types/src/index.ts"))
        );
    }

    #[test]
    fn workspaces_declared_as_an_object_with_a_packages_array_is_supported() {
        // The Yarn-modern shape: `{ "workspaces": { "packages": [...] } }`.
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "package.json",
            r#"{ "workspaces": { "packages": ["packages/*"], "nohoist": [] } }"#,
        );
        write(
            &dir,
            "packages/shared-types/package.json",
            r#"{ "name": "@mastore/shared-types" }"#,
        );
        write(&dir, "packages/shared-types/index.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let root = canonical(&dir);
        let got = e.resolve("@mastore/shared-types", &root.join("apps/web/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(root.join("packages/shared-types/index.ts"))
        );
    }

    #[test]
    fn a_missing_root_package_json_does_not_break_resolution() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("react", &dir.path().join("src/a.ts")),
            Resolution::External("react".into())
        );
    }

    #[test]
    fn a_malformed_root_package_json_does_not_break_resolution() {
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", "{ this is not json at all ");
        write(&dir, "src/a.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("react", &dir.path().join("src/a.ts")),
            Resolution::External("react".into())
        );
    }

    // --- Containment: a `workspaces` pattern cannot walk outside root ---

    #[test]
    fn a_workspaces_pattern_escaping_the_root_via_dot_dot_is_rejected() {
        // Outside `project/` (the scanned root) sits a real package with a
        // real entry point. A `workspaces` pattern that reaches it via
        // `..` must not register it: the specifier stays External, never
        // Internal into a location outside the scanned tree.
        let outer = TempDir::new().unwrap();
        write(
            &outer,
            "project/package.json",
            r#"{ "workspaces": ["../secret-package"] }"#,
        );
        write(
            &outer,
            "secret-package/package.json",
            r#"{ "name": "@evil/pkg" }"#,
        );
        write(&outer, "secret-package/index.ts", "");
        let root = outer.path().join("project");
        let e = TypeScriptExtractor::new(&root);
        assert_eq!(
            e.resolve("@evil/pkg", &root.join("src/a.ts")),
            Resolution::External("@evil/pkg".into())
        );
    }

    #[test]
    fn an_absolute_workspaces_pattern_is_rejected() {
        let outer = TempDir::new().unwrap();
        write(
            &outer,
            "secret-package/package.json",
            r#"{ "name": "@evil/pkg" }"#,
        );
        write(&outer, "secret-package/index.ts", "");
        let secret_dir = outer.path().join("secret-package");
        write(
            &outer,
            "project/package.json",
            &format!(r#"{{ "workspaces": ["{}"] }}"#, secret_dir.display()),
        );
        let root = outer.path().join("project");
        let e = TypeScriptExtractor::new(&root);
        assert_eq!(
            e.resolve("@evil/pkg", &root.join("src/a.ts")),
            Resolution::External("@evil/pkg".into())
        );
    }

    #[test]
    fn a_workspaces_pattern_matching_inside_node_modules_yields_no_package() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "package.json",
            r#"{ "workspaces": ["node_modules/*"] }"#,
        );
        write(
            &dir,
            "node_modules/evil-pkg/package.json",
            r#"{ "name": "@evil/pkg" }"#,
        );
        write(&dir, "node_modules/evil-pkg/index.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        assert_eq!(
            e.resolve("@evil/pkg", &dir.path().join("src/a.ts")),
            Resolution::External("@evil/pkg".into())
        );
    }

    #[test]
    fn a_deep_import_escaping_the_package_directory_does_not_resolve_outside_it() {
        // `secret.ts` genuinely exists two levels above the package
        // directory; a naive join would find it. Confinement must refuse
        // to look there.
        let dir = TempDir::new().unwrap();
        write(&dir, "package.json", r#"{ "workspaces": ["packages/*"] }"#);
        write(
            &dir,
            "packages/shared-types/package.json",
            r#"{ "name": "@mastore/shared-types" }"#,
        );
        write(&dir, "secret.ts", "");
        let e = TypeScriptExtractor::new(dir.path());
        let got = e.resolve(
            "@mastore/shared-types/../../secret",
            &dir.path().join("apps/web/a.ts"),
        );
        assert_eq!(got, Resolution::Unresolved);
    }
}
