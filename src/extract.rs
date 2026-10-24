//! Crate-level documentation extraction with inline Mermaid expansion.
//!
//! This mirrors `cargo_rdme::extract_doc_from_source_file`, which only keeps string-literal
//! `#![doc = "..."]` attributes (`//!` and `/*! */` comments desugar to those). In addition, it
//! recognizes `#![doc = simple_mermaid::mermaid!("path")]` and replaces it, at the same position,
//! with a fenced `mermaid` block containing the referenced file.
//!
//! The source is parsed with `syn`, so text inside doc comments (for instance a code example
//! showing the macro) is never mistaken for an attribute.

use crate::mermaid::{self, Location, MermaidInvocation, UnsupportedInvocation};
use cargo_rdme::Doc;
use quote::ToTokens;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use syn::{AttrStyle, Expr, ExprLit, Lit, Meta, MetaNameValue};

/// A piece of crate-level documentation, in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocSegment {
    /// Lines from a string-literal doc attribute (including `//!` and `/*! */` comments).
    Text(Vec<String>),
    /// A `mermaid!` invocation.
    Mermaid(MermaidInvocation),
}

/// Errors produced while extracting documentation.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// The source file could not be read.
    #[error("cannot read source file \"{path}\": {source}")]
    ReadSource {
        /// Source file path.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The source file is not valid Rust.
    #[error("cannot parse source file \"{path}\": {source}")]
    ParseSource {
        /// Source file path.
        path: PathBuf,
        /// Underlying error.
        source: syn::Error,
    },
    /// A `mermaid!` invocation uses syntax that is not supported.
    #[error("{}:{source}", path.display())]
    Unsupported {
        /// Source file path.
        path: PathBuf,
        /// Details.
        source: UnsupportedInvocation,
    },
    /// A diagram file referenced by `mermaid!` could not be read.
    #[error(
        "{}:{location}: cannot read mermaid diagram \"{literal}\" (resolved to \"{}\"): {source}",
        source_path.display(),
        resolved.display()
    )]
    ReadDiagram {
        /// Rust source file containing the invocation.
        source_path: PathBuf,
        /// Location of the invocation.
        location: Location,
        /// Path as written in the invocation.
        literal: String,
        /// Path after resolution relative to the source file.
        resolved: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
}

/// A diagram that was expanded into the documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedDiagram {
    /// The resolved diagram path.
    pub path: PathBuf,
    /// The invocation that referenced it.
    pub invocation: MermaidInvocation,
}

/// Documentation extracted from an entry file.
#[derive(Debug, Clone)]
pub struct ExtractedDoc {
    /// The crate documentation as Markdown, with diagrams expanded in place.
    pub doc: Doc,
    /// Every diagram expansion, in source order (repeated references appear repeatedly).
    pub diagrams: Vec<ExpandedDiagram>,
}

/// Splits the crate-level documentation of `source` into ordered segments.
///
/// `path` is only used in error messages.
pub fn doc_segments(source: &str, path: &Path) -> Result<Vec<DocSegment>, ExtractError> {
    let file = syn::parse_file(source).map_err(|source| ExtractError::ParseSource {
        path: path.to_owned(),
        source,
    })?;
    let unsupported = |source: UnsupportedInvocation| ExtractError::Unsupported {
        path: path.to_owned(),
        source,
    };

    let mut segments = Vec::new();

    for attr in file
        .attrs
        .iter()
        .filter(|a| matches!(a.style, AttrStyle::Inner(_)))
    {
        let segment = match &attr.meta {
            Meta::NameValue(MetaNameValue {
                path: attr_path,
                value,
                ..
            }) if attr_path.is_ident("doc") => {
                match value {
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(lit), ..
                    }) => Some(DocSegment::Text(literal_doc_lines(&lit.value()))),
                    Expr::Macro(expr) if mermaid::is_mermaid_macro_path(&expr.mac.path) => {
                        Some(DocSegment::Mermaid(
                            mermaid::parse_invocation(&expr.mac).map_err(unsupported)?,
                        ))
                    }
                    // Other expressions (e.g. `include_str!`) are skipped, as cargo-rdme does.
                    _ => None,
                }
            }
            _ => None,
        };

        match segment {
            Some(segment) => segments.push(segment),
            None => {
                if let Some(span) = mermaid::mentions_mermaid(attr.meta.to_token_stream()) {
                    return Err(unsupported(UnsupportedInvocation {
                        location: Location::of(span),
                        reason: format!(
                            "mermaid! must be the whole value of a crate-level `#![doc = ...]` \
                             attribute (it is not expanded inside `cfg_attr`, other macros or \
                             expressions); {}",
                            mermaid::SUPPORTED_SYNTAX
                        ),
                    }));
                }
            }
        }
    }

    Ok(segments)
}

/// Converts the value of one string-literal doc attribute to lines, exactly like cargo-rdme.
fn literal_doc_lines(string: &str) -> Vec<String> {
    match string.lines().count() {
        0 => vec![String::new()],
        1 => vec![string.strip_prefix(' ').unwrap_or(string).to_owned()],
        // Block comment: drop a leading blank line, keep everything else verbatim.
        _ => string
            .lines()
            .enumerate()
            .filter(|(i, l)| !(*i == 0 && l.chars().all(char::is_whitespace)))
            .map(|(_, l)| l.to_owned())
            .collect(),
    }
}

/// Resolves a diagram path the way `include_str!` does: relative to the directory of the file
/// containing the invocation, unless it is absolute.
#[must_use]
pub fn resolve_diagram_path(source_file: &Path, literal: &str) -> PathBuf {
    let base = source_file.parent().unwrap_or_else(|| Path::new(""));
    base.join(literal)
}

/// Builds the documentation from segments, reading diagrams with `read_diagram`.
pub fn expand_segments(
    segments: Vec<DocSegment>,
    source_path: &Path,
    mut read_diagram: impl FnMut(&Path) -> std::io::Result<String>,
) -> Result<Option<ExtractedDoc>, ExtractError> {
    if segments.is_empty() {
        return Ok(None);
    }

    let mut lines: Vec<String> = Vec::new();
    let mut diagrams = Vec::new();
    let mut cache: HashMap<PathBuf, String> = HashMap::new();

    for segment in segments {
        match segment {
            DocSegment::Text(text) => lines.extend(text),
            DocSegment::Mermaid(invocation) => {
                let resolved = resolve_diagram_path(source_path, &invocation.path);
                let content = if let Some(content) = cache.get(&resolved) {
                    content.clone()
                } else {
                    let content =
                        read_diagram(&resolved).map_err(|source| ExtractError::ReadDiagram {
                            source_path: source_path.to_owned(),
                            location: invocation.location,
                            literal: invocation.path.clone(),
                            resolved: resolved.clone(),
                            source,
                        })?;
                    cache.insert(resolved.clone(), content.clone());
                    content
                };

                lines.extend(mermaid::render_fenced_block(&content));
                diagrams.push(ExpandedDiagram {
                    path: resolved,
                    invocation,
                });
            }
        }
    }

    Ok(Some(ExtractedDoc {
        doc: Doc::from_str(lines.join("\n")),
        diagrams,
    }))
}

/// Extracts the crate-level documentation of the Rust file at `path`, expanding diagrams.
///
/// Returns `Ok(None)` if the file has no crate-level documentation.
pub fn extract_doc_from_source_file(path: &Path) -> Result<Option<ExtractedDoc>, ExtractError> {
    let source = std::fs::read_to_string(path).map_err(|source| ExtractError::ReadSource {
        path: path.to_owned(),
        source,
    })?;
    let segments = doc_segments(&source, path)?;

    expand_segments(segments, path, |p| std::fs::read_to_string(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(src: &str) -> Vec<DocSegment> {
        doc_segments(src, Path::new("src/lib.rs")).unwrap()
    }

    fn expand(src: &str, files: &[(&str, &str)]) -> Result<Option<ExtractedDoc>, ExtractError> {
        let files: HashMap<PathBuf, String> = files
            .iter()
            .map(|(p, c)| (PathBuf::from(p), (*c).to_owned()))
            .collect();
        let path = Path::new("src/lib.rs");
        expand_segments(doc_segments(src, path)?, path, |p| {
            files
                .get(p)
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        })
    }

    #[test]
    fn literal_docs_match_cargo_rdme() {
        let src = "#![no_std]\n//! Title\n//!No space\n//!\n//!   indented\n/*!\nblock\n second\n*/\nfn f() {}\n";
        assert_eq!(
            segments(src),
            [
                DocSegment::Text(vec!["Title".into()]),
                DocSegment::Text(vec!["No space".into()]),
                DocSegment::Text(vec![String::new()]),
                DocSegment::Text(vec!["  indented".into()]),
                DocSegment::Text(vec!["block".into(), " second".into()]),
            ]
        );
    }

    #[test]
    fn no_docs_is_none() {
        assert!(expand("fn f() {}", &[]).unwrap().is_none());
    }

    #[test]
    fn inline_mermaid_at_position() {
        let src = "//! # A\n//!\n#![doc = simple_mermaid::mermaid!(\"d.mmd\")]\n//!\n//! # B\n";
        let doc = expand(src, &[("src/d.mmd", "graph TD\nA-->B\n")])
            .unwrap()
            .unwrap();
        assert_eq!(
            doc.doc.as_string(),
            "# A\n\n```mermaid\ngraph TD\nA-->B\n```\n\n# B\n"
        );
    }

    #[test]
    fn repeated_references_read_once_and_expand_twice() {
        let src = "#![doc = mermaid!(\"d.mmd\")]\n//! mid\n#![doc = mermaid!(\"d.mmd\")]\n";
        let mut reads = 0;
        let path = Path::new("src/lib.rs");
        let doc = expand_segments(segments(src), path, |_| {
            reads += 1;
            Ok("x".to_owned())
        })
        .unwrap()
        .unwrap();
        assert_eq!(reads, 1);
        assert_eq!(doc.diagrams.len(), 2);
        assert_eq!(
            doc.doc.as_string(),
            "```mermaid\nx\n```\nmid\n```mermaid\nx\n```\n"
        );
    }

    #[test]
    fn macro_text_inside_doc_examples_is_text() {
        let src = "//! ```\n//! #![doc = simple_mermaid::mermaid!(\"missing.mmd\")]\n//! ```\n/// #[doc = mermaid!(\"x\")]\nfn f() {}\n";
        let doc = expand(src, &[]).unwrap().unwrap();
        assert!(doc.diagrams.is_empty());
        assert!(doc.doc.as_string().contains("mermaid!(\"missing.mmd\")"));
    }

    #[test]
    fn item_level_and_non_mermaid_macros_are_ignored() {
        let src = "//! top\n#![doc = include_str!(\"../README.md\")]\n#[doc = mermaid!(\"item.mmd\")]\nfn f() {}\n";
        let doc = expand(src, &[]).unwrap().unwrap();
        assert_eq!(doc.doc.as_string(), "top\n");
    }

    #[test]
    fn mermaid_in_cfg_attr_is_reported() {
        let src = "//! x\n#![cfg_attr(doc, doc = simple_mermaid::mermaid!(\"d.mmd\"))]\n";
        let err = doc_segments(src, Path::new("src/lib.rs")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("src/lib.rs:2:"), "{msg}");
        assert!(msg.contains("cfg_attr"), "{msg}");
    }

    #[test]
    fn missing_diagram_is_reported_with_paths() {
        let src = "//! x\n#![doc = mermaid!(\"../docs/nope.mmd\")]\n";
        let msg = expand(src, &[]).unwrap_err().to_string();
        assert!(msg.contains("src/lib.rs:2:"), "{msg}");
        assert!(msg.contains("\"../docs/nope.mmd\""), "{msg}");
        let resolved = Path::new("src").join("../docs/nope.mmd");
        assert!(msg.contains(&resolved.display().to_string()), "{msg}");
    }

    #[test]
    fn invalid_rust_is_reported() {
        assert!(matches!(
            doc_segments("fn {", Path::new("x.rs")),
            Err(ExtractError::ParseSource { .. })
        ));
    }
}
