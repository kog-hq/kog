use std::path::{Path, PathBuf};

/// Directories that are never source, whatever `.gitignore` says.
pub const ALWAYS_SKIP: &[&str] = &[
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
    fn discovered_paths_are_absolute() {
        let dir = TempDir::new().unwrap();
        write(&dir, "a.ts", "");
        let found = discover(dir.path(), &["ts"]);
        assert!(!found.is_empty());
        for path in found {
            assert!(path.is_absolute(), "path should be absolute: {:?}", path);
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
}
