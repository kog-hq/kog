//! Ruby and PHP: two languages whose imports are ordinary expressions.
//!
//! Ruby's `require_relative "util"` is exact — a path relative to the file
//! that wrote it — while `require "app/thing"` is resolved against the load
//! path, which only exists at run time; the convention every project follows
//! is `lib/`, and that is what is probed before giving up and calling it a
//! gem.
//!
//! PHP's modern form is `use App\Models\User;`, which names a class, and the
//! PSR-4 map in `composer.json` says which directory a namespace prefix
//! lives in. That map is read once, at construction: without it a PHP
//! codebase resolves nothing, because no path is ever written down.

use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::extractors::support;
use serde_json::Value;
use std::path::{Path, PathBuf};

// --- Ruby ----------------------------------------------------------------

const RUBY_QUERY: &str = r#"
(call method: (identifier) @method arguments: (argument_list (string) @spec))
"#;

const RUBY_EXTENSIONS: &[&str] = &["rb", "rake", "gemspec"];

/// Marks a specifier that came from `require_relative` rather than
/// `require`. The two have different rules and the same argument, so the
/// distinction has to survive into `resolve`.
const RELATIVE_PREFIX: &str = "./";

/// Directories a Ruby project puts its own code in, for a plain `require`.
const RUBY_LOAD_PATH: &[&str] = &["lib", "app", "."];

pub struct RubyExtractor {
    root: PathBuf,
}

impl RubyExtractor {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        }
    }
}

impl Extractor for RubyExtractor {
    fn lang(&self) -> &'static str {
        "ruby"
    }

    fn extensions(&self) -> &'static [&'static str] {
        RUBY_EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let language: tree_sitter::Language = tree_sitter_ruby::LANGUAGE.into();
        let groups = support::grouped_captures(&language, "ruby", RUBY_QUERY, source)?;

        let mut out = Vec::new();
        for group in groups {
            let (Some(method), Some(spec)) = (
                support::named(&group, "method"),
                support::named(&group, "spec"),
            ) else {
                continue;
            };
            let relative = match method.text.as_str() {
                "require_relative" => true,
                "require" => false,
                _ => continue,
            };
            let raw = support::unquote(spec.text.trim()).trim();
            if raw.is_empty() {
                continue;
            }
            out.push(Specifier {
                raw: if relative {
                    format!("{RELATIVE_PREFIX}{raw}")
                } else {
                    raw.to_string()
                },
                line: spec.line,
            });
        }
        Ok(out)
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        if let Some(path) = raw.strip_prefix(RELATIVE_PREFIX) {
            // `require_relative` is exact: relative to this file, always.
            let Some(base) = importer.parent() else {
                return Resolution::Unresolved;
            };
            let candidate = support::normalise(&base.join(path));
            if !support::contained(&self.root, &candidate) {
                return Resolution::Unresolved;
            }
            return match support::probe(&candidate, RUBY_EXTENSIONS, &[]) {
                Some(found) => Resolution::Internal(found),
                None => Resolution::Unresolved,
            };
        }

        for dir in RUBY_LOAD_PATH {
            let candidate = support::normalise(&self.root.join(dir).join(raw));
            if !support::contained(&self.root, &candidate) {
                continue;
            }
            if let Some(found) = support::probe(&candidate, RUBY_EXTENSIONS, &[]) {
                return Resolution::Internal(found);
            }
        }
        // Not in the project's own load path: the standard library or a gem.
        Resolution::External(raw.split('/').next().unwrap_or(raw).to_string())
    }
}

// --- PHP -----------------------------------------------------------------

const PHP_QUERY: &str = r#"
(namespace_use_clause (qualified_name) @use)
(namespace_use_clause (name) @use)
(include_expression (string) @include)
(require_expression (string) @include)
(include_once_expression (string) @include)
(require_once_expression (string) @include)
"#;

const PHP_EXTENSIONS: &[&str] = &["php"];

/// Marks a specifier that came from `include`/`require` — a literal path —
/// rather than a `use` statement, which names a namespace.
const PATH_PREFIX: &str = "path:";

pub struct PhpExtractor {
    root: PathBuf,
    /// PSR-4 prefixes from `composer.json`, longest first so the most
    /// specific namespace wins: `App\Domain\` before `App\`.
    psr4: Vec<(String, PathBuf)>,
}

impl PhpExtractor {
    pub fn new(root: &Path) -> Self {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let psr4 = Self::autoload_map(&root);
        Self { root, psr4 }
    }

    /// `autoload.psr-4` and `autoload-dev.psr-4` from the root
    /// `composer.json`. A prefix can map to one directory or to several.
    fn autoload_map(root: &Path) -> Vec<(String, PathBuf)> {
        let mut map: Vec<(String, PathBuf)> = Vec::new();
        let Ok(text) = std::fs::read_to_string(root.join("composer.json")) else {
            return map;
        };
        let Ok(manifest) = serde_json::from_str::<Value>(&text) else {
            return map;
        };
        for section in ["autoload", "autoload-dev"] {
            let Some(psr4) = manifest.get(section).and_then(|a| a.get("psr-4")) else {
                continue;
            };
            let Some(entries) = psr4.as_object() else {
                continue;
            };
            for (prefix, target) in entries {
                let directories: Vec<&str> = match target {
                    Value::String(s) => vec![s.as_str()],
                    Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
                    _ => continue,
                };
                for directory in directories {
                    let path = support::normalise(&root.join(directory));
                    // `composer.json` ships with the repository, so its
                    // paths are not trusted to stay inside it.
                    if support::contained(root, &path) {
                        map.push((prefix.clone(), path));
                    }
                }
            }
        }
        map.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
        map
    }
}

impl Extractor for PhpExtractor {
    fn lang(&self) -> &'static str {
        "php"
    }

    fn extensions(&self) -> &'static [&'static str] {
        PHP_EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let language: tree_sitter::Language = tree_sitter_php::LANGUAGE_PHP.into();
        let captures = support::captures(&language, "php", PHP_QUERY, source)?;
        Ok(captures
            .into_iter()
            .filter_map(|capture| {
                let raw = support::unquote(capture.text.trim()).trim();
                if raw.is_empty() {
                    return None;
                }
                Some(Specifier {
                    raw: if capture.name == "include" {
                        format!("{PATH_PREFIX}{raw}")
                    } else {
                        raw.trim_start_matches('\\').to_string()
                    },
                    line: capture.line,
                })
            })
            .collect())
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        if let Some(path) = raw.strip_prefix(PATH_PREFIX) {
            // `require __DIR__ . '/bootstrap.php'` reaches here as the
            // string fragment alone, which is still a usable relative path.
            let Some(base) = importer.parent() else {
                return Resolution::Unresolved;
            };
            let candidate = support::normalise(&base.join(path.trim_start_matches('/')));
            if !support::contained(&self.root, &candidate) {
                return Resolution::Unresolved;
            }
            return match support::probe(&candidate, PHP_EXTENSIONS, &[]) {
                Some(found) => Resolution::Internal(found),
                None => Resolution::Unresolved,
            };
        }

        for (prefix, directory) in &self.psr4 {
            let Some(rest) = raw.strip_prefix(prefix.as_str()) else {
                continue;
            };
            // PSR-4: the rest of the namespace is the path, one directory
            // per separator, and the class is the file name.
            let candidate = support::normalise(
                &directory.join(rest.replace('\\', "/").trim_start_matches('/')),
            );
            if !support::contained(&self.root, &candidate) {
                return Resolution::Unresolved;
            }
            // The prefix matched, so the author named a class this project
            // autoloads: a miss is Unresolved, never External.
            return match support::probe(&candidate, PHP_EXTENSIONS, &[]) {
                Some(found) => Resolution::Internal(found),
                None => Resolution::Unresolved,
            };
        }

        // No autoload prefix claimed it: a vendor package or a global class.
        Resolution::External(raw.split('\\').next().unwrap_or(raw).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn root(dir: &TempDir) -> PathBuf {
        dir.path().canonicalize().unwrap()
    }

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = root(dir).join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn ruby_extracts_both_require_forms_and_nothing_else() {
        let dir = TempDir::new().unwrap();
        let e = RubyExtractor::new(dir.path());
        let specifiers = e
            .extract(
                "require \"json\"\nrequire_relative \"util\"\nputs \"not an import\"\nload \"other.rb\"\n",
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(raws, vec!["json", "./util"]);
    }

    #[test]
    fn require_relative_resolves_beside_the_requiring_file() {
        let dir = TempDir::new().unwrap();
        write(&dir, "lib/app/main.rb", "");
        write(&dir, "lib/app/util.rb", "");
        let e = RubyExtractor::new(dir.path());

        let resolution = e.resolve("./util", &root(&dir).join("lib/app/main.rb"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("lib/app/util.rb")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn a_plain_require_resolves_through_lib_and_a_gem_stays_external() {
        let dir = TempDir::new().unwrap();
        write(&dir, "lib/app/thing.rb", "");
        write(&dir, "bin/run.rb", "");
        let e = RubyExtractor::new(dir.path());
        let importer = root(&dir).join("bin/run.rb");

        assert!(
            matches!(e.resolve("app/thing", &importer), Resolution::Internal(ref p) if p.ends_with("lib/app/thing.rb"))
        );
        assert_eq!(
            e.resolve("json", &importer),
            Resolution::External("json".into())
        );
    }

    #[test]
    fn php_extracts_use_statements_and_include_paths() {
        let dir = TempDir::new().unwrap();
        let e = PhpExtractor::new(dir.path());
        let specifiers = e
            .extract(
                "<?php\nuse App\\Models\\User;\nuse \\App\\Support\\Str;\nrequire_once 'bootstrap.php';\n$x = 1;\n",
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(
            raws,
            vec![
                "App\\Models\\User",
                "App\\Support\\Str",
                "path:bootstrap.php"
            ]
        );
    }

    #[test]
    fn a_psr4_namespace_resolves_to_its_class_file() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "composer.json",
            r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        );
        write(&dir, "src/Models/User.php", "");
        write(&dir, "src/Http/Controller.php", "");
        let e = PhpExtractor::new(dir.path());

        let resolution = e.resolve(
            "App\\Models\\User",
            &root(&dir).join("src/Http/Controller.php"),
        );
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("src/Models/User.php")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn the_most_specific_psr4_prefix_wins() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "composer.json",
            r#"{"autoload":{"psr-4":{"App\\":"src/","App\\Domain\\":"domain/"}}}"#,
        );
        write(&dir, "src/Domain/Order.php", "");
        write(&dir, "domain/Order.php", "");
        let e = PhpExtractor::new(dir.path());

        let resolution = e.resolve("App\\Domain\\Order", &root(&dir).join("src/x.php"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("domain/Order.php")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn a_vendor_class_is_external_and_a_missing_autoloaded_one_is_unresolved() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "composer.json",
            r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        );
        write(&dir, "src/x.php", "");
        let e = PhpExtractor::new(dir.path());
        let importer = root(&dir).join("src/x.php");

        assert_eq!(
            e.resolve("Illuminate\\Support\\Collection", &importer),
            Resolution::External("Illuminate".into())
        );
        assert_eq!(
            e.resolve("App\\Models\\Ghost", &importer),
            Resolution::Unresolved,
            "the prefix matched, so this is a broken class reference, not a package"
        );
    }

    #[test]
    fn a_project_with_no_composer_json_treats_every_use_as_external() {
        let dir = TempDir::new().unwrap();
        write(&dir, "index.php", "");
        let e = PhpExtractor::new(dir.path());
        assert!(matches!(
            e.resolve("App\\Models\\User", &root(&dir).join("index.php")),
            Resolution::External(_)
        ));
    }
}
