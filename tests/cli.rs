//! End-to-end tests running the executable against independent fixture projects.
//!
//! Fixture manifests are stored as `_Cargo.toml` so that they are not treated as nested packages
//! by `cargo package`; they are renamed when copied into a temporary directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_cargo-rdme-with-mermaid");

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let target = if name == "_Cargo.toml" {
            to.join("Cargo.toml")
        } else {
            to.join(&name)
        };
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
            dir.path(),
        );
        Fixture { dir }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap()
    }

    fn write(&self, rel: &str, content: &str) {
        fs::write(self.path(rel), content).unwrap();
    }

    /// Runs the binary the way cargo does (with the subcommand name as first argument).
    fn run_in(&self, rel_dir: &str, args: &[&str]) -> Output {
        Command::new(BIN)
            .arg("rdme-with-mermaid")
            .args(args)
            .current_dir(self.path(rel_dir))
            .env_remove("CARGO_RDME_RUSTDOC_TOOLCHAIN")
            .output()
            .unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_in("", args)
    }

    /// Snapshot of every file, to verify that nothing was written.
    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            let mut entries: Vec<_> = fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push((path.clone(), fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(self.dir.path(), &mut out);
        out
    }
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("terminated by signal")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[track_caller]
fn assert_code(output: &Output, expected: i32) {
    assert_eq!(code(output), expected, "stderr:\n{}", stderr(output));
}

fn expected(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/expected")
            .join(name),
    )
    .unwrap()
}

#[test]
fn crate_without_mermaid_matches_expected() {
    let f = Fixture::new("plain");
    assert_code(&f.run(&["--intralinks-strip-links"]), 0);
    assert_eq!(f.read("README.md"), expected("plain.md"));
}

#[test]
fn initial_marker_is_expanded_and_outside_content_preserved() {
    let f = Fixture::new("plain");
    assert_code(&f.run(&["--intralinks-strip-links"]), 0);
    let readme = f.read("README.md");
    assert!(readme.starts_with("# plain\n\n[![badge](https://example.org/badge.svg)](https://example.org)\n\n<!-- cargo-rdme start -->\n"));
    assert!(readme.ends_with(
        "<!-- cargo-rdme end -->\n\n## License\n\nHand-written footer that must be preserved.\n"
    ));
    assert!(!readme.contains("<!-- cargo-rdme -->"));
}

#[test]
fn hidden_lines_headings_and_stripped_intralinks() {
    let f = Fixture::new("plain");
    assert_code(&f.run(&["--intralinks-strip-links"]), 0);
    let readme = f.read("README.md");
    assert!(!readme.contains("hidden_setup"), "hidden line kept");
    assert!(
        readme.contains("```rust\nlet answer"),
        "code block not tagged as rust"
    );
    assert!(readme.contains("\n# Not hidden"), "`##` line not unescaped");
    assert!(
        readme.contains("\n## Example\n") && readme.contains("\n### Details\n"),
        "headings not nested"
    );
    assert!(
        readme.contains("See `answer` and the module for more."),
        "intralinks not stripped"
    );

    let f = Fixture::new("plain");
    assert_code(
        &f.run(&["--intralinks-strip-links", "--heading-base-level", "0"]),
        0,
    );
    assert!(f.read("README.md").contains("\n# Example\n"));
}

#[test]
fn inline_diagrams_match_expected() {
    let f = Fixture::new("inline");
    assert_code(&f.run(&[]), 0);
    assert_eq!(f.read("README.md"), expected("inline.md"));
}

#[test]
fn diagram_positions_repeats_and_example_text() {
    let f = Fixture::new("inline");
    assert_code(&f.run(&[]), 0);
    let readme = f.read("README.md");

    let arch = "```mermaid\nflowchart TD\n    A[Client] --> B[Server]\n```";
    let at = |needle: &str| {
        readme
            .find(needle)
            .unwrap_or_else(|| panic!("missing {needle:?}"))
    };
    assert_eq!(readme.matches(arch).count(), 2, "repeated reference");
    assert!(at("## Architecture") < at(arch) && at(arch) < at("## Flow"));
    assert!(at("## Flow") < at("````mermaid") && at("````mermaid") < at("## Usage"));
    assert!(
        at("## Again") < readme.rfind(arch).unwrap()
            && readme.rfind(arch).unwrap() < at("The end.")
    );
    // Source-relative path inside src/, CRLF normalized, fence lengthened for backticks.
    assert!(readme.contains(
        "````mermaid\nsequenceDiagram\n    Alice->>Bob: ```code``` in a message\n````\n"
    ));
    // Macro text in a doc example is not expanded (its path does not even exist).
    assert!(
        readme.contains("```rust\n#![doc = simple_mermaid::mermaid!(\"does/not/exist.mmd\")]\n```")
    );
    assert!(!readme.contains("Stale generated content."));
    assert!(
        readme.starts_with("# inline\n\nIntro written by hand.\n\n<!-- cargo-rdme start -->\n")
    );
    assert!(readme.ends_with("<!-- cargo-rdme end -->\n\nFooter written by hand.\n"));
}

#[test]
fn regeneration_is_idempotent_and_check_passes() {
    let f = Fixture::new("inline");
    assert_code(&f.run(&[]), 0);
    let first = f.snapshot();
    let output = f.run(&[]);
    assert_code(&output, 0);
    assert!(
        !stderr(&output).contains("updated"),
        "unchanged README was rewritten"
    );
    assert_eq!(f.snapshot(), first);
    assert_code(&f.run(&["--check"]), 0);
    assert_eq!(f.snapshot(), first);
}

#[test]
fn check_on_initial_marker_fails_without_writing() {
    let f = Fixture::new("inline");
    let before = f.snapshot();
    assert_code(&f.run(&["--check"]), 3);
    assert_eq!(f.snapshot(), before);
}

#[test]
fn stale_documentation_fails_check_without_writes() {
    let f = Fixture::new("inline");
    assert_code(&f.run(&[]), 0);
    let lib = f.read("src/lib.rs").replace("The end.", "The new end.");
    f.write("src/lib.rs", &lib);
    let before = f.snapshot();

    let output = f.run(&["--check"]);
    assert_code(&output, 3);
    assert!(stderr(&output).contains("is not up to date"));
    assert_eq!(f.snapshot(), before);
}

#[test]
fn stale_diagram_fails_check_without_writes() {
    let f = Fixture::new("inline");
    assert_code(&f.run(&[]), 0);
    f.write(
        "docs/architecture.mmd",
        "flowchart TD\n    A[Client] --> C[Cache]\n",
    );
    let before = f.snapshot();

    assert_code(&f.run(&["--check"]), 3);
    assert_eq!(f.snapshot(), before);

    assert_code(&f.run(&[]), 0);
    assert_eq!(
        f.read("README.md")
            .matches("A[Client] --> C[Cache]")
            .count(),
        2
    );
    assert_code(&f.run(&["--check"]), 0);
}

#[test]
fn missing_diagram_is_reported_and_nothing_written() {
    let f = Fixture::new("inline");
    fs::remove_file(f.path("src/diagrams/flow.mmd")).unwrap();
    let before = f.snapshot();

    for args in [&[][..], &["--check"][..]] {
        let output = f.run(args);
        assert_code(&output, 1);
        let err = stderr(&output);
        assert!(err.contains("lib.rs:11:"), "{err}");
        assert!(
            err.contains("cannot read mermaid diagram \"diagrams/flow.mmd\""),
            "{err}"
        );
        let resolved = format!("src{}diagrams/flow.mmd", std::path::MAIN_SEPARATOR);
        assert!(err.contains(&resolved), "{err}");
        assert_eq!(f.snapshot(), before);
    }
}

#[test]
fn unsupported_syntax_is_reported() {
    for (attr, needle) in [
        (
            "#![doc = simple_mermaid::mermaid!(concat!(\"a\", \".mmd\"))]",
            "must be a string literal",
        ),
        (
            "#![doc = simple_mermaid::mermaid!(\"a.mmd\" sideways)]",
            "unknown option `sideways`",
        ),
        (
            "#![cfg_attr(doc, doc = simple_mermaid::mermaid!(\"a.mmd\"))]",
            "cfg_attr",
        ),
    ] {
        let f = Fixture::new("plain");
        f.write("src/lib.rs", &format!("//! Docs.\n{attr}\n"));
        let before = f.snapshot();
        let output = f.run(&["--intralinks-strip-links"]);
        assert_code(&output, 1);
        let err = stderr(&output);
        assert!(
            err.contains("unsupported mermaid! invocation") && err.contains(needle),
            "{err}"
        );
        assert!(err.contains("lib.rs:2:"), "{err}");
        assert_eq!(f.snapshot(), before);
    }
}

#[test]
fn malformed_markers_are_reported_before_writing() {
    let f = Fixture::new("plain");
    f.write("README.md", "# x\n\n<!-- cargo-rdme start -->\n\nold\n");
    let before = f.snapshot();
    let output = f.run(&["--intralinks-strip-links"]);
    assert_code(&output, 1);
    assert!(stderr(&output).contains("unmatched start marker"));
    assert_eq!(f.snapshot(), before);

    f.write("README.md", "# x\n\n<!-- cargo-rdme end -->\n");
    let output = f.run(&["--intralinks-strip-links"]);
    assert_code(&output, 1);
    assert!(stderr(&output).contains("unexpected end marker at line 3"));
}

#[test]
fn readme_without_marker_is_left_alone() {
    let f = Fixture::new("plain");
    f.write("README.md", "# no marker\n");
    let output = f.run(&["--intralinks-strip-links"]);
    assert_code(&output, 0);
    assert!(stderr(&output).contains("no marker found"));
    assert_eq!(f.read("README.md"), "# no marker\n");
}

#[test]
fn crlf_readme_keeps_crlf() {
    let f = Fixture::new("crlf");
    assert_code(&f.run(&[]), 0);
    let readme = f.read("README.md");
    assert_eq!(
        readme,
        "# crlf\r\n\r\n<!-- cargo-rdme start -->\r\n\r\nWindows-style crate.\r\n\r\n```mermaid\r\ngraph TD\r\n    x --> y\r\n```\r\n\r\n<!-- cargo-rdme end -->\r\n\r\nFooter\r\n"
    );
    assert_code(&f.run(&["--check"]), 0);
    let before = f.snapshot();
    // Requesting other line endings makes the same content stale.
    assert_code(&f.run(&["--check", "--line-terminator", "lf"]), 3);
    assert_eq!(f.snapshot(), before);
    assert_code(&f.run(&["--line-terminator", "lf"]), 0);
    assert!(!f.read("README.md").contains('\r'));
    assert_code(&f.run(&["--check"]), 0);
}

#[test]
fn lf_can_be_converted_to_crlf_via_config() {
    let f = Fixture::new("plain");
    f.write(
        ".cargo-rdme.toml",
        "line-terminator = \"crlf\"\n[intralinks]\nstrip-links = true\n",
    );
    assert_code(&f.run(&[]), 0);
    let readme = f.read("README.md");
    assert_eq!(readme.matches("\r\n").count(), readme.matches('\n').count());
    assert_code(&f.run(&["--check"]), 0);
}

#[test]
fn workspace_project_selection_and_config() {
    let f = Fixture::new("workspace");
    let output = f.run(&[]);
    assert_code(&output, 1);
    assert!(stderr(&output).contains("--workspace-project"));

    // .cargo-rdme.toml at the workspace root sets heading-base-level and strip-links.
    assert_code(&f.run(&["--workspace-project", "alpha"]), 0);
    assert_eq!(
        f.read("crates/alpha/ALPHA.md"),
        expected("workspace-alpha.md")
    );

    // Binary-only package with a custom bin path.
    assert_code(&f.run(&["-w", "beta"]), 0);
    assert!(
        f.read("crates/beta/README.md")
            .contains("The beta command-line tool.")
    );
    assert_code(
        &f.run(&["-w", "beta", "--entrypoint", "bin:beta-cli", "--check"]),
        0,
    );
    assert_code(&f.run(&["-w", "beta", "--entrypoint", "lib", "--check"]), 1);

    let output = f.run(&["-w", "gamma"]);
    assert_code(&output, 1);
    assert!(stderr(&output).contains("no package \"gamma\""));
}

#[test]
fn discovery_from_member_dir_and_manifest_path() {
    let f = Fixture::new("workspace");
    assert_code(&f.run_in("crates/alpha/lib", &[]), 0);
    assert_eq!(
        f.read("crates/alpha/ALPHA.md"),
        expected("workspace-alpha.md")
    );

    let other = tempfile::tempdir().unwrap();
    let manifest = f.path("crates/alpha/Cargo.toml");
    let output = Command::new(BIN)
        .args(["rdme-with-mermaid", "--check", "--manifest-path"])
        .arg(&manifest)
        .current_dir(other.path())
        .output()
        .unwrap();
    assert_code(&output, 0);
}

#[test]
fn readme_path_override() {
    let f = Fixture::new("plain");
    f.write("OTHER.md", "<!-- cargo-rdme -->\n");
    let before = f.read("README.md");
    assert_code(
        &f.run(&["--intralinks-strip-links", "--readme-path", "OTHER.md"]),
        0,
    );
    assert!(f.read("OTHER.md").contains("A crate without any diagram."));
    assert_eq!(f.read("README.md"), before);
    assert_code(&f.run(&["--readme-path", "MISSING.md"]), 1);
}

#[test]
fn direct_invocation_without_subcommand_name() {
    let f = Fixture::new("plain");
    let output = Command::new(BIN)
        .args(["--intralinks-strip-links"])
        .current_dir(f.path(""))
        .output()
        .unwrap();
    assert_code(&output, 0);
    assert_eq!(f.read("README.md"), expected("plain.md"));
}

#[test]
fn removed_installer_is_rejected_without_touching_project() {
    let f = Fixture::new("plain");
    let before = f.snapshot();
    let output = f.run(&["install-rust-toolchain-for-intralinks"]);
    assert_code(&output, 2);
    assert!(stderr(&output).contains("unexpected argument"));
    assert_eq!(f.snapshot(), before);
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.org",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}

#[test]
fn uncommitted_readme_is_protected_unless_forced() {
    let f = Fixture::new("plain");
    let dir = f.path("");
    git(&dir, &["init", "-q"]);

    // Untracked README: refuse.
    let output = f.run(&["--intralinks-strip-links"]);
    assert_code(&output, 2);
    assert!(stderr(&output).contains("--force"));

    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "init"]);

    // Tracked and clean: write.
    assert_code(&f.run(&["--intralinks-strip-links"]), 0);

    // Tracked with uncommitted changes (from the previous write), but nothing new to write: OK.
    assert_code(&f.run(&["--intralinks-strip-links"]), 0);

    // Uncommitted changes and new content: refuse, even from a subdirectory.
    f.write(
        "src/lib.rs",
        &f.read("src/lib.rs").replace("without any", "with no"),
    );
    let before = f.snapshot();
    assert_code(&f.run_in("src", &["--intralinks-strip-links"]), 2);
    assert_eq!(f.snapshot(), before);

    assert_code(&f.run(&["--intralinks-strip-links", "--force"]), 0);
    assert!(f.read("README.md").contains("A crate with no diagram."));
}

#[test]
fn runs_as_cargo_subcommand() {
    let f = Fixture::new("inline");
    let bin_dir = Path::new(BIN).parent().unwrap();
    let path = std::env::join_paths(std::iter::once(bin_dir.to_owned()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let run = |args: &[&str]| {
        Command::new(&cargo)
            .arg("rdme-with-mermaid")
            .args(args)
            .env("PATH", &path)
            .current_dir(f.path(""))
            .output()
            .unwrap()
    };

    assert_code(&run(&["--check"]), 3);
    assert_code(&run(&[]), 0);
    assert_code(&run(&["--check"]), 0);
    assert_eq!(f.read("README.md"), expected("inline.md"));
}
