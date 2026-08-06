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
