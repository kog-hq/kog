//! Shared machinery for language front ends.
//!
//! Every extractor does the same three things — parse with a grammar, pull
//! the captured text out of a query, and probe the filesystem for a
//! candidate path. Doing them in one place keeps a new language to the part
//! that is actually language-specific: its resolution rules.

use crate::extractor::{ExtractError, Specifier};
use std::fs;
use std::path::{Path, PathBuf};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

/// One node a query captured, with everything an extractor needs to decide
/// what it is.
pub struct Capture {
    /// The node's source text, verbatim — quotes and all.
    pub text: String,
    /// One-based line of the node's first character.
    pub line: usize,
    /// Byte offset just past the node, for the rare case where the grammar
    /// stops short of the construct — a Sass `@use` whose argument the CSS
    /// grammar does not model, and which therefore has to be read from the
    /// source that follows.
    pub end_byte: usize,
    /// The capture name it matched, without the leading `@`. Lets one query
    /// serve several constructs (`@use` and `@mod`, `@src` and `@href`)
    /// without a second parse of the same file.
    pub name: String,
}

/// Run `query_source` over `source`, parsed with `language`.
///
/// A grammar or query that fails to load is an [`ExtractError::Grammar`]:
/// the extractor is broken, not the file, and the distinction matters
/// because a file-level failure is recorded per file while a broken query
/// would silently zero out a whole language.
pub fn captures(
    language: &Language,
    lang_name: &'static str,
    query_source: &str,
    source: &str,
) -> Result<Vec<Capture>, ExtractError> {
    let mut parser = Parser::new();
    parser
        .set_language(language)
        .map_err(|_| ExtractError::Grammar(lang_name))?;
    let tree = parser.parse(source, None).ok_or(ExtractError::Parse)?;
    let query = Query::new(language, query_source).map_err(|_| ExtractError::Grammar(lang_name))?;
    let names = query.capture_names().to_vec();

    let mut cursor = QueryCursor::new();
    // `matches` is a StreamingIterator: a `for` loop does not compile.
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let text = source[capture.node.byte_range()].to_string();
            out.push(Capture {
                text,
                line: capture.node.start_position().row + 1,
                end_byte: capture.node.end_byte(),
                name: names
                    .get(capture.index as usize)
                    .map(|n| (*n).to_string())
                    .unwrap_or_default(),
            });
        }
    }
    Ok(out)
}

/// The same query, with each match's captures kept together.
///
/// Needed whenever a construct is only interesting in combination — a Ruby
/// `require` is a method name *and* its string argument, an HTML `src` is an
/// attribute *and* the tag it sits on. Flattening those into one list would
/// lose which belongs with which.
pub fn grouped_captures(
    language: &Language,
    lang_name: &'static str,
    query_source: &str,
    source: &str,
) -> Result<Vec<Vec<Capture>>, ExtractError> {
    let mut parser = Parser::new();
    parser
        .set_language(language)
        .map_err(|_| ExtractError::Grammar(lang_name))?;
    let tree = parser.parse(source, None).ok_or(ExtractError::Parse)?;
    let query = Query::new(language, query_source).map_err(|_| ExtractError::Grammar(lang_name))?;
    let names = query.capture_names().to_vec();

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        out.push(
            m.captures
                .iter()
                .map(|capture| Capture {
                    text: source[capture.node.byte_range()].to_string(),
                    line: capture.node.start_position().row + 1,
                    end_byte: capture.node.end_byte(),
                    name: names
                        .get(capture.index as usize)
                        .map(|n| (*n).to_string())
                        .unwrap_or_default(),
                })
                .collect(),
        );
    }
    Ok(out)
}

/// The text of the capture named `name` in one match's capture group.
pub fn named<'a>(group: &'a [Capture], name: &str) -> Option<&'a Capture> {
    group.iter().find(|c| c.name == name)
}

/// The remainder of a statement starting at `from`: everything up to the
/// next `;`, `{` or line break.
///
/// The escape hatch for a grammar that recognises a construct but not its
/// argument — `tree-sitter-css` parses Sass's `@use "buttons";` as an
/// at-rule holding only the keyword, leaving the string in an ERROR node
/// beside it. Reading forward from the keyword recovers it without
/// abandoning the parser for a line scanner.
pub fn statement_tail(source: &str, from: usize) -> &str {
    let rest = &source[from.min(source.len())..];
    let end = rest.find([';', '{', '\n']).unwrap_or(rest.len());
    &rest[..end]
}

/// The first single- or double-quoted string in a fragment of source, with
/// its quotes removed. For the constructs whose grammar node is coarser
/// than the thing we need (`@import url("x.css")`, an `@use` at-rule).
pub fn first_quoted(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let open = bytes.iter().position(|b| *b == b'"' || *b == b'\'')?;
    let quote = bytes[open];
    let close = open + 1 + bytes[open + 1..].iter().position(|b| *b == quote)?;
    Some(&text[open + 1..close])
}

/// Strip the quotes a grammar leaves on a string literal, in every style
/// the supported languages use: `"…"`, `'…'`, `` `…` `` (Go raw strings and
/// JavaScript template literals) and C's `<…>` system-include brackets.
pub fn unquote(text: &str) -> &str {
    text.trim_matches(|c| c == '"' || c == '\'' || c == '`' || c == '<' || c == '>')
}

/// Turn captures into specifiers, dropping anything that unquotes to
/// nothing — an empty specifier resolves to nowhere by definition and would
/// only pollute the diagnostics.
pub fn specifiers_from(captures: Vec<Capture>) -> Vec<Specifier> {
    captures
        .into_iter()
        .filter_map(|c| {
            let raw = unquote(&c.text).trim().to_string();
            (!raw.is_empty()).then_some(Specifier { raw, line: c.line })
        })
        .collect()
}

/// Probe a candidate path: the exact file, then the same path with each
/// extension appended, then the directory's index file under each of
/// `index_names`.
///
/// This is the shape every language's resolver needs, differing only in the
/// extensions and index names it uses: `index.ts` for TypeScript,
/// `__init__.py` for Python, `mod.rs` for Rust.
pub fn probe(candidate: &Path, extensions: &[&str], index_names: &[&str]) -> Option<PathBuf> {
    if candidate.is_file() {
        return Some(candidate.to_path_buf());
    }
    for extension in extensions {
        let mut probed = candidate.to_path_buf();
        let name = probed.file_name()?.to_str()?.to_string();
        probed.set_file_name(format!("{name}.{extension}"));
        if probed.is_file() {
            return Some(probed);
        }
    }
    if candidate.is_dir() {
        for index in index_names {
            let probed = candidate.join(index);
            if probed.is_file() {
                return Some(probed);
            }
        }
    }
    None
}

/// Every file directly inside `dir` (not recursively) whose extension is
/// one of `extensions`, sorted.
///
/// What a language whose imports name a *directory* resolves to: Go's
/// package, C#'s namespace folder. Sorted so the resulting edge set is
/// identical between runs.
pub fn files_in_dir(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| extensions.contains(&e))
        })
        .collect();
    files.sort();
    files
}

/// Lexical `.`/`..` collapsing, for candidate paths that may not exist.
pub fn normalise(path: &Path) -> PathBuf {
    crate::tsconfig::normalise_public(path)
}

/// Whether `path` is inside `root`, lexically. Used to confine every
/// resolver to the tree it was told to scan: a config file that ships with
/// the repository (a tsconfig alias, a composer autoload map, a Go module
/// path) can otherwise name a directory outside the project entirely.
pub fn contained(root: &Path, path: &Path) -> bool {
    normalise(path).starts_with(normalise(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn unquote_handles_every_literal_style_the_languages_use() {
        assert_eq!(unquote(r#""fmt""#), "fmt");
        assert_eq!(unquote("'os'"), "os");
        assert_eq!(unquote("`raw`"), "raw");
        assert_eq!(unquote("<stdio.h>"), "stdio.h");
        assert_eq!(unquote("bare"), "bare");
    }

    #[test]
    fn probe_finds_the_exact_file_then_an_extension_then_an_index() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("pkg")).unwrap();
        fs::write(dir.path().join("exact.py"), "").unwrap();
        fs::write(dir.path().join("pkg/__init__.py"), "").unwrap();

        assert!(probe(&dir.path().join("exact.py"), &["py"], &[]).is_some());
        assert!(probe(&dir.path().join("exact"), &["py"], &[]).is_some());
        assert!(probe(&dir.path().join("pkg"), &["py"], &["__init__.py"]).is_some());
        assert!(probe(&dir.path().join("ghost"), &["py"], &["__init__.py"]).is_none());
    }

    #[test]
    fn files_in_dir_is_not_recursive_and_is_sorted() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("b.go"), "").unwrap();
        fs::write(dir.path().join("a.go"), "").unwrap();
        fs::write(dir.path().join("c.md"), "").unwrap();
        fs::write(dir.path().join("sub/d.go"), "").unwrap();

        let files = files_in_dir(dir.path(), &["go"]);
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("a.go"));
        assert!(files[1].ends_with("b.go"));
    }

    #[test]
    fn containment_is_lexical_and_rejects_a_path_that_walks_out() {
        let root = Path::new("/project");
        assert!(contained(root, Path::new("/project/src/a.rs")));
        assert!(!contained(root, Path::new("/project/../elsewhere/a.rs")));
        assert!(!contained(root, Path::new("/elsewhere/a.rs")));
    }
}
