//! What every file extension is, whether or not KOG can read it.
//!
//! The registry decides which extensions an extractor *claims*. This module
//! decides what an unclaimed extension *was*, which is a different question
//! and the one that makes the coverage report honest: `.scala` and `.png`
//! are both unclaimed, but only one of them is a gap.
//!
//! An extension missing from both tables is reported as unrecognised rather
//! than quietly assumed to be either — the coverage report names it with its
//! count, so a language KOG has never heard of shows up as a line to add
//! here instead of disappearing into a total.

/// What a file extension is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Source code. If no extractor claims it, that is a coverage gap.
    Source,
    /// Documentation, configuration, data, assets, build output. Never a
    /// graph node, and never a gap.
    NotSource,
    /// Neither table knows it. Counted and named, claimed by nobody.
    Unrecognised,
}

/// What one extension is, and which language it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classification {
    pub kind: Kind,
    /// The language or file family, named even when unsupported — an
    /// unsupported entry that does not say *which* language is missing is
    /// not a report, it is a shrug.
    pub lang: Option<&'static str>,
}

/// Source-code extensions, with the language they belong to. Being listed
/// here says nothing about whether KOG can read the language: that is the
/// registry's business. It says that a file with this extension is code,
/// so leaving it out of the graph is a gap worth naming.
const SOURCE: &[(&str, &str)] = &[
    // — Web / JS family —
    ("ts", "TypeScript"),
    ("tsx", "TypeScript"),
    ("mts", "TypeScript"),
    ("cts", "TypeScript"),
    ("js", "JavaScript"),
    ("jsx", "JavaScript"),
    ("mjs", "JavaScript"),
    ("cjs", "JavaScript"),
    ("vue", "Vue"),
    ("svelte", "Svelte"),
    ("astro", "Astro"),
    ("mdx", "MDX"),
    // Template languages. Source, and easy to miss: a template renders other
    // templates, so it holds real cross-file references. Filed as "not
    // source" they left the coverage denominator entirely, which published
    // 0.9627 for `sinatra/sinatra` where the honest number is 0.6769 — the
    // unrecognised always flattering the figure is the same defect this tool
    // exists to catch, pointing the other way.
    ("erb", "ERB"),
    ("rhtml", "ERB"),
    ("erubis", "Erubis"),
    ("haml", "Haml"),
    ("hamlit", "Hamlit"),
    ("slim", "Slim"),
    ("liquid", "Liquid"),
    ("mustache", "Mustache"),
    ("hbs", "Handlebars"),
    ("handlebars", "Handlebars"),
    ("ejs", "EJS"),
    ("pug", "Pug"),
    ("jade", "Pug"),
    ("twig", "Twig"),
    ("blade.php", "Blade"),
    ("jinja", "Jinja"),
    ("jinja2", "Jinja"),
    ("j2", "Jinja"),
    ("njk", "Nunjucks"),
    ("tmpl", "Template"),
    ("tpl", "Template"),
    ("html", "HTML"),
    ("htm", "HTML"),
    ("xhtml", "HTML"),
    ("css", "CSS"),
    ("scss", "Sass"),
    ("sass", "Sass"),
    ("less", "Less"),
    ("styl", "Stylus"),
    // — Systems —
    ("go", "Go"),
    ("rs", "Rust"),
    ("c", "C"),
    ("h", "C or C++ header"),
    ("cc", "C++"),
    ("cpp", "C++"),
    ("cxx", "C++"),
    ("c++", "C++"),
    ("hh", "C++"),
    ("hpp", "C++"),
    ("hxx", "C++"),
    ("ipp", "C++"),
    ("inl", "C++"),
    ("zig", "Zig"),
    ("nim", "Nim"),
    ("d", "D"),
    ("cr", "Crystal"),
    ("v", "V or Verilog"),
    ("sv", "SystemVerilog"),
    ("svh", "SystemVerilog"),
    ("vhd", "VHDL"),
    ("vhdl", "VHDL"),
    ("asm", "Assembly"),
    ("s", "Assembly"),
    // — JVM / .NET —
    ("java", "Java"),
    ("kt", "Kotlin"),
    ("kts", "Kotlin"),
    ("scala", "Scala"),
    ("sc", "Scala"),
    ("groovy", "Groovy"),
    ("gradle", "Groovy"),
    ("clj", "Clojure"),
    ("cljs", "Clojure"),
    ("cljc", "Clojure"),
    ("cs", "C#"),
    ("vb", "Visual Basic"),
    ("fs", "F#"),
    ("fsx", "F#"),
    // — Scripting —
    ("py", "Python"),
    ("pyi", "Python"),
    ("pyw", "Python"),
    ("rb", "Ruby"),
    ("rake", "Ruby"),
    ("gemspec", "Ruby"),
    ("php", "PHP"),
    ("pl", "Perl"),
    ("pm", "Perl"),
    ("lua", "Lua"),
    ("tcl", "Tcl"),
    ("awk", "AWK"),
    ("sh", "Shell"),
    ("bash", "Shell"),
    ("zsh", "Shell"),
    ("ksh", "Shell"),
    ("fish", "Fish shell"),
    ("ps1", "PowerShell"),
    ("psm1", "PowerShell"),
    ("bat", "Batch"),
    ("cmd", "Batch"),
    ("vim", "Vimscript"),
    ("el", "Emacs Lisp"),
    // — Mobile / Apple —
    ("swift", "Swift"),
    ("m", "Objective-C"),
    ("mm", "Objective-C++"),
    ("dart", "Dart"),
    // — Functional —
    ("hs", "Haskell"),
    ("lhs", "Haskell"),
    ("ex", "Elixir"),
    ("exs", "Elixir"),
    ("erl", "Erlang"),
    ("hrl", "Erlang"),
    ("ml", "OCaml"),
    ("mli", "OCaml"),
    ("rkt", "Racket"),
    ("scm", "Scheme"),
    ("ss", "Scheme"),
    ("lisp", "Common Lisp"),
    ("cl", "Common Lisp"),
    ("jl", "Julia"),
    ("r", "R"),
    // — Data / infrastructure as code —
    ("sql", "SQL"),
    ("graphql", "GraphQL"),
    ("gql", "GraphQL"),
    ("proto", "Protocol Buffers"),
    ("tf", "Terraform"),
    ("tfvars", "Terraform"),
    ("hcl", "HCL"),
    ("nix", "Nix"),
    ("cmake", "CMake"),
    ("mk", "Make"),
    ("sol", "Solidity"),
    ("wat", "WebAssembly text"),
    // — Legacy —
    ("f90", "Fortran"),
    ("f95", "Fortran"),
    ("for", "Fortran"),
    ("pas", "Pascal"),
    ("ada", "Ada"),
    ("adb", "Ada"),
    ("ads", "Ada"),
    ("cob", "COBOL"),
    ("cbl", "COBOL"),
    ("ino", "Arduino"),
];

/// Extensions that are not source code, with the family they belong to.
/// These never count against coverage: a repository is not worse mapped
/// for containing a README.
// Build-system source. `.m4` and `.am` carry real `include` directives, and
// `.in` is the input a generated file is produced from; treating them as data
// removed 62 files from `curl`'s coverage denominator.
const BUILD_SOURCE: &[(&str, &str)] = &[
    ("m4", "M4"),
    ("am", "Automake"),
    ("ac", "Autoconf"),
    ("mk", "Make"),
];

const NOT_SOURCE: &[(&str, &str)] = &[
    // Data and configuration
    ("json", "data"),
    ("jsonc", "data"),
    ("json5", "data"),
    ("yaml", "data"),
    ("yml", "data"),
    ("toml", "data"),
    ("xml", "data"),
    ("csv", "data"),
    ("tsv", "data"),
    ("ini", "configuration"),
    ("cfg", "configuration"),
    ("conf", "configuration"),
    ("env", "configuration"),
    ("properties", "configuration"),
    ("editorconfig", "configuration"),
    ("lock", "lockfile"),
    ("sum", "lockfile"),
    // Documentation
    ("md", "documentation"),
    ("markdown", "documentation"),
    ("rst", "documentation"),
    ("adoc", "documentation"),
    ("txt", "documentation"),
    ("tex", "documentation"),
    ("pdf", "documentation"),
    ("license", "documentation"),
    // Images and media
    ("svg", "image"),
    ("png", "image"),
    ("jpg", "image"),
    ("jpeg", "image"),
    ("gif", "image"),
    ("webp", "image"),
    ("avif", "image"),
    ("ico", "image"),
    ("bmp", "image"),
    ("tiff", "image"),
    ("psd", "image"),
    ("mp3", "media"),
    ("mp4", "media"),
    ("wav", "media"),
    ("mov", "media"),
    ("webm", "media"),
    ("ogg", "media"),
    ("woff", "font"),
    ("woff2", "font"),
    ("ttf", "font"),
    ("otf", "font"),
    ("eot", "font"),
    // Archives and binaries
    ("zip", "archive"),
    ("tar", "archive"),
    ("gz", "archive"),
    ("bz2", "archive"),
    ("xz", "archive"),
    ("7z", "archive"),
    ("rar", "archive"),
    ("so", "binary"),
    ("dylib", "binary"),
    ("dll", "binary"),
    ("a", "binary"),
    ("lib", "binary"),
    ("o", "binary"),
    ("obj", "binary"),
    ("exe", "binary"),
    ("bin", "binary"),
    ("wasm", "binary"),
    ("class", "binary"),
    ("jar", "binary"),
    ("pyc", "binary"),
    ("pyo", "binary"),
    ("db", "database"),
    ("sqlite", "database"),
    ("sqlite3", "database"),
    // Build and tooling output
    ("map", "build output"),
    ("snap", "test snapshot"),
    ("log", "log"),
    ("patch", "patch"),
    ("diff", "patch"),
    ("d.ts", "declaration"),
];

/// Extensionless files whose *name* identifies them, so `Dockerfile` and
/// `Makefile` are classified rather than lumped into one nameless bucket.
const BY_FILENAME: &[(&str, Kind, &str)] = &[
    ("Makefile", Kind::Source, "Make"),
    ("makefile", Kind::Source, "Make"),
    ("GNUmakefile", Kind::Source, "Make"),
    ("Dockerfile", Kind::Source, "Dockerfile"),
    ("Containerfile", Kind::Source, "Dockerfile"),
    ("Rakefile", Kind::Source, "Ruby"),
    ("Gemfile", Kind::Source, "Ruby"),
    ("Podfile", Kind::Source, "Ruby"),
    ("Brewfile", Kind::Source, "Ruby"),
    ("Vagrantfile", Kind::Source, "Ruby"),
    ("Justfile", Kind::Source, "Just"),
    ("justfile", Kind::Source, "Just"),
    ("LICENSE", Kind::NotSource, "documentation"),
    ("LICENCE", Kind::NotSource, "documentation"),
    ("NOTICE", Kind::NotSource, "documentation"),
    ("CODEOWNERS", Kind::NotSource, "configuration"),
];

/// Classify one file by extension, falling back to its whole name for the
/// extensionless files that carry their identity in the name itself.
pub fn classify(extension: &str, file_name: &str) -> Classification {
    if extension.is_empty() {
        if let Some((_, kind, lang)) = BY_FILENAME.iter().find(|(n, _, _)| *n == file_name) {
            return Classification {
                kind: *kind,
                lang: Some(lang),
            };
        }
    }

    // Extensions are matched case-insensitively: `.PNG` and `.png` are the
    // same thing, and a repository written on a case-insensitive filesystem
    // should not produce two coverage rows for one extension.
    let lower = extension.to_ascii_lowercase();

    if let Some((_, lang)) = SOURCE
        .iter()
        .chain(BUILD_SOURCE.iter())
        .find(|(e, _)| *e == lower)
    {
        return Classification {
            kind: Kind::Source,
            lang: Some(lang),
        };
    }
    if let Some((_, family)) = NOT_SOURCE.iter().find(|(e, _)| *e == lower) {
        return Classification {
            kind: Kind::NotSource,
            lang: Some(family),
        };
    }
    Classification {
        kind: Kind::Unrecognised,
        lang: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_extension_is_named_with_its_language() {
        let c = classify("scala", "Main.scala");
        assert_eq!(c.kind, Kind::Source);
        assert_eq!(c.lang, Some("Scala"));
    }

    #[test]
    fn an_asset_extension_is_not_source_and_not_a_gap() {
        assert_eq!(classify("png", "logo.png").kind, Kind::NotSource);
        assert_eq!(classify("md", "README.md").kind, Kind::NotSource);
    }

    #[test]
    fn an_unknown_extension_is_reported_as_unrecognised_rather_than_guessed() {
        let c = classify("qqq", "thing.qqq");
        assert_eq!(c.kind, Kind::Unrecognised);
        assert_eq!(c.lang, None);
    }

    #[test]
    fn extensionless_files_are_classified_by_name() {
        assert_eq!(classify("", "Dockerfile").lang, Some("Dockerfile"));
        assert_eq!(classify("", "Makefile").kind, Kind::Source);
    }

    /// Found by auditing every extension across an eleven-repository corpus,
    /// rather than only the languages that already had an extractor.
    ///
    /// A template renders other templates, so it holds real cross-file
    /// references and is source by any reading. Filed as "not source" these
    /// left the coverage denominator altogether, and `sinatra/sinatra`
    /// published **0.9627** where the honest figure was **0.6769** — 68 files
    /// of Ruby templates removed from the question rather than counted
    /// against it. An unrecognised extension always flattering the published
    /// number is the same defect this tool exists to catch, pointing the
    /// other way.
    #[test]
    fn a_template_is_source_and_counts_against_coverage() {
        for extension in ["erb", "haml", "slim", "erubis", "hamlit", "hbs", "ejs"] {
            let classification = classify(extension, &format!("view.{extension}"));
            assert_eq!(
                classification.kind,
                Kind::Source,
                ".{extension} must count as source, or it leaves the coverage denominator"
            );
            assert!(classification.lang.is_some(), ".{extension} must be named");
        }
    }

    /// The same for build systems: an `include` in a Makefile fragment or an
    /// autoconf macro is a real cross-file reference. 62 files on `curl`.
    #[test]
    fn build_system_files_are_source_too() {
        for extension in ["m4", "am", "ac", "mk"] {
            assert_eq!(
                classify(extension, &format!("thing.{extension}")).kind,
                Kind::Source,
                ".{extension} must count as source"
            );
        }
        assert_eq!(classify("", "LICENSE").kind, Kind::NotSource);
        assert_eq!(classify("", "whatever").kind, Kind::Unrecognised);
    }

    #[test]
    fn extension_matching_ignores_case() {
        assert_eq!(classify("PNG", "LOGO.PNG").kind, Kind::NotSource);
        assert_eq!(classify("Go", "main.Go").lang, Some("Go"));
    }

    /// The two tables must not disagree about an extension: whichever were
    /// consulted first would win silently, and the coverage report would
    /// depend on table order rather than on fact.
    #[test]
    fn no_extension_is_listed_as_both_source_and_not_source() {
        for (ext, _) in SOURCE {
            assert!(
                !NOT_SOURCE.iter().any(|(e, _)| e == ext),
                "{ext} is listed in both tables"
            );
        }
    }

    #[test]
    fn neither_table_repeats_an_extension() {
        for table in [SOURCE, NOT_SOURCE] {
            let mut seen: Vec<&str> = table.iter().map(|(e, _)| *e).collect();
            let before = seen.len();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(before, seen.len(), "a table repeats an extension");
        }
    }
}
