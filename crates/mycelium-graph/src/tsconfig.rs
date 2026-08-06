use crate::build_walker;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsStr;
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

/// A config file that was found but could not be used — for this index,
/// always a tsconfig. Never fatal: the subtree it would have governed falls
/// back to weaker resolution, but the reason is never lost.
///
/// The shape (`path`, `reason`) is language-neutral, which is why
/// `Extractor::skipped_configs` returns it as the general "one config file
/// skipped, and why" type: it lives in this module because `TsConfigIndex`
/// is its only producer today, not because the concept is tsconfig-specific.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedConfig {
    pub path: PathBuf,
    pub reason: String,
}

/// Every tsconfig in the project, keyed by the directory it governs.
#[derive(Debug, Default)]
pub struct TsConfigIndex {
    /// Sorted by descending directory depth, so the nearest config wins.
    scopes: Vec<(PathBuf, Vec<PathMapping>)>,
    /// Configs that were found but could not be used, with why. Sorted by
    /// path for reproducible output, and deduplicated: a config reachable
    /// through more than one `extends` chain, or both a same-directory loser
    /// and an `extends` target, appears here exactly once.
    skipped: Vec<SkippedConfig>,
}

impl TsConfigIndex {
    /// Walk the project, load every `tsconfig*.json`, resolve `extends` chains.
    ///
    /// An unreadable or malformed config is skipped, never fatal: the subtree
    /// it governs falls back to relative-only resolution, and the reason is
    /// recorded in [`TsConfigIndex::skipped`].
    pub fn build(root: &Path) -> Self {
        let walker = build_walker(root).build();

        // Group candidates by directory first: a directory can hold more than
        // one `tsconfig*.json` (e.g. `tsconfig.json` and `tsconfig.build.json`
        // side by side), and only one of them may govern that directory.
        let mut by_dir: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
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
            by_dir.entry(dir).or_default().push(path.to_path_buf());
        }

        let mut scopes: Vec<(PathBuf, Vec<PathMapping>)> = Vec::new();
        let mut skipped: Vec<SkippedConfig> = Vec::new();

        for (dir, mut candidates) in by_dir {
            // Deterministic tie-break: `tsconfig.json` wins if present in the
            // directory; otherwise the candidates are ordered by file name so
            // the winner never depends on filesystem readdir order.
            candidates.sort_by(|a, b| {
                let a_is_default = a.file_name() == Some(OsStr::new("tsconfig.json"));
                let b_is_default = b.file_name() == Some(OsStr::new("tsconfig.json"));
                match (a_is_default, b_is_default) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.file_name().cmp(&b.file_name()),
                }
            });
            // `by_dir` only ever gains an entry via a push, so every group
            // has at least one candidate.
            let (winner, losers) = candidates
                .split_first()
                .expect("a directory group is never empty");
            // Losing siblings never contribute mappings or feed their own
            // `extends` chain into the index, but a read/parse failure on one
            // must still be surfaced: otherwise a broken sibling next to a
            // healthy `tsconfig.json` would disappear without a trace.
            for loser in losers {
                if let Some(skip) = Self::validate_config(loser) {
                    skipped.push(skip);
                }
            }
            let mappings = Self::mappings_from_chain(winner, 0, &mut skipped);
            if !mappings.is_empty() {
                scopes.push((dir, mappings));
            }
        }

        // Deepest directory first: `mappings_for` returns on the first prefix hit.
        scopes.sort_by_key(|(dir, _)| std::cmp::Reverse(dir.components().count()));
        // The same broken config can be reached more than once — as a loser
        // in its directory *and* as an `extends` target of its winning
        // sibling, or from more than one `extends` chain across different
        // directories entirely. Sorting by path first makes every visit to
        // the same file adjacent, so `dedup_by` collapses them to one entry,
        // keeping the first reason encountered rather than piling up
        // repeats of the same diagnosis.
        skipped.sort_by(|a, b| a.path.cmp(&b.path));
        skipped.dedup_by(|a, b| a.path == b.path);
        Self { scopes, skipped }
    }

    /// Read a tsconfig and parse it into its raw representation, or describe
    /// why that failed. This is the one place that defines what "unreadable"
    /// and "malformed" mean for a tsconfig, shared by
    /// [`TsConfigIndex::mappings_from_chain`] (which goes on to collect
    /// mappings) and [`TsConfigIndex::validate_config`] (which only checks
    /// for failure), so the two can never drift apart.
    fn read_and_parse(config_path: &Path) -> Result<RawTsConfig, SkippedConfig> {
        let text = std::fs::read_to_string(config_path).map_err(|err| SkippedConfig {
            path: config_path.to_path_buf(),
            reason: err.to_string(),
        })?;
        jsonc_parser::parse_to_serde_value::<RawTsConfig>(
            &text,
            &jsonc_parser::ParseOptions::default(),
        )
        .map_err(|err| SkippedConfig {
            path: config_path.to_path_buf(),
            reason: err.to_string(),
        })
    }

    /// Collect mappings from this config and everything it extends.
    /// The child's own mappings come first so they take precedence.
    ///
    /// Any config that cannot be read or fails to parse is recorded in
    /// `skipped` (with the underlying error as the reason) and contributes no
    /// mappings; a config that parses fine but simply declares no `paths` is
    /// not a skip.
    fn mappings_from_chain(
        config_path: &Path,
        depth: usize,
        skipped: &mut Vec<SkippedConfig>,
    ) -> Vec<PathMapping> {
        // Guards against a cyclic or absurdly deep `extends` chain.
        if depth > 16 {
            return Vec::new();
        }
        let raw = match Self::read_and_parse(config_path) {
            Ok(raw) => raw,
            Err(skip) => {
                skipped.push(skip);
                return Vec::new();
            }
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
                mappings.extend(Self::mappings_from_chain(&parent_path, depth + 1, skipped));
            }
        }

        mappings
    }

    /// Check whether a candidate config can be read and parsed, without
    /// collecting its mappings or following its `extends` chain.
    ///
    /// Used only for configs that lost the same-directory tie-break: their
    /// content must never enter the index, but a read or parse failure on
    /// them must still be recorded, or a broken sibling would vanish
    /// without a trace next to a healthy winner. `None` means the config is
    /// valid (whether or not it declares any `paths`); that is not a skip.
    fn validate_config(config_path: &Path) -> Option<SkippedConfig> {
        Self::read_and_parse(config_path).err()
    }

    /// Mappings that apply to a file, nearest enclosing tsconfig first.
    ///
    /// When more than one `tsconfig*.json` governs the very same directory,
    /// only one is used: the file literally named `tsconfig.json` wins; if no
    /// candidate has that name, they are ordered by file name so the choice
    /// is deterministic rather than dependent on filesystem enumeration
    /// order. See [`TsConfigIndex::build`].
    pub fn mappings_for(&self, importer: &Path) -> &[PathMapping] {
        for (dir, mappings) in &self.scopes {
            if importer.starts_with(dir) {
                return mappings;
            }
        }
        &[]
    }

    /// Configs that were found but could not be used, with why. Sorted by
    /// path, and deduplicated so a config reachable more than once — as a
    /// same-directory loser and an `extends` target, or from more than one
    /// `extends` chain — appears here exactly once.
    pub fn skipped(&self) -> &[SkippedConfig] {
        &self.skipped
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

/// Lexical normalisation, reused by the extractors.
pub fn normalise_public(path: &Path) -> PathBuf {
    normalise(path)
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
    fn when_child_and_parent_share_a_pattern_the_childs_mapping_comes_first() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.base.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./base-src/*"] } } }"#,
        );
        write(
            &dir,
            "apps/api/tsconfig.json",
            r#"{
                "extends": "../../tsconfig.base.json",
                "compilerOptions": { "paths": { "@/*": ["./src/*"] } }
            }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        let mappings = index.mappings_for(&dir.path().join("apps/api/src/a.ts"));
        let matching: Vec<&PathMapping> = mappings.iter().filter(|m| m.pattern == "@/*").collect();
        assert_eq!(matching.len(), 2);
        // The resolver (task 6) takes the first candidate that resolves to a
        // real file, so ordering here is load-bearing, not incidental.
        assert_eq!(matching[0].targets[0], dir.path().join("apps/api/src/*"));
        assert_eq!(matching[1].targets[0], dir.path().join("base-src/*"));
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
    fn when_two_configs_share_a_directory_tsconfig_json_wins() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./from-tsconfig-json/*"] } } }"#,
        );
        write(
            &dir,
            "tsconfig.build.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./from-build-json/*"] } } }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        let mappings = index.mappings_for(&dir.path().join("a.ts"));
        assert_eq!(
            mappings[0].targets[0],
            dir.path().join("from-tsconfig-json/*")
        );
    }

    #[test]
    fn when_neither_shared_config_is_named_tsconfig_json_the_alphabetical_one_wins() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.a.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./from-a/*"] } } }"#,
        );
        write(
            &dir,
            "tsconfig.b.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./from-b/*"] } } }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        let mappings = index.mappings_for(&dir.path().join("a.ts"));
        assert_eq!(mappings[0].targets[0], dir.path().join("from-a/*"));
    }

    #[test]
    fn a_malformed_sibling_config_is_recorded_without_affecting_the_winner() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#,
        );
        write(&dir, "tsconfig.local.json", "{ this is not json at all ");
        let index = TsConfigIndex::build(dir.path());

        // 1. The valid config's mapping still resolves through `mappings_for`.
        let mappings = index.mappings_for(&dir.path().join("a.ts"));
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].targets[0], dir.path().join("src/*"));

        // 2. The malformed sibling appears in `skipped()` with a non-empty reason.
        assert_eq!(index.skipped().len(), 1);
        assert_eq!(
            index.skipped()[0].path,
            dir.path().join("tsconfig.local.json")
        );
        assert!(!index.skipped()[0].reason.is_empty());

        // 3. The malformed sibling contributes no mapping: the only mapping
        // present is `tsconfig.json`'s own `@/*`, already checked above.
    }

    #[test]
    fn a_winner_extending_its_own_malformed_sibling_is_recorded_once() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{
                "extends": "./tsconfig.local.json",
                "compilerOptions": { "paths": { "@/*": ["./src/*"] } }
            }"#,
        );
        write(&dir, "tsconfig.local.json", "{ this is not json at all ");
        let index = TsConfigIndex::build(dir.path());

        // The winner's own paths still resolve.
        let mappings = index.mappings_for(&dir.path().join("a.ts"));
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].targets[0], dir.path().join("src/*"));

        // `tsconfig.local.json` is reached twice: once as the losing
        // same-directory sibling (via `validate_config`), once as the
        // winner's own `extends` target (via `mappings_from_chain`). Without
        // deduplication this would be 2.
        assert_eq!(index.skipped().len(), 1);
        assert_eq!(
            index.skipped()[0].path,
            dir.path().join("tsconfig.local.json")
        );
        assert!(!index.skipped()[0].reason.is_empty());
    }

    #[test]
    fn a_malformed_config_reached_from_two_extends_chains_is_recorded_once() {
        let dir = TempDir::new().unwrap();
        write(&dir, "tsconfig.shared.json", "{ this is not json at all ");
        write(
            &dir,
            "apps/a/tsconfig.json",
            r#"{
                "extends": "../../tsconfig.shared.json",
                "compilerOptions": { "paths": { "@a/*": ["./src/*"] } }
            }"#,
        );
        write(
            &dir,
            "apps/b/tsconfig.json",
            r#"{
                "extends": "../../tsconfig.shared.json",
                "compilerOptions": { "paths": { "@b/*": ["./src/*"] } }
            }"#,
        );
        let index = TsConfigIndex::build(dir.path());

        // Both apps still resolve their own paths.
        assert_eq!(
            index
                .mappings_for(&dir.path().join("apps/a/src/x.ts"))
                .len(),
            1
        );
        assert_eq!(
            index
                .mappings_for(&dir.path().join("apps/b/src/x.ts"))
                .len(),
            1
        );

        // `tsconfig.shared.json` is reached from three directions here (its
        // own top-level discovery, and each app's `extends`); without
        // deduplication it would appear more than once.
        let shared_skips: Vec<&SkippedConfig> = index
            .skipped()
            .iter()
            .filter(|s| s.path == dir.path().join("tsconfig.shared.json"))
            .collect();
        assert_eq!(shared_skips.len(), 1);
    }

    #[test]
    fn a_malformed_tsconfig_is_skipped_without_panicking() {
        let dir = TempDir::new().unwrap();
        write(&dir, "tsconfig.json", "{ this is not json at all ");
        let index = TsConfigIndex::build(dir.path());
        assert!(index.mappings_for(&dir.path().join("a.ts")).is_empty());
        assert_eq!(index.skipped().len(), 1);
        assert_eq!(index.skipped()[0].path, dir.path().join("tsconfig.json"));
        assert!(!index.skipped()[0].reason.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_tsconfig_that_cannot_be_read_is_skipped_without_panicking() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#,
        );
        let path = dir.path().join("tsconfig.json");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();

        let index = TsConfigIndex::build(dir.path());

        // Restore permissions before any assertion can panic, so TempDir's
        // Drop can still clean up the directory.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        assert!(index.mappings_for(&dir.path().join("a.ts")).is_empty());
        assert_eq!(index.skipped().len(), 1);
        assert_eq!(index.skipped()[0].path, path);
        assert!(!index.skipped()[0].reason.is_empty());
    }

    #[test]
    fn a_tsconfig_with_no_paths_is_not_recorded_as_skipped() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "moduleResolution": "bundler" } }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        assert!(index.skipped().is_empty());
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
        // The missing extends target is itself an unreadable config: it must
        // be surfaced, not silently absorbed.
        assert_eq!(index.skipped().len(), 1);
        assert_eq!(
            index.skipped()[0].path,
            dir.path().join("does-not-exist.json")
        );
        assert!(!index.skipped()[0].reason.is_empty());
    }

    #[test]
    fn a_cyclic_extends_chain_terminates_instead_of_hanging() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{
                "extends": "./child/tsconfig.json",
                "compilerOptions": { "paths": { "@root/*": ["./root/*"] } }
            }"#,
        );
        write(
            &dir,
            "child/tsconfig.json",
            r#"{
                "extends": "../tsconfig.json",
                "compilerOptions": { "paths": { "@child/*": ["./src/*"] } }
            }"#,
        );
        let index = TsConfigIndex::build(dir.path());
        // The depth guard must stop the cycle: this call has to return
        // (not hang, not overflow the stack) while still carrying whatever
        // mappings were legitimately collected before the cutoff.
        let mappings = index.mappings_for(&dir.path().join("child/src/a.ts"));
        assert!(!mappings.is_empty());
        assert!(mappings.len() < 100);
    }

    #[test]
    fn a_tsconfig_inside_node_modules_is_never_indexed() {
        // Even without a .git directory, node_modules should be excluded.
        // This proves the ALWAYS_SKIP list is active.
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@root/*": ["./src/*"] } } }"#,
        );
        write(
            &dir,
            "node_modules/package/tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@pkg/*": ["./lib/*"] } } }"#,
        );
        // Note: no .git or .gitignore exists. If ALWAYS_SKIP is not applied,
        // the node_modules config would be indexed.
        let index = TsConfigIndex::build(dir.path());
        // Only the root config should be indexed.
        let root_mappings = index.mappings_for(&dir.path().join("src/a.ts"));
        assert_eq!(root_mappings.len(), 1);
        assert_eq!(root_mappings[0].pattern, "@root/*");

        // Prove that the node_modules config was never discovered at all.
        assert!(!index
            .skipped()
            .iter()
            .any(|s| s.path.components().any(|c| c.as_os_str() == "node_modules")));
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
