//! Recognition and rendering of [`simple-mermaid`](https://docs.rs/simple-mermaid) invocations.
//!
//! Only the grammar accepted by `simple_mermaid::mermaid!` is recognized: a single string literal
//! path, optionally followed by an alignment keyword (`left`, `center`, `right`) and/or `framed`.
//! No macro is ever executed; the invocation is interpreted syntactically.

use proc_macro2::{Span, TokenStream, TokenTree};
use std::fmt;
use syn::Lit;

/// Horizontal alignment requested by a `mermaid!` invocation.
///
/// Markdown fenced code blocks cannot express alignment, so this is validated and recorded but
/// does not affect the generated README.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    /// `left`.
    Left,
    /// `center`, or no alignment keyword.
    #[default]
    Center,
    /// `right`.
    Right,
}

/// A syntactically valid `mermaid!` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidInvocation {
    /// The diagram path, exactly as written in the string literal.
    pub path: String,
    /// Requested alignment.
    pub alignment: Alignment,
    /// Whether `framed` was requested.
    pub framed: bool,
    /// Location of the invocation in its source file.
    pub location: Location,
}

/// One-based line and column of a source position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Location {
    /// One-based line number.
    pub line: usize,
    /// One-based column number.
    pub column: usize,
}

impl Location {
    pub(crate) fn of(span: Span) -> Location {
        let start = span.start();
        Location {
            line: start.line,
            column: start.column + 1,
        }
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// Returns whether `path` names the `simple-mermaid` macro.
///
/// Accepted forms are `mermaid`, `simple_mermaid::mermaid` and `::simple_mermaid::mermaid`.
/// Renamed imports (`use simple_mermaid::mermaid as diagram`) are not resolved.
#[must_use]
pub fn is_mermaid_macro_path(path: &syn::Path) -> bool {
    let plain = path.segments.iter().all(|s| s.arguments.is_none());
    let idents: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();

    plain
        && match idents.as_slice() {
            [single] => single == "mermaid" && path.leading_colon.is_none(),
            [krate, name] => krate == "simple_mermaid" && name == "mermaid",
            _ => false,
        }
}

/// Returns whether `tokens` contain something that looks like a `mermaid!` invocation anywhere,
/// including nested groups.
///
/// This is used to report invocations in positions that are not supported (for instance inside
/// `cfg_attr` or `concat!`) instead of silently dropping them.
#[must_use]
pub fn mentions_mermaid(tokens: TokenStream) -> Option<Span> {
    let mut previous_mermaid: Option<Span> = None;

    for tree in tokens {
        match tree {
            TokenTree::Ident(ident) if ident == "mermaid" => previous_mermaid = Some(ident.span()),
            TokenTree::Punct(punct) if punct.as_char() == '!' && previous_mermaid.is_some() => {
                return previous_mermaid;
            }
            TokenTree::Group(group) => {
                if let Some(span) = mentions_mermaid(group.stream()) {
                    return Some(span);
                }
                previous_mermaid = None;
            }
            _ => previous_mermaid = None,
        }
    }

    None
}

/// Error produced when a `mermaid!` invocation does not follow the supported grammar.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{location}: unsupported mermaid! invocation: {reason}")]
pub struct UnsupportedInvocation {
    /// Where the invocation is.
    pub location: Location,
    /// Human-readable explanation, including the supported syntax.
    pub reason: String,
}

pub(crate) const SUPPORTED_SYNTAX: &str = "expected `mermaid!(\"path\")`, optionally followed by \
     one of `left`/`center`/`right` and/or `framed`";

/// Parses the arguments of a `mermaid!` macro invocation.
pub fn parse_invocation(mac: &syn::Macro) -> Result<MermaidInvocation, UnsupportedInvocation> {
    let location = Location::of(
        mac.path
            .segments
            .first()
            .map_or_else(Span::call_site, |s| s.ident.span()),
    );
    let unsupported =
        |location: Location, reason: String| UnsupportedInvocation { location, reason };

    let mut tokens = mac.tokens.clone().into_iter();

    let path = match tokens.next() {
        Some(TokenTree::Literal(literal)) => match Lit::new(literal.clone()) {
            Lit::Str(lit) => lit.value(),
            _ => {
                return Err(unsupported(
                    Location::of(literal.span()),
                    format!(
                        "diagram path must be a string literal, found `{literal}`; {SUPPORTED_SYNTAX}"
                    ),
                ));
            }
        },
        Some(other) => {
            return Err(unsupported(
                Location::of(other.span()),
                format!(
                    "diagram path must be a string literal, found `{other}` (macros and constants \
                     are not evaluated); {SUPPORTED_SYNTAX}"
                ),
            ));
        }
        None => {
            return Err(unsupported(
                location,
                format!("missing diagram path; {SUPPORTED_SYNTAX}"),
            ));
        }
    };

    let mut alignment: Option<Alignment> = None;
    let mut framed = false;

    for token in tokens {
        let TokenTree::Ident(ident) = &token else {
            return Err(unsupported(
                Location::of(token.span()),
                format!("unexpected token `{token}`; {SUPPORTED_SYNTAX}"),
            ));
        };

        let keyword = ident.to_string();
        let new_alignment = match keyword.as_str() {
            "left" => Some(Alignment::Left),
            "center" => Some(Alignment::Center),
            "right" => Some(Alignment::Right),
            "framed" if !framed => {
                framed = true;
                None
            }
            "framed" => {
                return Err(unsupported(
                    Location::of(ident.span()),
                    "`framed` given more than once".to_owned(),
                ));
            }
            _ => {
                return Err(unsupported(
                    Location::of(ident.span()),
                    format!("unknown option `{keyword}`; {SUPPORTED_SYNTAX}"),
                ));
            }
        };

        if let Some(new_alignment) = new_alignment {
            if alignment.is_some() {
                return Err(unsupported(
                    Location::of(ident.span()),
                    "only one of `left`, `center` and `right` may be given".to_owned(),
                ));
            }
            alignment = Some(new_alignment);
        }
    }

    Ok(MermaidInvocation {
        path,
        alignment: alignment.unwrap_or_default(),
        framed,
        location,
    })
}

/// Renders diagram source as the lines of a fenced `mermaid` Markdown code block.
///
/// Line endings are normalized to `\n` and trailing line breaks are removed. The fence is made of
/// backticks and is always longer than the longest run of backticks in the diagram, so the diagram
/// can never close the block early.
#[must_use]
pub fn render_fenced_block(diagram: &str) -> Vec<String> {
    let normalized = diagram.replace("\r\n", "\n");
    let body = normalized.trim_end_matches(['\n', '\r']);
    let fence = "`".repeat(longest_backtick_run(body).max(2) + 1);

    let mut lines = Vec::with_capacity(body.lines().count() + 2);
    lines.push(format!("{fence}mermaid"));
    if !body.is_empty() {
        lines.extend(body.split('\n').map(ToOwned::to_owned));
    }
    lines.push(fence);
    lines
}

fn longest_backtick_run(text: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;

    for c in text.chars() {
        if c == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }

    longest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invocation(src: &str) -> Result<MermaidInvocation, UnsupportedInvocation> {
        let mac: syn::Macro = syn::parse_str(src).unwrap();
        parse_invocation(&mac)
    }

    #[test]
    fn recognizes_macro_paths() {
        for (path, expected) in [
            ("mermaid", true),
            ("simple_mermaid::mermaid", true),
            ("::simple_mermaid::mermaid", true),
            ("::mermaid", false),
            ("other::mermaid", false),
            ("simple_mermaid::other", false),
            ("a::simple_mermaid::mermaid", false),
        ] {
            let path: syn::Path = syn::parse_str(path).unwrap();
            assert_eq!(is_mermaid_macro_path(&path), expected, "{path:?}");
        }
    }

    #[test]
    fn parses_plain_path() {
        let inv = invocation(r#"mermaid!("diagrams/a.mmd")"#).unwrap();
        assert_eq!(inv.path, "diagrams/a.mmd");
        assert_eq!(inv.alignment, Alignment::Center);
        assert!(!inv.framed);
    }

    #[test]
    fn parses_raw_string_and_other_delimiters() {
        assert_eq!(
            invocation(r##"mermaid![r#"a.mmd"#]"##).unwrap().path,
            "a.mmd"
        );
        assert_eq!(invocation(r#"mermaid!{"a.mmd"}"#).unwrap().path, "a.mmd");
    }

    #[test]
    fn parses_options_in_any_order() {
        let inv = invocation(r#"mermaid!("a.mmd" framed right)"#).unwrap();
        assert_eq!((inv.alignment, inv.framed), (Alignment::Right, true));
        let inv = invocation(r#"mermaid!("a.mmd" left)"#).unwrap();
        assert_eq!((inv.alignment, inv.framed), (Alignment::Left, false));
        let inv = invocation(r#"mermaid!("a.mmd" framed)"#).unwrap();
        assert_eq!((inv.alignment, inv.framed), (Alignment::Center, true));
    }

    #[test]
    #[allow(clippy::needless_raw_string_hashes)]
    fn rejects_unsupported_forms() {
        for src in [
            r#"mermaid!()"#,
            r#"mermaid!(concat!("a", ".mmd"))"#,
            r#"mermaid!(PATH)"#,
            r#"mermaid!(b"a.mmd")"#,
            r#"mermaid!("a.mmd", left)"#,
            r#"mermaid!("a.mmd" left right)"#,
            r#"mermaid!("a.mmd" framed framed)"#,
            r#"mermaid!("a.mmd" transparent)"#,
            r#"mermaid!("a.mmd" "b.mmd")"#,
        ] {
            let err = invocation(src).unwrap_err();
            assert!(!err.reason.is_empty(), "{src}");
        }
    }

    #[test]
    fn detects_nested_mentions() {
        let tokens: TokenStream = r#"cfg_attr(doc, doc = simple_mermaid::mermaid!("a.mmd"))"#
            .parse()
            .unwrap();
        assert!(mentions_mermaid(tokens).is_some());
        let tokens: TokenStream = r#"doc = "mermaid is nice! really""#.parse().unwrap();
        assert!(mentions_mermaid(tokens).is_none());
        let tokens: TokenStream = "doc(hidden)".parse().unwrap();
        assert!(mentions_mermaid(tokens).is_none());
    }

    #[test]
    fn renders_fence_and_strips_trailing_newlines() {
        assert_eq!(
            render_fenced_block("flowchart TD\r\nA --> B\r\n\n"),
            ["```mermaid", "flowchart TD", "A --> B", "```"]
        );
        assert_eq!(render_fenced_block(""), ["```mermaid", "```"]);
    }

    #[test]
    fn lengthens_fence_for_backticks() {
        let lines = render_fenced_block("A[\"`md`\"]\n```\n");
        assert_eq!(lines.first().unwrap(), "````mermaid");
        assert_eq!(lines.last().unwrap(), "````");
        let lines = render_fenced_block("x ````` y");
        assert_eq!(lines.last().unwrap(), "``````");
    }
}
