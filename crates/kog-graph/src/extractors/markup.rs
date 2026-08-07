//! HTML and stylesheets: the two file types that reference other files
//! without importing them in any language sense.
//!
//! An HTML page names its scripts, its stylesheets and the pages it links
//! to; a stylesheet names the partials it composes. Both are real edges in
//! a codebase — a page is where a bundle actually enters the application —
//! and both are invisible to a tool that only reads import statements.
//!
//! What is deliberately *not* followed: URLs (`https://`, `//cdn…`,
//! `data:`, `mailto:`, `#anchor`). They name something that is not a file
//! in this repository, so they are external by definition.

use crate::extractor::{ExtractError, Extractor, Resolution, Specifier};
use crate::extractors::support;
use std::path::{Path, PathBuf};

/// Tags whose reference is a file this project owns. `img`, `video` and
/// friends are deliberately absent: they name assets, which are nodes, but
/// listing every image reference would bury the structural edges — the ones
/// that say where code enters the page — under a pile of decoration.
const LINKING_TAGS: &[&str] = &["script", "link", "a", "iframe", "embed", "object"];

const HTML_QUERY: &str = r#"
(element (start_tag (tag_name) @tag
  (attribute (attribute_name) @name (quoted_attribute_value (attribute_value) @value))))
(script_element (start_tag (tag_name) @tag
  (attribute (attribute_name) @name (quoted_attribute_value (attribute_value) @value))))
(self_closing_tag (tag_name) @tag
  (attribute (attribute_name) @name (quoted_attribute_value (attribute_value) @value)))
"#;

const HTML_EXTENSIONS: &[&str] = &["html", "htm", "xhtml"];

pub struct HtmlExtractor {
    root: PathBuf,
}

impl HtmlExtractor {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        }
    }
}

/// Whether a reference names something outside this repository entirely.
fn is_remote(target: &str) -> bool {
    target.is_empty()
        || target.starts_with('#')
        || target.starts_with("//")
        || target.contains("://")
        || target.starts_with("data:")
        || target.starts_with("mailto:")
        || target.starts_with("tel:")
        || target.starts_with("javascript:")
        // A framework's own template syntax, not a path: `{{ url }}`,
        // `{% static %}`, `<%= asset %>`, `${href}`, and SvelteKit's
        // `%sveltekit.assets%/favicon.png` — the placeholder is substituted
        // at build time, so no file of that name has ever existed.
        || target.starts_with('{')
        || target.starts_with("<%")
        || target.starts_with("${")
        || (target.starts_with('%') && target[1..].contains('%'))
}

/// Directories a build tool serves at the URL root. `/favicon.svg` in a
/// Vite or Next.js app is `public/favicon.svg` on disk, and nothing in the
/// HTML says so.
const WEB_ROOTS: &[&str] = &["public", "static", "assets", "www"];

/// Where a root-relative reference (`/src/main.tsx`) might actually live.
///
/// A leading `/` means "the root of the served site", which is not the same
/// directory as the root of the repository: in a Vite app the page sits in
/// `app/index.html` and is served from `app/`, so `/src/main.tsx` is
/// `app/src/main.tsx`. Found by scanning KOG's own repository, where both
/// references in its one HTML page were reported unresolved.
fn document_roots(root: &Path, importer: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(parent) = importer.parent() {
        roots.push(parent.to_path_buf());
        roots.extend(WEB_ROOTS.iter().map(|name| parent.join(name)));
    }
    roots.push(root.to_path_buf());
    roots.extend(WEB_ROOTS.iter().map(|name| root.join(name)));
    roots.dedup();
    roots
}

/// Resolve a document-relative reference against the file that wrote it,
/// and a root-relative one (`/assets/app.js`) against the document roots a
/// build tool would serve.
fn resolve_reference(root: &Path, raw: &str, importer: &Path, extensions: &[&str]) -> Resolution {
    if is_remote(raw) {
        return Resolution::External(raw.to_string());
    }
    // A query string or fragment is a cache-buster or an anchor, never part
    // of the file's name on disk: `app.css?v=2`, `page.html#section`.
    let path = raw.split(['?', '#']).next().unwrap_or(raw);
    if path.is_empty() {
        return Resolution::External(raw.to_string());
    }

    let bases = match path.strip_prefix('/') {
        Some(_) => document_roots(root, importer),
        None => match importer.parent() {
            Some(parent) => vec![parent.to_path_buf()],
            None => return Resolution::Unresolved,
        },
    };
    for base in bases {
        let candidate = support::normalise(&base.join(path.trim_start_matches('/')));
        if !support::contained(root, &candidate) {
            continue;
        }
        if let Some(found) = support::probe(&candidate, extensions, &["index.html"]) {
            return Resolution::Internal(found);
        }
    }
    Resolution::Unresolved
}

impl Extractor for HtmlExtractor {
    fn lang(&self) -> &'static str {
        "html"
    }

    fn extensions(&self) -> &'static [&'static str] {
        HTML_EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let language: tree_sitter::Language = tree_sitter_html::LANGUAGE.into();
        let groups = support::grouped_captures(&language, "html", HTML_QUERY, source)?;

        let mut out = Vec::new();
        for group in groups {
            let (Some(tag), Some(name), Some(value)) = (
                support::named(&group, "tag"),
                support::named(&group, "name"),
                support::named(&group, "value"),
            ) else {
                continue;
            };
            if !LINKING_TAGS.contains(&tag.text.to_ascii_lowercase().as_str()) {
                continue;
            }
            let attribute = name.text.to_ascii_lowercase();
            if attribute != "src" && attribute != "href" && attribute != "data" {
                continue;
            }
            let raw = support::unquote(&value.text).trim().to_string();
            if raw.is_empty() {
                continue;
            }
            out.push(Specifier {
                raw,
                line: value.line,
            });
        }
        Ok(out)
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        resolve_reference(&self.root, raw, importer, &[])
    }
}

// --- Stylesheets ---------------------------------------------------------

/// `@import` has its own node in the CSS grammar. Sass's `@use` and
/// `@forward` do not: the grammar recognises the keyword, then fails on the
/// string that follows and leaves it in an ERROR node — so the at-rule is
/// captured by its keyword and its argument read from the source after it.
/// Without this, every partial in a Sass codebase silently vanishes.
const CSS_QUERY: &str = r#"
(import_statement) @statement
(at_rule (at_keyword) @keyword)
"#;

const CSS_EXTENSIONS: &[&str] = &["css", "scss", "sass", "less", "styl"];

/// At-rules that name another stylesheet. `@media`, `@supports` and the
/// rest name nothing.
const IMPORTING_AT_RULES: &[&str] = &["@import", "@use", "@forward", "@require"];

pub struct CssExtractor {
    root: PathBuf,
}

impl CssExtractor {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        }
    }
}

impl Extractor for CssExtractor {
    fn lang(&self) -> &'static str {
        "css"
    }

    fn lang_for(&self, path: &Path) -> &'static str {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match extension.as_str() {
            "scss" | "sass" => "sass",
            "less" => "less",
            "styl" => "stylus",
            _ => "css",
        }
    }

    fn extensions(&self) -> &'static [&'static str] {
        CSS_EXTENSIONS
    }

    fn extract(&self, source: &str) -> Result<Vec<Specifier>, ExtractError> {
        let language: tree_sitter::Language = tree_sitter_css::LANGUAGE.into();
        let captures = support::captures(&language, "css", CSS_QUERY, source)?;

        let mut out = Vec::new();
        for capture in captures {
            let keyword = capture
                .text
                .split(|c: char| c.is_whitespace() || c == '(' || c == '"' || c == '\'')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !IMPORTING_AT_RULES.contains(&keyword.as_str()) {
                continue;
            }
            let text = if capture.name == "keyword" {
                support::statement_tail(source, capture.end_byte)
            } else {
                capture.text.as_str()
            };
            let Some(raw) = support::first_quoted(text) else {
                continue;
            };
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            out.push(Specifier {
                raw: raw.to_string(),
                line: capture.line,
            });
        }
        Ok(out)
    }

    fn resolve(&self, raw: &str, importer: &Path) -> Resolution {
        if is_remote(raw) {
            return Resolution::External(raw.to_string());
        }
        // `~bootstrap/scss/x` and a bare `bootstrap/x` both name a package
        // in `node_modules`, which is never scanned.
        if raw.starts_with('~') {
            return Resolution::External(raw.trim_start_matches('~').to_string());
        }

        let Some(base) = importer.parent() else {
            return Resolution::Unresolved;
        };
        let candidate = support::normalise(&base.join(raw));
        if !support::contained(&self.root, &candidate) {
            return Resolution::Unresolved;
        }

        if let Some(found) =
            support::probe(&candidate, CSS_EXTENSIONS, &["index.scss", "index.css"])
        {
            return Resolution::Internal(found);
        }

        // Sass partials: `@use "buttons"` is `_buttons.scss` on disk. The
        // underscore is never written in the import, so a resolver that
        // does not know this rule misses every partial in the project.
        if let (Some(parent), Some(name)) = (candidate.parent(), candidate.file_name()) {
            let partial = parent.join(format!("_{}", name.to_string_lossy()));
            if let Some(found) = support::probe(&partial, CSS_EXTENSIONS, &[]) {
                return Resolution::Internal(found);
            }
        }

        // A bare name that resolved to nothing local is a package import.
        if !raw.starts_with('.') && !raw.starts_with('/') {
            return Resolution::External(raw.split('/').next().unwrap_or(raw).to_string());
        }
        Resolution::Unresolved
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
    fn a_page_names_its_script_and_stylesheet() {
        let dir = TempDir::new().unwrap();
        let e = HtmlExtractor::new(dir.path());
        let specifiers = e
            .extract(
                r#"<html>
<head>
  <link rel="stylesheet" href="/styles/app.css" />
  <script type="module" src="./main.js"></script>
</head>
<body><img src="logo.png"><a href="about.html">About</a></body>
</html>"#,
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(
            raws,
            vec!["/styles/app.css", "./main.js", "about.html"],
            "an <img> is decoration, not structure"
        );
    }

    #[test]
    fn a_root_relative_reference_resolves_against_the_project_root() {
        let dir = TempDir::new().unwrap();
        write(&dir, "pages/index.html", "");
        write(&dir, "styles/app.css", "");
        let e = HtmlExtractor::new(dir.path());

        let resolution = e.resolve("/styles/app.css", &root(&dir).join("pages/index.html"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("styles/app.css")),
            "got {resolution:?}"
        );
    }

    /// The case KOG's own repository produced: a Vite app whose page lives
    /// in `app/index.html` and is served from `app/`, so `/src/main.tsx` is
    /// `app/src/main.tsx` and `/favicon.svg` is `app/public/favicon.svg`.
    /// Resolving both against the repository root reported two broken
    /// references that are not broken at all.
    #[test]
    fn a_root_relative_reference_finds_the_document_root_and_its_public_directory() {
        let dir = TempDir::new().unwrap();
        write(&dir, "app/index.html", "");
        write(&dir, "app/src/main.tsx", "");
        write(&dir, "app/public/favicon.svg", "");
        let e = HtmlExtractor::new(dir.path());
        let importer = root(&dir).join("app/index.html");

        assert!(
            matches!(e.resolve("/src/main.tsx", &importer), Resolution::Internal(ref p) if p.ends_with("app/src/main.tsx"))
        );
        assert!(
            matches!(e.resolve("/favicon.svg", &importer), Resolution::Internal(ref p) if p.ends_with("app/public/favicon.svg"))
        );
    }

    #[test]
    fn a_cache_busting_query_is_stripped_before_probing() {
        let dir = TempDir::new().unwrap();
        write(&dir, "index.html", "");
        write(&dir, "app.js", "");
        let e = HtmlExtractor::new(dir.path());
        assert!(matches!(
            e.resolve("./app.js?v=3", &root(&dir).join("index.html")),
            Resolution::Internal(_)
        ));
    }

    #[test]
    fn a_url_a_template_expression_and_an_anchor_are_never_probed() {
        let dir = TempDir::new().unwrap();
        write(&dir, "index.html", "");
        let e = HtmlExtractor::new(dir.path());
        let importer = root(&dir).join("index.html");
        for raw in [
            "https://cdn.example.com/app.js",
            "//cdn.example.com/app.js",
            "#main",
            "mailto:a@b.c",
            "{{ url_for('static') }}",
            // SvelteKit substitutes this at build time; no file is named that.
            "%sveltekit.assets%/favicon.png",
        ] {
            assert!(
                matches!(e.resolve(raw, &importer), Resolution::External(_)),
                "{raw} should be external"
            );
        }
    }

    #[test]
    fn an_at_import_and_an_at_use_are_both_extracted() {
        let dir = TempDir::new().unwrap();
        let e = CssExtractor::new(dir.path());
        let specifiers = e
            .extract(
                "@import \"reset.css\";\n@use \"buttons\";\n@media (min-width: 40em) { a { color: red } }\n",
            )
            .unwrap();
        let raws: Vec<&str> = specifiers.iter().map(|s| s.raw.as_str()).collect();
        assert_eq!(raws, vec!["reset.css", "buttons"], "@media names no file");
    }

    #[test]
    fn a_sass_partial_resolves_through_its_underscore() {
        let dir = TempDir::new().unwrap();
        write(&dir, "styles/app.scss", "@use \"buttons\";");
        write(&dir, "styles/_buttons.scss", "");
        let e = CssExtractor::new(dir.path());

        let resolution = e.resolve("buttons", &root(&dir).join("styles/app.scss"));
        assert!(
            matches!(resolution, Resolution::Internal(ref p) if p.ends_with("_buttons.scss")),
            "got {resolution:?}"
        );
    }

    #[test]
    fn a_package_stylesheet_is_external() {
        let dir = TempDir::new().unwrap();
        write(&dir, "styles/app.scss", "");
        let e = CssExtractor::new(dir.path());
        let importer = root(&dir).join("styles/app.scss");
        assert_eq!(
            e.resolve("~bootstrap/scss/bootstrap", &importer),
            Resolution::External("bootstrap/scss/bootstrap".into())
        );
        assert_eq!(
            e.resolve("normalize.css/normalize", &importer),
            Resolution::External("normalize.css".into())
        );
    }

    #[test]
    fn a_stylesheet_dialect_is_labelled_by_its_own_name() {
        let dir = TempDir::new().unwrap();
        let e = CssExtractor::new(dir.path());
        assert_eq!(e.lang_for(Path::new("a.css")), "css");
        assert_eq!(e.lang_for(Path::new("a.scss")), "sass");
        assert_eq!(e.lang_for(Path::new("a.less")), "less");
    }
}
