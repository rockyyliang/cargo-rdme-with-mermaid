//! Composing the final README in memory.
//!
//! The pipeline is: extract docs (with diagrams expanded) → cargo-rdme transforms (hidden Rust
//! lines, Rust code-block tags, intralinks) → cargo-rdme injection between markers (with heading
//! adjustment) → serialization with the selected line terminator. The README is not written here;
//! intralink resolution may invoke Cargo/rustdoc and write build artifacts.

use super::extract::{self, ExpandedDiagram};
use crate::Error;
use crate::config::options::{LineTerminatorOpt, Options};
use crate::project::{Entrypoint, Project};
use cargo_rdme::transform::{
    DocTransform, DocTransformIntralinks, DocTransformRustMarkdownTag,
    DocTransformRustRemoveComments, IntralinksConfig,
};
use cargo_rdme::{
    Doc, InjectDocError, LineTerminator, PackageTarget, Readme, inject_doc_in_readme,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// Everything needed to compare or write the README.
#[derive(Debug)]
pub struct Generated {
    /// The README file.
    pub readme_path: PathBuf,
    /// The README bytes as they were read.
    pub current: Vec<u8>,
    /// The complete new README bytes.
    pub rendered: Vec<u8>,
    /// Line terminator used for `rendered`.
    pub line_terminator: LineTerminator,
    /// Whether the README contained a cargo-rdme marker.
    pub had_marker: bool,
    /// Warnings emitted by the transforms (e.g. unresolved intralinks).
    pub warnings: Vec<String>,
    /// Diagrams expanded into the documentation.
    pub diagrams: Vec<ExpandedDiagram>,
}

impl Generated {
    /// Whether the README on disk already equals the generated README, byte for byte.
    #[must_use]
    pub fn is_up_to_date(&self) -> bool {
        self.current == self.rendered
    }
}

/// Information about the crate needed by cargo-rdme's transforms.
#[derive(Debug)]
pub struct TransformContext<'a> {
    /// Package name.
    pub package_name: &'a str,
    /// Target kind of the entrypoint.
    pub target: PackageTarget,
    /// Workspace package selection, forwarded to rustdoc for intralinks.
    pub workspace_project: Option<&'a str>,
    /// Package manifest.
    pub manifest_path: &'a Path,
    /// Intralink settings.
    pub intralinks: &'a IntralinksConfig,
}

/// Applies cargo-rdme's documentation transforms, returning the new doc and any warnings.
pub fn transform_doc(doc: &Doc, ctx: TransformContext<'_>) -> Result<(Doc, Vec<String>), Error> {
    let Ok(doc) = DocTransformRustRemoveComments::new().transform(doc);
    let Ok(doc) = DocTransformRustMarkdownTag::new().transform(&doc);

    let warnings = RefCell::new(Vec::new());
    let intralinks = DocTransformIntralinks::new(
        ctx.package_name.to_owned(),
        ctx.target,
        ctx.workspace_project.map(ToOwned::to_owned),
        ctx.manifest_path.to_owned(),
        |msg: &str| warnings.borrow_mut().push(msg.to_owned()),
        Some(ctx.intralinks.clone()),
    );
    let doc = intralinks.transform(&doc).map_err(Error::Intralinks)?;

    Ok((doc, warnings.into_inner()))
}

/// Returns the line terminator predominantly used by `text` (LF on ties), like cargo-rdme.
#[must_use]
pub fn infer_line_terminator(text: &str) -> LineTerminator {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    if crlf > lf {
        LineTerminator::CrLf
    } else {
        LineTerminator::Lf
    }
}

/// Resolves the line-terminator option against the current README text.
#[must_use]
pub fn select_line_terminator(opt: LineTerminatorOpt, current_readme: &str) -> LineTerminator {
    match opt {
        LineTerminatorOpt::Auto => infer_line_terminator(current_readme),
        LineTerminatorOpt::Lf => LineTerminator::Lf,
        LineTerminatorOpt::CrLf => LineTerminator::CrLf,
    }
}

/// The README produced by injecting `doc` into `readme_text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedReadme {
    /// Serialized bytes.
    pub bytes: Vec<u8>,
    /// Whether a marker was found.
    pub had_marker: bool,
}

/// Injects `doc` into `readme_text` between cargo-rdme markers and serializes the result.
///
/// # Panics
///
/// Never: serialization writes into memory, which cannot fail.
pub fn render_readme(
    readme_text: &str,
    doc: &Doc,
    heading_base_level: Option<u8>,
    line_terminator: LineTerminator,
) -> Result<RenderedReadme, InjectDocError> {
    let new = inject_doc_in_readme(&Readme::from_str(readme_text), doc, heading_base_level)?;
    let mut bytes = Vec::with_capacity(readme_text.len() + doc.as_string().len());
    new.readme
        .write(&mut bytes, line_terminator)
        .expect("writing to a Vec cannot fail");

    Ok(RenderedReadme {
        bytes,
        had_marker: new.had_marker,
    })
}

/// Resolves the README path from options and project.
pub fn readme_path(
    options: &Options,
    project: &Project,
    current_dir: &Path,
) -> Result<PathBuf, Error> {
    match &options.readme_path {
        None => project.readme_path().ok_or(Error::NoReadmeFile(None)),
        Some(path) => {
            let path = current_dir.join(path);
            if path.is_file() {
                Ok(path)
            } else {
                Err(Error::NoReadmeFile(Some(path)))
            }
        }
    }
}

/// Runs the whole pipeline for already-merged `options`, without writing the README.
///
/// Intralink resolution may invoke Cargo/rustdoc and write build artifacts.
pub fn generate(options: &Options, current_dir: &Path) -> Result<Generated, Error> {
    let project = Project::discover(
        options.manifest_path.as_deref(),
        options.workspace_project.as_deref(),
        current_dir,
    )?;
    let Entrypoint {
        path: entry,
        target,
    } = project.entrypoint(&options.entrypoint)?;

    let extracted = extract::extract_doc_from_source_file(&entry)?
        .ok_or_else(|| Error::NoRustdoc(entry.clone()))?;

    let (doc, warnings) = transform_doc(
        &extracted.doc,
        TransformContext {
            package_name: &project.package_name,
            target,
            workspace_project: options.workspace_project.as_deref(),
            manifest_path: &project.manifest_path,
            intralinks: &options.intralinks,
        },
    )?;

    let readme_path = readme_path(options, &project, current_dir)?;
    let current = std::fs::read(&readme_path).map_err(|source| Error::ReadReadme {
        path: readme_path.clone(),
        source,
    })?;
    let current_text = std::str::from_utf8(&current).map_err(|_| Error::ReadReadme {
        path: readme_path.clone(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, "README is not valid UTF-8"),
    })?;

    let line_terminator = select_line_terminator(options.line_terminator, current_text);
    let rendered = render_readme(
        current_text,
        &doc,
        options.heading_base_level,
        line_terminator,
    )
    .map_err(|source| Error::InjectDoc {
        path: readme_path.clone(),
        source,
    })?;

    Ok(Generated {
        readme_path,
        current,
        rendered: rendered.bytes,
        line_terminator,
        had_marker: rendered.had_marker,
        warnings,
        diagrams: extracted.diagrams,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib_ctx(intralinks: &IntralinksConfig) -> TransformContext<'_> {
        TransformContext {
            package_name: "demo",
            target: PackageTarget::Lib,
            workspace_project: None,
            manifest_path: Path::new("Cargo.toml"),
            intralinks,
        }
    }

    fn render(readme: &str, doc: &str, lt: LineTerminator) -> RenderedReadme {
        let intralinks = IntralinksConfig {
            strip_links: Some(true),
            ..Default::default()
        };
        let (doc, _) = transform_doc(&Doc::from_str(doc), lib_ctx(&intralinks)).unwrap();
        render_readme(readme, &doc, None, lt).unwrap()
    }

    fn text(r: &RenderedReadme) -> &str {
        std::str::from_utf8(&r.bytes).unwrap()
    }

    #[test]
    fn hidden_lines_and_rust_tags() {
        let r = render(
            "<!-- cargo-rdme -->\n",
            "```\n# use x;\nlet a = 1;\n```\n",
            LineTerminator::Lf,
        );
        assert_eq!(
            text(&r),
            "<!-- cargo-rdme start -->\n\n```rust\nlet a = 1;\n```\n\n<!-- cargo-rdme end -->\n"
        );
    }

    #[test]
    fn mermaid_fence_is_untouched_by_rust_transforms() {
        let r = render(
            "<!-- cargo-rdme -->\n",
            "```mermaid\n# not hidden\n```\n",
            LineTerminator::Lf,
        );
        assert!(text(&r).contains("```mermaid\n# not hidden\n```"));
    }

    #[test]
    fn headings_follow_readme_section_but_not_inside_fences() {
        let r = render(
            "# Title\n\n<!-- cargo-rdme -->\n",
            "# Usage\n\n```mermaid\n# x\n```\n",
            LineTerminator::Lf,
        );
        assert!(text(&r).contains("## Usage\n"));
        assert!(text(&r).contains("```mermaid\n# x\n```"));
    }

    #[test]
    fn strips_intralinks() {
        let r = render(
            "<!-- cargo-rdme -->\n",
            "See [`Foo`](crate::Foo) and [`Bar`].\n",
            LineTerminator::Lf,
        );
        assert!(text(&r).contains("See `Foo` and `Bar`."), "{}", text(&r));
    }

    #[test]
    fn crlf_serialization() {
        let r = render(
            "a\r\n<!-- cargo-rdme -->\r\nb\r\n",
            "doc\n",
            LineTerminator::CrLf,
        );
        assert_eq!(
            text(&r),
            "a\r\n<!-- cargo-rdme start -->\r\n\r\ndoc\r\n\r\n<!-- cargo-rdme end -->\r\nb\r\n"
        );
    }

    #[test]
    fn line_terminator_inference() {
        assert_eq!(infer_line_terminator("a\r\nb\r\nc\n"), LineTerminator::CrLf);
        assert_eq!(infer_line_terminator("a\r\nb\n"), LineTerminator::Lf);
        assert_eq!(infer_line_terminator(""), LineTerminator::Lf);
        assert_eq!(
            select_line_terminator(LineTerminatorOpt::CrLf, "a\n"),
            LineTerminator::CrLf
        );
    }

    #[test]
    fn marker_errors() {
        let doc = Doc::from_str("x");
        assert!(matches!(
            render_readme(
                "<!-- cargo-rdme start -->\n",
                &doc,
                None,
                LineTerminator::Lf
            ),
            Err(InjectDocError::UnmatchedMarkerCargoRdmeStart)
        ));
        assert!(matches!(
            render_readme(
                "a\n<!-- cargo-rdme end -->\n",
                &doc,
                None,
                LineTerminator::Lf
            ),
            Err(InjectDocError::UnexpectedMarkerCargoRdmeEnd { line_number: 2 })
        ));
    }

    #[test]
    fn no_marker_leaves_readme_unchanged() {
        let r = render_readme("# Hi\n", &Doc::from_str("x"), None, LineTerminator::Lf).unwrap();
        assert!(!r.had_marker);
        assert_eq!(r.bytes, b"# Hi\n");
    }
}
