use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::tsconfig::TsConfigIndex;
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
    /// Best entry-point candidate for a bare import of the package: its
    /// declared `exports`/`main` (normalised, may not exist on disk), or
    /// the package directory itself so `probe`'s directory-index fallback
    /// applies.
    entry: PathBuf,
}

pub struct TypeScriptExtractor {
    tsconfig: TsConfigIndex,
    workspace_packages: BTreeMap<String, WorkspacePackage>,
}

impl TypeScriptExtractor {
    pub fn new(root: &Path) -> Self {
        Self {
            tsconfig: TsConfigIndex::build(root),
            workspace_packages: Self::discover_workspace_packages(root),
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
    /// `apps/*` or `packages/*`) to the directories it matches.
    /// `node_modules` and hidden entries are never candidates.
    fn expand_pattern(root: &Path, pattern: &str) -> Vec<PathBuf> {
        let mut current = vec![root.to_path_buf()];
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
                        if name == "node_modules" || name.starts_with('.') {
                            continue;
                        }
                        if entry.file_type().is_ok_and(|t| t.is_dir())
                            && Self::segment_matches(segment, &name)
                        {
                            next.push(entry.path());
                        }
                    }
                } else {
                    let candidate = base.join(segment);
                    if candidate.is_dir() {
                        next.push(candidate);
                    }
                }
            }
            current = next;
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

    /// A package's best entry-point candidate: its declared
    /// `exports`/`main`, or the package directory itself so `probe`'s
    /// directory-index fallback (`index.{ts,tsx,js,jsx}`) applies.
    fn entry_point(dir: &Path, manifest: &Value) -> PathBuf {
        let target = Self::exports_target(manifest.get("exports")).or_else(|| {
            manifest
                .get("main")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        match target {
            Some(t) => crate::tsconfig::normalise_public(&dir.join(t)),
            None => dir.to_path_buf(),
        }
    }

    /// Only the simple `exports` shapes: a string, or an object with a
    /// `.` key whose value is a string or has `import`/`default`.
    /// Anything more elaborate falls through to `main` instead of
    /// guessing wrong — v0 does not need a full exports resolver.
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

        // 3. Workspace package (design doc §6, rule 3). A specifier equal
        //    to a local package name, or a deep import into one, means the
        //    author named a package that exists in this workspace, so a
        //    miss is Unresolved, never External — same fail-closed shape
        //    as the alias branch above.
        let mut matched_a_workspace_package = false;
        for (name, package) in &self.workspace_packages {
            let candidate = if raw == name {
                Some(package.entry.clone())
            } else {
                raw.strip_prefix(name.as_str())
                    .and_then(|rest| rest.strip_prefix('/'))
                    .map(|rest| package.dir.join(rest))
            };
            if let Some(candidate) = candidate {
                matched_a_workspace_package = true;
                if let Some(path) = self.probe(&candidate) {
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
        let got = e.resolve("@mastore/shared-types", &dir.path().join("apps/web/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(dir.path().join("packages/shared-types/src/index.ts"))
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
        let got = e.resolve(
            "@mastore/shared-types/utils",
            &dir.path().join("apps/web/a.ts"),
        );
        assert_eq!(
            got,
            Resolution::Internal(dir.path().join("packages/shared-types/utils.ts"))
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
        let got = e.resolve("@mastore/shared-types", &dir.path().join("apps/web/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(dir.path().join("packages/shared-types/index.ts"))
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
        let got = e.resolve("@mastore/shared-types", &dir.path().join("apps/web/a.ts"));
        // The tsconfig alias target, not the workspace package's `main`.
        assert_eq!(
            got,
            Resolution::Internal(dir.path().join("packages/shared-types/src/index.ts"))
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
        let got = e.resolve("@mastore/shared-types", &dir.path().join("apps/web/a.ts"));
        assert_eq!(
            got,
            Resolution::Internal(dir.path().join("packages/shared-types/index.ts"))
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
}
