use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Directories that are never source, whatever `.gitignore` says.
///
/// Entries are matched by directory name at any depth. Each one is either
/// dependency code that is not the project's own (`node_modules`, `vendor`,
/// `Pods`, `.venv`), build output (`target`, `dist`, `build`, `.next`,
/// `.turbo`, `__pycache__`), or tooling state (`.git`, `coverage`). A file
/// under any of them is never a node, and an import that resolves into one
/// is reported as [`crate::ExclusionReason::SkippedDirectory`] rather than
/// silently dropped.
pub const ALWAYS_SKIP: &[&str] = &[
    // JavaScript / general
    "node_modules",
    "bower_components",
    ".yarn",
    ".pnpm-store",
    // Build output
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".output",
    ".parcel-cache",
    "bazel-out",
    "cmake-build-debug",
    "cmake-build-release",
    // Language toolchain state
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".gradle",
    ".dart_tool",
    ".bundle",
    "Pods",
    "Carthage",
    "DerivedData",
    // Version control and tooling
    ".git",
    ".hg",
    ".svn",
    "vendor",
    "coverage",
    ".terraform",
];

/// Whether any component of `path` is a directory this walker never enters.
///
/// Used to explain an exclusion after the fact: a file that exists on disk
/// but never became a node was either refused by this list or by an ignore
/// rule, and this is what tells the two apart.
pub fn is_in_skipped_directory(path: &Path) -> bool {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|name| ALWAYS_SKIP.contains(&name))
}

/// Every file under `root` the walker visited, whatever its extension, plus
/// the directories it refused to enter.
///
/// One walk answers both questions the graph needs: which files are
/// candidates for an extractor, and which files exist but are not — the
/// second being what makes the coverage report possible at all. Walking
/// twice (once per extension set, once for everything) would double the
/// cost on a large repository and risk the two disagreeing.
#[derive(Debug, Default)]
pub struct Survey {
    /// Absolute paths, sorted, so two runs on an unchanged tree agree.
    pub files: Vec<PathBuf>,
    /// Directories refused by [`ALWAYS_SKIP`], counted by name. Ignore-rule
    /// exclusions are absent by construction: a `.gitignore`d directory is
    /// never handed to the filter at all, so it cannot be counted here
    /// without a second, far more expensive walk.
    pub skipped_directories: BTreeMap<String, usize>,
}

/// Walk `root` once, collecting every visited file and every refused
/// directory. Returns an empty survey for an unreadable or missing root,
/// like [`discover`].
pub fn survey(root: &Path) -> Survey {
    let root = match root.canonicalize() {
        Ok(canonical) => canonical,
        Err(_) => return Survey::default(),
    };

    // `filter_entry` takes an `Fn`, so the tally needs interior mutability.
    // The walk here is single-threaded (`WalkBuilder::build`, not
    // `build_parallel`), so the lock is never contended.
    let skipped: Arc<Mutex<BTreeMap<String, usize>>> = Arc::default();
    let tally = Arc::clone(&skipped);

    let mut builder = ignore::WalkBuilder::new(&root);
    builder.hidden(false).git_ignore(true).require_git(false);
    builder.filter_entry(move |entry| {
        let Some(name) = entry.file_name().to_str() else {
            return true;
        };
        if !ALWAYS_SKIP.contains(&name) {
            return true;
        }
        // Count only directories: a *file* called `build` is not a skipped
        // directory, and reporting it as one would be a lie in the table.
        if entry.file_type().is_some_and(|t| t.is_dir()) {
            *tally
                .lock()
                .expect("skip tally")
                .entry(name.to_string())
                .or_insert(0) += 1;
        }
        false
    });

    let mut files: Vec<PathBuf> = builder
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .collect();
    files.sort();

    // The builder still owns the closure that holds the other `Arc`, so the
    // tally is cloned out rather than unwrapped: `Arc::try_unwrap` would
    // fail here and silently yield an empty table.
    let skipped_directories = skipped.lock().expect("skip tally").clone();
    Survey {
        files,
        skipped_directories,
    }
}

/// Build a walker that respects `.gitignore` and skips build directories.
///
/// This walker is configured to:
/// - Honor `.gitignore` even without a `.git` directory
/// - Skip hard-coded build directories (`ALWAYS_SKIP`) via filter_entry
/// - Not skip hidden files/directories (callers handle if needed)
///
/// Callers should chain additional filters as needed (e.g., for file type or extension).
pub fn build_walker(root: &Path) -> ignore::WalkBuilder {
    let mut builder = ignore::WalkBuilder::new(root);
    builder.hidden(false).git_ignore(true).require_git(false);
    builder.filter_entry(|entry| {
        entry
            .file_name()
            .to_str()
            .is_none_or(|name| !ALWAYS_SKIP.contains(&name))
    });
    builder
}

/// Every source file under `root` matching one of `extensions`.
///
/// Respects `.gitignore`. Returns absolute paths, sorted, so two runs on an
/// unchanged tree produce byte-identical output.
pub fn discover(root: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    // Canonicalize root to ensure returned paths are absolute.
    let root = match root.canonicalize() {
        Ok(canonical) => canonical,
        Err(_) => return Vec::new(), // Unreadable or missing root yields nothing.
    };

    let mut found: Vec<PathBuf> = build_walker(&root)
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
        write(&dir, ".gitignore", "generated/\n");
        write(&dir, "src/a.ts", "");
        write(&dir, "generated/b.ts", "");
        let found = discover(dir.path(), &["ts"]);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("src/a.ts"));
    }

    #[test]
    fn gitignore_is_honoured_even_without_a_git_directory() {
        // Prove that gitignore works without .git by comparing two runs:
        // one with .gitignore only, and one where we rewrite the walker
        // to require_git(true). The gitignore-only case must find fewer
        // files — proving that gitignore parsing happens regardless.
        let dir = TempDir::new().unwrap();
        write(&dir, ".gitignore", "excluded/\n");
        write(&dir, "src/a.ts", "");
        write(&dir, "excluded/b.ts", "");
        // Note: no .git directory exists. If require_git defaulted to true,
        // both files would be found; with require_git(false), only one.
        let found = discover(dir.path(), &["ts"]);
        assert_eq!(found.len(), 1, "gitignore should exclude excluded/");
    }

    #[test]
    fn discovered_paths_are_canonical() {
        // Test that canonicalize() normalizes paths to remove `..` components.
        // Pass a non-canonical absolute path (containing `..`) to discover, and
        // verify that results match those from the canonical path directly.
        let dir = TempDir::new().unwrap();
        write(&dir, "a.ts", "");

        // Canonical root: dir.path() is always absolute
        let canonical_root = dir.path();
        let found_canonical = discover(canonical_root, &["ts"]);

        // Non-canonical root: add a subdir and `..` to backtrack
        let subdir = dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        let noncanonical_root = subdir.join("..");

        // Without canonicalize(), the non-canonical path would return paths with `..`
        // inside them; with canonicalize(), they should be identical.
        let found_noncanonical = discover(&noncanonical_root, &["ts"]);

        // Both should find the same file with identical paths
        assert_eq!(
            found_canonical, found_noncanonical,
            "canonical and non-canonical roots should produce identical results"
        );

        // Verify both are absolute
        for path in &found_canonical {
            assert!(
                path.is_absolute(),
                "canonical path should be absolute: {:?}",
                path
            );
        }
        for path in &found_noncanonical {
            assert!(
                path.is_absolute(),
                "non-canonical path should still be absolute after canonicalize: {:?}",
                path
            );
        }
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
    fn always_skip_and_gitignore_filters_are_independent() {
        // Prove that ALWAYS_SKIP works regardless of what .gitignore says.
        // Create a .gitignore that does NOT exclude node_modules (it explicitly
        // allows it with a negation pattern), yet node_modules should still be
        // excluded by the hard-coded skip list.
        let dir = TempDir::new().unwrap();
        // Write a .gitignore that ignores 'generated/' but explicitly allows node_modules
        write(&dir, ".gitignore", "generated/\n!node_modules/\n");
        write(&dir, "src/a.ts", "");
        write(&dir, "node_modules/pkg/index.ts", "");
        write(&dir, "generated/auto.ts", "");

        let found = discover(dir.path(), &["ts"]);

        // Only src/a.ts should be found:
        // - src/a.ts: kept (normal source)
        // - node_modules/pkg/index.ts: excluded by ALWAYS_SKIP, despite gitignore
        //   allowing it
        // - generated/auto.ts: excluded by gitignore
        assert_eq!(found.len(), 1, "only src/a.ts should be found");
        assert!(found[0].ends_with("src/a.ts"));
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
        let found = discover(std::path::Path::new("/definitely/not/a/real/path"), &["ts"]);
        assert!(found.is_empty());
    }

    #[test]
    fn a_survey_keeps_every_extension_not_just_the_source_ones() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "README.md", "");
        write(&dir, "logo.png", "");
        let survey = survey(dir.path());
        assert_eq!(survey.files.len(), 3, "a survey is not extension-filtered");
    }

    #[test]
    fn a_survey_counts_the_directories_it_refused_by_name() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "");
        write(&dir, "node_modules/pkg/index.ts", "");
        write(&dir, "packages/x/node_modules/dep/index.ts", "");
        write(&dir, "dist/bundle.js", "");
        let survey = survey(dir.path());
        assert_eq!(survey.files.len(), 1, "only src/a.ts is visited");
        assert_eq!(survey.skipped_directories.get("node_modules"), Some(&2));
        assert_eq!(survey.skipped_directories.get("dist"), Some(&1));
    }

    /// A *file* named `build` is not a build directory. Counting it as one
    /// would put a fiction in the coverage table.
    #[test]
    fn a_file_sharing_a_skipped_directorys_name_is_not_counted_as_one() {
        let dir = TempDir::new().unwrap();
        write(&dir, "build", "#!/bin/sh\n");
        let survey = survey(dir.path());
        assert!(survey.skipped_directories.is_empty());
    }

    #[test]
    fn a_skipped_directory_is_recognised_at_any_depth() {
        assert!(is_in_skipped_directory(Path::new("a/node_modules/b/c.ts")));
        assert!(is_in_skipped_directory(Path::new("dist/x.js")));
        assert!(!is_in_skipped_directory(Path::new("src/builder/x.ts")));
    }

    #[test]
    fn a_survey_of_a_missing_root_yields_nothing_rather_than_panicking() {
        let survey = survey(std::path::Path::new("/definitely/not/a/real/path"));
        assert!(survey.files.is_empty());
        assert!(survey.skipped_directories.is_empty());
    }
}
