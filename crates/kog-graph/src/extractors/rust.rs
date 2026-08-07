//! Rust: a module tree that is declared, not inferred.
//!
//! Two constructs point at a file. `mod foo;` names one literally — it is
//! the only import form in any supported language that *is* a filesystem
//! path by definition, so it either resolves or the crate does not compile.
//! `use` names a module path, which this resolver maps back to a file
//! through the conventional layout every crate uses: `src/a.rs` is
//! `crate::a`, `src/a/mod.rs` is `crate::a`, `src/a/b.rs` is `crate::a::b`.
//!
//! A `use` path usually ends in an item — a struct, a function, a trait —
//! not a module, so resolution takes the longest prefix that names a real
//! module file. `use crate::model::Node` resolves to `src/model.rs`, which
//! is the edge that was meant.

use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::extractors::support;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `!body` matches only a module *declaration* — `mod foo;` pointing at
/// another file — never `mod tests { … }`, which declares no file and would
/// otherwise produce an unresolvable specifier in every test module in the
/// codebase.
const IMPORT_QUERY: &str = r#"
(use_declaration argument: (_) @use)
(mod_item !body name: (identifier) @mod)
"#;

const EXTENSIONS: &[&str] = &["rs"];

/// Prefix marking a specifier that came from `mod foo;` rather than `use`.
/// Carried in the specifier text itself so the diagnostic a reader sees
/// (`src/lib.rs:3 [unresolved: not_found] mod ghost`) says which construct
/// failed, without a second field on every specifier in every language.
const MOD_PREFIX: &str = "mod ";

/// One crate in the scanned tree, with its module path index.
struct CrateInfo {
    /// The package name with `-` normalised to `_`, which is how it is
    /// written in a `use` path.
    name: String,
    src: PathBuf,
    /// Module path (`a::b`, empty string for the crate root) to the file
    /// that defines it.
    modules: BTreeMap<String, PathBuf>,
}

pub struct RustExtractor {
    crates: Vec<CrateInfo>,
}

impl RustExtractor {
    pub fn new(root: &Path) -> Self {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let crates = crate::discover::survey(&root)
            .files
            .iter()
            .filter(|p| p.file_name().is_some_and(|n| n == "Cargo.toml"))
            .filter_map(|manifest| {
                let dir = manifest.parent()?;
                let name = Self::package_name(manifest)?;
                let src = dir.join("src");
                if !src.is_dir() {
                    return None; // A virtual workspace manifest has no code.
                }
                Some(CrateInfo {
                    name,
                    modules: Self::index_modules(&src),
                    src,
                })
            })
            .collect();
        Self { crates }
    }

    /// `[package] name`. A name inherited from the workspace
    /// (`name = { workspace = true }`) is not a string, and yields `None`:
    /// the crate is then simply not addressable by name, which costs one
    /// resolution rule rather than producing a wrong one.
    fn package_name(manifest: &Path) -> Option<String> {
        let text = std::fs::read_to_string(manifest).ok()?;
        // `Table`, not `Value`: parsing a whole TOML *document* into a
        // `Value` fails on the first table header.
        let document: toml::Table = text.parse().ok()?;
        let name = document.get("package")?.get("name")?.as_str()?;
        Some(name.replace('-', "_"))
    }

    /// Map every `.rs` file under `src` to the module path it defines.
    fn index_modules(src: &Path) -> BTreeMap<String, PathBuf> {
        let mut modules = BTreeMap::new();
        for file in crate::discover::discover(src, EXTENSIONS) {
            let Ok(relative) = file.strip_prefix(src) else {
                continue;
            };
            let segments: Vec<String> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let Some((last, parents)) = segments.split_last() else {
                continue;
            };
            // `src/bin/*.rs` are crate roots of their own binaries, not
            // modules of the library, and indexing them would let
            // `crate::bin::x` resolve to something no `use` path can name.
            if parents.first().is_some_and(|p| p == "bin") {
                continue;
            }
            let mut path: Vec<&str> = parents.iter().map(String::as_str).collect();
            match last.as_str() {
                "lib.rs" | "main.rs" | "mod.rs" => {}
                other => path.push(other.trim_end_matches(".rs")),
            }
            modules.insert(path.join("::"), file.clone());
        }
        modules
    }

    /// The crate a file belongs to: the one whose `src` directory contains
    /// it, most specific first so a nested crate inside a workspace member
    /// wins over its parent.
    fn crate_of(&self, importer: &Path) -> Option<&CrateInfo> {
        self.crates
            .iter()
            .filter(|c| importer.starts_with(&c.src))
            .max_by_key(|c| c.src.components().count())
    }

    /// The module path of the importing file itself, as `use self::…` and
    /// `use super::…` need it.
    fn module_of(krate: &CrateInfo, importer: &Path) -> Vec<String> {
        krate
            .modules
            .iter()
            .find(|(_, path)| *path == importer)
            .map(|(module, _)| {
                if module.is_empty() {
                    Vec::new()
                } else {
                    module.split("::").map(str::to_string).collect()
                }
            })
            .unwrap_or_default()
    }

    /// The longest prefix of `segments` that names a module in `krate`.
    ///
    /// A `use` path almost always ends in an item rather than a module, so
    /// dropping trailing segments until one matches is not a fallback — it
    /// is the rule.
    fn longest_module_prefix(krate: &CrateInfo, segments: &[String]) -> Option<PathBuf> {
        for length in (0..=segments.len()).rev() {
            let key = segments[..length].join("::");
            if let Some(path) = krate.modules.get(&key) {
                return Some(path.clone());
            }
        }
        None
    }

    /// Split one `use` argument into the module paths it names.
    ///
    /// `use crate::{a::b, c};` is two imports, and treating it as one
    /// (resolving only the `crate` prefix) would collapse two real edges
    /// into an edge to the crate root. `self` inside a list names the
    /// prefix itself: `use crate::a::{self, b}` is `crate::a` and
    /// `crate::a::b`.
    fn expand(text: &str) -> Vec<String> {
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        Self::expand_inner(&text)
    }

    fn expand_inner(text: &str) -> Vec<String> {
        let text = text.trim();
        let Some(open) = text.find('{') else {
            return vec![Self::clean(text)];
        };
        let prefix = &text[..open];
        let Some(close) = text.rfind('}') else {
            return vec![Self::clean(prefix)];
        };
        let inner = &text[open + 1..close];

        let mut out = Vec::new();
        for part in Self::split_top_level(inner) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if part == "self" {
                out.push(Self::clean(prefix));
                continue;
            }
            out.extend(Self::expand_inner(&format!("{prefix}{part}")));
        }
        if out.is_empty() {
            out.push(Self::clean(prefix));
        }
        out
    }

    /// Split on commas that are not inside a nested brace group.
    fn split_top_level(inner: &str) -> Vec<String> {
        let mut parts = Vec::new();
        let mut depth = 0usize;
        let mut current = String::new();
        for c in inner.chars() {
            match c {
                '{' => {
                    depth += 1;
                    current.push(c);
                }
                '}' => {
                    depth = depth.saturating_sub(1);
                    current.push(c);
                }
                ',' if depth == 0 => parts.push(std::mem::take(&mut current)),
                _ => current.push(c),
            }
        }
        parts.push(current);
        parts
    }

    /// Reduce one path to the module path it names: no alias, no glob, no
    /// trailing separator.
    fn clean(text: &str) -> String {
        let text = text.trim();
        let text = match text.find(" as ") {
            Some(index) => &text[..index],
            None => text,
        };
        text.trim()
            .trim_end_matches('*')
            .trim_end_matches("::")
            .trim()
            .to_string()
    }

    /// Resolve `mod foo;` against the importing file's own directory. A
    /// module declared by `src/a.rs` lives in `src/a/`, one declared by
    /// `src/a/mod.rs` (or a crate root) lives beside it.
    fn resolve_mod(&self, name: &str, importer: &Path) -> Resolution {
        let Some(parent) = importer.parent() else {
            return Resolution::Unresolved;
        };
        let is_directory_owner = importer
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| matches!(n, "lib.rs" | "main.rs" | "mod.rs"));
        let dir = if is_directory_owner {
            parent.to_path_buf()
        } else {
            match importer.file_stem().and_then(|s| s.to_str()) {
                Some(stem) => parent.join(stem),
                None => return Resolution::Unresolved,
            }
        };

        for candidate in [
            dir.join(format!("{name}.rs")),
            dir.join(name).join("mod.rs"),
        ] {
            if candidate.is_file() {
                return Resolution::Internal(candidate);
            }
        }
        Resolution::Unresolved
    }
}

impl Extractor for RustExtractor {
    fn lang(&self) -> &'static str {
        "rust"
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        let captures = support::captures(&language, "rust", IMPORT_QUERY, source)?;

        let mut out = Vec::new();
        for capture in captures {
            if capture.name == "mod" {
                out.push(Specifier {
                    raw: format!("{MOD_PREFIX}{}", capture.text.trim()),
                    line: capture.line,
                });
                continue;
            }
            for path in Self::expand(&capture.text) {
                if path.is_empty() {
                    continue;
                }
                out.push(Specifier {
                    raw: path,
                    line: capture.line,
                });
            }
        }
        Ok(out)
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        if let Some(name) = raw.strip_prefix(MOD_PREFIX) {
            return self.resolve_mod(name, importer);
        }

        let Some(importing_crate) = self.crate_of(importer) else {
            // A `.rs` file outside any crate's `src` — a build script, or a
            // file in a directory with no manifest. Nothing internal can be
            // resolved from it with any confidence.
            return Resolution::External(raw.split("::").next().unwrap_or(raw).to_string());
        };

        let segments: Vec<String> = raw.split("::").map(str::to_string).collect();
        let Some((first, rest)) = segments.split_first() else {
            return Resolution::Unresolved;
        };

        let (target_crate, path): (&CrateInfo, Vec<String>) = match first.as_str() {
            "crate" | "$crate" => (importing_crate, rest.to_vec()),
            "self" => {
                let mut path = Self::module_of(importing_crate, importer);
                path.extend(rest.to_vec());
                (importing_crate, path)
            }
            "super" => {
                let mut path = Self::module_of(importing_crate, importer);
                path.pop();
                path.extend(rest.to_vec());
                (importing_crate, path)
            }
            name => {
                let normalised = name.replace('-', "_");
                if normalised == importing_crate.name {
                    (importing_crate, rest.to_vec())
                } else {
                    match self.crates.iter().find(|c| c.name == normalised) {
                        // A sibling crate in the same workspace: an edge
                        // into it is as real as one inside this crate.
                        Some(other) => (other, rest.to_vec()),
                        None => return Resolution::External(name.to_string()),
                    }
                }
            }
        };

        // The path named a module inside a crate we scanned, so a miss is
        // Unresolved, never a fall-through to External.
        match Self::longest_module_prefix(target_crate, &path) {
            Some(file) => Resolution::Internal(file),
            None => Resolution::Unresolved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// `TempDir` hands back a non-canonical path on macOS (`/var` is a
    /// symlink to `/private/var`). `build_graph` always passes canonical
    /// paths to an extractor, so tests must too, or every containment check
    /// fails for a reason production never sees.
    fn root(dir: &TempDir) -> PathBuf {
        dir.path().canonicalize().unwrap()
    }

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = root(dir).join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn crate_at(dir: &TempDir, name: &str) {
        write(
            dir,
            "Cargo.toml",
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
        );
    }

    #[test]
    fn use_declarations_and_module_declarations_are_both_extracted() {
        let dir = TempDir::new().unwrap();
        let e = RustExtractor::new(dir.path());
        let specifiers = e
            .extract(
                "mod model;\nmod tests { fn inner() {} }\nuse crate::model::Node;\nuse std::fs;\n",
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(
            raws,
            vec!["mod model", "crate::model::Node", "std::fs"],
            "an inline `mod tests {{ … }}` declares no file and must not appear"
        );
    }

    #[test]
    fn a_use_list_expands_into_one_specifier_per_path() {
        let dir = TempDir::new().unwrap();
        let e = RustExtractor::new(dir.path());
        let specifiers = e
            .extract("use crate::{model::Node, graph::build};\n")
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(raws, vec!["crate::model::Node", "crate::graph::build"]);
    }

    #[test]
    fn self_inside_a_list_names_the_prefix_itself() {
        let dir = TempDir::new().unwrap();
        let e = RustExtractor::new(dir.path());
        let specifiers = e.extract("use crate::model::{self, Node};\n").unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(raws, vec!["crate::model", "crate::model::Node"]);
    }

    #[test]
    fn an_alias_and_a_glob_are_reduced_to_the_module_path() {
        let dir = TempDir::new().unwrap();
        let e = RustExtractor::new(dir.path());
        let specifiers = e
            .extract("use crate::model::Node as N;\nuse crate::graph::*;\n")
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(raws, vec!["crate::model::Node", "crate::graph"]);
    }

    #[test]
    fn a_module_declaration_resolves_to_the_file_it_names() {
        let dir = TempDir::new().unwrap();
        crate_at(&dir, "app");
        write(&dir, "src/lib.rs", "mod model;");
        write(&dir, "src/model.rs", "");
        let e = RustExtractor::new(dir.path());

        let resolution = e.resolve("mod model", &root(&dir).join("src/lib.rs"));
        assert!(matches!(resolution, Resolution::Internal(ref p) if p.ends_with("src/model.rs")));
    }

    #[test]
    fn a_module_declared_by_a_non_root_file_lives_in_that_files_directory() {
        let dir = TempDir::new().unwrap();
        crate_at(&dir, "app");
        write(&dir, "src/lib.rs", "mod api;");
        write(&dir, "src/api.rs", "mod routes;");
        write(&dir, "src/api/routes.rs", "");
        let e = RustExtractor::new(dir.path());

        let resolution = e.resolve("mod routes", &root(&dir).join("src/api.rs"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("src/api/routes.rs")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn a_module_declared_beside_a_mod_rs_resolves_in_the_same_directory() {
        let dir = TempDir::new().unwrap();
        crate_at(&dir, "app");
        write(&dir, "src/lib.rs", "mod api;");
        write(&dir, "src/api/mod.rs", "mod routes;");
        write(&dir, "src/api/routes.rs", "");
        let e = RustExtractor::new(dir.path());

        let resolution = e.resolve("mod routes", &root(&dir).join("src/api/mod.rs"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("src/api/routes.rs")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn a_use_path_resolves_to_the_longest_prefix_that_is_a_module() {
        // `Node` is a struct, not a module: the edge that was meant points
        // at the file that defines it, `src/model.rs`.
        let dir = TempDir::new().unwrap();
        crate_at(&dir, "app");
        write(&dir, "src/lib.rs", "");
        write(&dir, "src/model.rs", "");
        let e = RustExtractor::new(dir.path());

        let resolution = e.resolve("crate::model::Node", &root(&dir).join("src/lib.rs"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("src/model.rs")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn a_nested_module_resolves_through_both_layout_conventions() {
        let dir = TempDir::new().unwrap();
        crate_at(&dir, "app");
        write(&dir, "src/lib.rs", "");
        write(&dir, "src/api/mod.rs", "");
        write(&dir, "src/api/routes.rs", "");
        let e = RustExtractor::new(dir.path());
        let importer = root(&dir).join("src/lib.rs");

        assert!(
            matches!(e.resolve("crate::api", &importer), Resolution::Internal(ref p) if p.ends_with("src/api/mod.rs"))
        );
        assert!(
            matches!(e.resolve("crate::api::routes::handle", &importer), Resolution::Internal(ref p) if p.ends_with("src/api/routes.rs"))
        );
    }

    #[test]
    fn super_and_self_resolve_relative_to_the_importing_module() {
        let dir = TempDir::new().unwrap();
        crate_at(&dir, "app");
        write(&dir, "src/lib.rs", "");
        write(&dir, "src/model.rs", "");
        write(&dir, "src/api/mod.rs", "");
        write(&dir, "src/api/routes.rs", "");
        let e = RustExtractor::new(dir.path());
        let importer = root(&dir).join("src/api/routes.rs");

        // `super` from `crate::api::routes` is `crate::api`.
        assert!(
            matches!(e.resolve("super::helper", &importer), Resolution::Internal(ref p) if p.ends_with("src/api/mod.rs")),
            "super must climb one module, not one directory"
        );
        // `self` is the importing module itself.
        assert!(
            matches!(e.resolve("self::Thing", &importer), Resolution::Internal(ref p) if p.ends_with("src/api/routes.rs"))
        );
    }

    #[test]
    fn a_sibling_workspace_crate_is_internal_and_a_dependency_is_external() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "crates/graph/Cargo.toml",
            "[package]\nname = \"kog-graph\"\nversion = \"0.1.0\"\n",
        );
        write(&dir, "crates/graph/src/lib.rs", "");
        write(&dir, "crates/graph/src/model.rs", "");
        write(
            &dir,
            "crates/cli/Cargo.toml",
            "[package]\nname = \"kog-cli\"\nversion = \"0.1.0\"\n",
        );
        write(&dir, "crates/cli/src/main.rs", "");
        let e = RustExtractor::new(dir.path());
        let importer = root(&dir).join("crates/cli/src/main.rs");

        // The dash in `kog-graph` becomes an underscore in a `use` path.
        let resolution = e.resolve("kog_graph::model::Node", &importer);
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("graph/src/model.rs")),
            "a workspace sibling is internal, got {resolution:?}"
        );
        assert_eq!(
            e.resolve("serde::Deserialize", &importer),
            Resolution::External("serde".into())
        );
        assert_eq!(
            e.resolve("std::fs", &importer),
            Resolution::External("std".into())
        );
    }

    #[test]
    fn a_path_naming_this_crate_but_no_module_is_unresolved_not_external() {
        let dir = TempDir::new().unwrap();
        crate_at(&dir, "app");
        write(&dir, "src/lib.rs", "");
        let e = RustExtractor::new(dir.path());
        // `crate::` names this crate, so nothing here can be a dependency.
        // Everything drops to the crate root, which exists — so use a crate
        // with no root at all to reach the miss.
        let other = TempDir::new().unwrap();
        crate_at(&other, "empty");
        fs::create_dir_all(root(&other).join("src")).unwrap();
        fs::write(root(&other).join("src/only.rs"), "").unwrap();
        let e2 = RustExtractor::new(other.path());
        assert_eq!(
            e2.resolve("crate::ghost::Thing", &root(&other).join("src/only.rs")),
            Resolution::Unresolved
        );
        // And the first extractor still resolves its own root, proving the
        // above is a genuine miss rather than a broken index.
        assert!(matches!(
            e.resolve("crate::whatever", &root(&dir).join("src/lib.rs")),
            Resolution::Internal(_)
        ));
    }
}
