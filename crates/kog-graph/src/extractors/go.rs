//! Go: the language where an import names a package, not a file.
//!
//! This is the case the `Extractor` trait was designed against. A Go import
//! path resolves to a *directory* — every `.go` file in it is part of the
//! same package, and any of them may hold the symbol being used. One
//! specifier therefore yields a set of targets ([`Resolution::InternalSet`]),
//! counts once towards the resolution rate, and contributes an edge per file
//! in the package that is a node.
//!
//! Resolution is driven by `go.mod`. A path that starts with a module
//! declared inside the scanned tree is internal; everything else is a
//! dependency or the standard library, and leaves the denominator.

use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::extractors::support;
use std::path::{Path, PathBuf};

/// The string literal of every import spec, in both the single-import and
/// the parenthesised-block form — the grammar represents them identically.
const IMPORT_QUERY: &str = r#"
(import_spec (interpreted_string_literal) @spec)
(import_spec (raw_string_literal) @spec)
"#;

const EXTENSIONS: &[&str] = &["go"];

/// One `go.mod`, and the directory it governs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Module {
    /// The declared module path, e.g. `github.com/kog-hq/kog`.
    path: String,
    /// The directory holding the `go.mod`.
    dir: PathBuf,
}

pub struct GoExtractor {
    /// Every `go.mod` in the scanned tree, in the order the walker found
    /// them. Which one resolves a given import is decided per import, from
    /// the importing file's own position — see [`GoExtractor::resolve`].
    /// Ordering them here cannot work: two `go.mod` files may declare the
    /// same module path, and nothing about the pair alone says which one a
    /// particular file belongs to.
    modules: Vec<Module>,
}

impl GoExtractor {
    pub fn new(root: &Path) -> Self {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let modules: Vec<Module> = crate::discover::survey(&root)
            .files
            .iter()
            .filter(|p| p.file_name().is_some_and(|n| n == "go.mod"))
            .filter_map(|manifest| {
                let path = Self::module_path(manifest)?;
                let dir = manifest.parent()?.to_path_buf();
                Some(Module { path, dir })
            })
            .collect();
        Self { modules }
    }

    /// The `module` line of a `go.mod`. Read line by line rather than with a
    /// parser: the directive is a fixed, single-line form, and a `go.mod`
    /// that is malformed everywhere else should still tell us the module
    /// path if that one line is intact.
    fn module_path(manifest: &Path) -> Option<String> {
        let text = std::fs::read_to_string(manifest).ok()?;
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("module ") {
                let path = rest.trim().trim_matches('"').trim();
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
        None
    }
}

impl Extractor for GoExtractor {
    fn lang(&self) -> &'static str {
        "go"
    }

    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let language: tree_sitter::Language = tree_sitter_go::LANGUAGE.into();
        Ok(support::specifiers_from(support::captures(
            &language,
            "go",
            IMPORT_QUERY,
            source,
        )?))
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        // A Go file belongs to the module of the nearest enclosing `go.mod`,
        // so that one is tried first, and every other matching module after
        // it.
        //
        // Both halves of that are load-bearing, and neither was there before.
        // Two `go.mod` files can declare the *same* module path: `cli/cli`
        // ships a CodeQL test fixture whose `go.mod` repeats the
        // repository's own `module github.com/cli/cli/v2`. The old code
        // ordered modules by path length — a tie here — and returned on the
        // first one whose prefix matched, so every self-import in the
        // repository was resolved against a fixture directory that holds
        // none of them. Fail-closed then turned each one into `Unresolved`
        // with no second chance: 2,995 of 3,427 internal specifiers, a
        // published Go rate of 0.1261, on a repository where the imports are
        // all perfectly ordinary.
        let mut candidates: Vec<&Module> = self.modules.iter().collect();
        candidates.sort_by_key(|module| {
            (
                // Enclosing modules first…
                !importer.starts_with(&module.dir),
                // …deepest first among them, which is Go's "nearest
                // `go.mod` wins" rule.
                std::cmp::Reverse(module.dir.components().count()),
            )
        });

        let mut named_this_repository = false;
        for module in candidates {
            let rest = if raw == module.path {
                Some("")
            } else {
                raw.strip_prefix(&module.path)
                    .and_then(|rest| rest.strip_prefix('/'))
            };
            let Some(rest) = rest else {
                continue;
            };
            named_this_repository = true;

            let package_dir = support::normalise(&module.dir.join(rest));
            if !support::contained(&module.dir, &package_dir) {
                continue;
            }
            let files = support::files_in_dir(&package_dir, EXTENSIONS);
            if !files.is_empty() {
                return Resolution::InternalSet(files);
            }
        }

        if named_this_repository {
            // A module prefix matched somewhere and no module held the
            // package: the author named a package in this repository that is
            // not here. Unresolved, never a fall-through to External — the
            // same fail-closed shape as a TypeScript alias.
            return Resolution::Unresolved;
        }

        // Everything else is the standard library or a dependency. The
        // whole path is the package's identity in Go — `net/http` is not a
        // sub-path of a package called `net`, it *is* the package.
        Resolution::External(raw.to_string())
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

    /// Found by measuring [`cli/cli`](https://github.com/cli/cli), which
    /// ships a CodeQL test fixture whose `go.mod` repeats the repository's
    /// own `module` line verbatim. Two modules then declare the same path,
    /// the old ordering (by path length) could not tell them apart, and the
    /// resolver returned on whichever it saw first — sending every
    /// self-import in the repository to a fixture directory that holds none
    /// of them. Fail-closed did the rest: 2,995 of 3,427 internal specifiers
    /// unresolved, and a published Go rate of 0.1261 on a repository whose
    /// imports are entirely ordinary.
    #[test]
    fn a_duplicate_module_path_elsewhere_does_not_hide_the_real_one() {
        let dir = TempDir::new().unwrap();
        // The fixture, buried deep, declaring the same module path.
        write(
            &dir,
            ".github/codeql/tests/fixture/go.mod",
            "module example.com/app\n",
        );
        write(
            &dir,
            ".github/codeql/tests/fixture/main.go",
            "package main\n",
        );
        // The real module, at the root.
        write(&dir, "go.mod", "module example.com/app\n");
        write(&dir, "internal/store/store.go", "package store\n");
        write(&dir, "main.go", "package main\n");

        let e = GoExtractor::new(dir.path());
        let resolution = e.resolve(
            "example.com/app/internal/store",
            &root(&dir).join("main.go"),
        );

        assert!(
            matches!(resolution, Resolution::InternalSet(ref files)
                     if files.len() == 1 && files[0].ends_with("internal/store/store.go")),
            "the importing file's own module must win, got {resolution:?}"
        );
    }

    /// The other half of the same rule: a file *inside* the nested module
    /// resolves against that one, because Go's rule is the nearest enclosing
    /// `go.mod` and not the outermost.
    #[test]
    fn a_file_inside_a_nested_module_resolves_against_that_module() {
        let dir = TempDir::new().unwrap();
        write(&dir, "go.mod", "module example.com/outer\n");
        write(&dir, "shared/shared.go", "package shared\n");
        write(&dir, "tools/go.mod", "module example.com/inner\n");
        write(&dir, "tools/lib/lib.go", "package lib\n");
        write(&dir, "tools/main.go", "package main\n");

        let e = GoExtractor::new(dir.path());
        let resolution = e.resolve("example.com/inner/lib", &root(&dir).join("tools/main.go"));

        assert!(
            matches!(resolution, Resolution::InternalSet(ref files)
                     if files.len() == 1 && files[0].ends_with("tools/lib/lib.go")),
            "got {resolution:?}"
        );
    }

    /// A package named in this repository that genuinely is not here stays
    /// `Unresolved` — trying every module must not become a fall-through to
    /// `External`, which would let a broken import hide as a dependency.
    #[test]
    fn a_self_module_import_that_names_nothing_is_still_unresolved() {
        let dir = TempDir::new().unwrap();
        write(&dir, "go.mod", "module example.com/app\n");
        write(&dir, "main.go", "package main\n");

        let e = GoExtractor::new(dir.path());
        let resolution = e.resolve(
            "example.com/app/internal/ghost",
            &root(&dir).join("main.go"),
        );

        assert_eq!(resolution, Resolution::Unresolved);
    }

    #[test]
    fn a_parenthesised_import_block_yields_every_specifier() {
        let dir = TempDir::new().unwrap();
        let e = GoExtractor::new(dir.path());
        let specifiers = e
            .extract(
                r#"package main

import (
	"fmt"
	"example.com/app/internal/store"
)
"#,
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(raws, vec!["fmt", "example.com/app/internal/store"]);
        assert_eq!(specifiers[0].line, 4);
    }

    #[test]
    fn a_single_import_is_found_too() {
        let dir = TempDir::new().unwrap();
        let e = GoExtractor::new(dir.path());
        let specifiers = e.extract("package main\nimport \"os\"\n").unwrap();
        assert_eq!(specifiers.len(), 1);
        assert_eq!(specifiers[0].raw, "os");
    }

    #[test]
    fn an_import_of_a_local_package_resolves_to_every_file_in_it() {
        let dir = TempDir::new().unwrap();
        write(&dir, "go.mod", "module example.com/app\n\ngo 1.22\n");
        write(&dir, "main.go", "package main");
        write(&dir, "internal/store/store.go", "package store");
        write(&dir, "internal/store/query.go", "package store");
        write(&dir, "internal/store/README.md", "not go");

        let e = GoExtractor::new(dir.path());
        let resolution = e.resolve(
            "example.com/app/internal/store",
            &root(&dir).join("main.go"),
        );
        let targets = resolution.targets();
        assert_eq!(
            targets.len(),
            2,
            "a package is every .go file in its directory, got {targets:?}"
        );
        assert!(targets.iter().all(|t| t.extension().unwrap() == "go"));
    }

    #[test]
    fn the_standard_library_and_dependencies_are_external() {
        let dir = TempDir::new().unwrap();
        write(&dir, "go.mod", "module example.com/app\n");
        let e = GoExtractor::new(dir.path());
        assert_eq!(
            e.resolve("net/http", &root(&dir).join("main.go")),
            Resolution::External("net/http".into()),
            "a stdlib path is the package's whole identity, not its first segment"
        );
        assert_eq!(
            e.resolve("github.com/gin-gonic/gin", &root(&dir).join("main.go")),
            Resolution::External("github.com/gin-gonic/gin".into())
        );
    }

    #[test]
    fn an_import_naming_this_module_but_no_directory_is_unresolved_not_external() {
        // The module prefix matched, so the author meant a package in this
        // repository. Falling through to External here would hide a broken
        // import behind the dependency count.
        let dir = TempDir::new().unwrap();
        write(&dir, "go.mod", "module example.com/app\n");
        write(&dir, "main.go", "package main");
        let e = GoExtractor::new(dir.path());
        assert_eq!(
            e.resolve("example.com/app/ghost", &root(&dir).join("main.go")),
            Resolution::Unresolved
        );
    }

    #[test]
    fn the_most_specific_module_wins_in_a_multi_module_repository() {
        let dir = TempDir::new().unwrap();
        write(&dir, "go.mod", "module example.com/app\n");
        write(&dir, "tools/go.mod", "module example.com/app/tools\n");
        write(&dir, "tools/gen/gen.go", "package gen");
        // Deliberately also present under the outer module's own layout, so
        // resolving against the wrong module would still find *a* file and
        // the test would pass for the wrong reason.
        write(&dir, "tools/gen/other.go", "package gen");

        let e = GoExtractor::new(dir.path());
        let resolution = e.resolve("example.com/app/tools/gen", &root(&dir).join("main.go"));
        assert_eq!(resolution.targets().len(), 2);
        assert!(resolution.targets()[0].ends_with("tools/gen/gen.go"));
    }

    #[test]
    fn a_repository_with_no_go_mod_treats_every_import_as_external() {
        let dir = TempDir::new().unwrap();
        write(&dir, "main.go", "package main");
        let e = GoExtractor::new(dir.path());
        assert!(matches!(
            e.resolve("example.com/app/store", &root(&dir).join("main.go")),
            Resolution::External(_)
        ));
    }

    #[test]
    fn a_module_path_naming_a_directory_outside_the_module_is_refused() {
        let dir = TempDir::new().unwrap();
        write(&dir, "go.mod", "module example.com/app\n");
        write(&dir, "main.go", "package main");
        let e = GoExtractor::new(dir.path());
        assert_eq!(
            e.resolve("example.com/app/../../etc", &root(&dir).join("main.go")),
            Resolution::Unresolved
        );
    }
}
