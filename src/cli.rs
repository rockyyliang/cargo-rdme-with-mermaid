//! Command-line parsing.

use crate::options::{CmdOptions, EntrypointOpt, LineTerminatorOpt};
use clap::{Args, Parser, Subcommand};
use std::ffi::OsString;
use std::path::PathBuf;

/// The cargo subcommand name. Cargo passes it as the first argument when invoked as
/// `cargo rdme-with-mermaid`.
pub const SUBCOMMAND: &str = "rdme-with-mermaid";

/// Create the README from your crate's documentation, expanding simple-mermaid diagrams.
#[derive(Debug, Parser)]
#[command(
    name = "cargo-rdme-with-mermaid",
    bin_name = "cargo rdme-with-mermaid",
    version
)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Cli {
    /// Auxiliary commands.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Generation options.
    #[command(flatten)]
    pub run: RunArgs,
}

/// Auxiliary commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Install the nightly rust toolchain cargo-rdme needs for intralink resolution.
    #[command(hide = true)]
    InstallRustToolchainForIntralinks,
}

/// Options for generating or checking the README. Same flags as `cargo rdme`.
#[derive(Debug, Args, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct RunArgs {
    /// Check that the README is up to date without writing it (exit code 3 on mismatch, 4 if
    /// warnings were emitted).
    #[arg(short, long)]
    pub check: bool,

    /// Path to `Cargo.toml`.
    #[arg(long, value_name = "PATH")]
    pub manifest_path: Option<PathBuf>,

    /// Workspace package to get the documentation from.
    #[arg(short, long, value_name = "NAME")]
    pub workspace_project: Option<String>,

    /// Source entrypoint: auto, lib, bin, or bin:NAME.
    #[arg(long, value_name = "ENTRYPOINT")]
    pub entrypoint: Option<EntrypointOpt>,

    /// README path (overrides the `readme` field in `Cargo.toml`).
    #[arg(short, long, value_name = "PATH")]
    pub readme_path: Option<PathBuf>,

    /// Line terminator for the README: auto, lf, or crlf.
    #[arg(long, value_name = "TERMINATOR")]
    pub line_terminator: Option<LineTerminatorOpt>,

    /// Heading level added to the headings of the crate documentation.
    #[arg(long, value_name = "LEVEL")]
    pub heading_base_level: Option<u8>,

    /// Remove intralinks instead of resolving them (does not need rustdoc).
    #[arg(long)]
    pub intralinks_strip_links: bool,

    /// Enable all features when running rustdoc to resolve intralinks.
    #[arg(long)]
    pub intralinks_all_features: bool,

    /// Features to enable when running rustdoc to resolve intralinks (comma separated).
    #[arg(long, value_delimiter = ',', value_name = "FEATURES")]
    pub intralinks_features: Option<Vec<String>>,

    /// Disable default features when running rustdoc to resolve intralinks.
    #[arg(long)]
    pub intralinks_no_default_features: bool,

    /// In check mode, do not fail because of warnings.
    #[arg(long)]
    pub no_fail_on_warnings: bool,

    /// Update the README even if it has uncommitted changes.
    #[arg(short, long)]
    pub force: bool,
}

impl From<RunArgs> for CmdOptions {
    fn from(args: RunArgs) -> CmdOptions {
        CmdOptions {
            workspace_project: args.workspace_project,
            entrypoint: args.entrypoint,
            line_terminator: args.line_terminator,
            check: args.check,
            no_fail_on_warnings: args.no_fail_on_warnings,
            intralinks_strip_links: args.intralinks_strip_links,
            force: args.force,
            readme_path: args.readme_path,
            manifest_path: args.manifest_path,
            intralinks_all_features: args.intralinks_all_features,
            intralinks_features: args.intralinks_features,
            intralinks_no_default_features: args.intralinks_no_default_features,
            heading_base_level: args.heading_base_level,
        }
    }
}

/// Removes the subcommand name cargo inserts, so both `cargo rdme-with-mermaid --check` and
/// `cargo-rdme-with-mermaid --check` work.
#[must_use]
pub fn normalize_args(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut args: Vec<OsString> = args.into_iter().collect();
    if args.get(1).is_some_and(|a| a == SUBCOMMAND) {
        args.remove(1);
    }
    args
}

/// Parses process-style arguments (including the program name).
pub fn parse_from(args: impl IntoIterator<Item = OsString>) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(normalize_args(args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        parse_from(args.iter().map(OsString::from))
    }

    #[test]
    fn clap_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn cargo_and_direct_invocation() {
        assert!(
            parse(&["cargo-rdme-with-mermaid", "rdme-with-mermaid", "--check"])
                .unwrap()
                .run
                .check
        );
        assert!(
            parse(&["cargo-rdme-with-mermaid", "--check"])
                .unwrap()
                .run
                .check
        );
        assert!(!parse(&["cargo-rdme-with-mermaid"]).unwrap().run.check);
    }

    #[test]
    fn parses_flags() {
        let cli = parse(&[
            "x",
            "rdme-with-mermaid",
            "--manifest-path",
            "a/Cargo.toml",
            "--workspace-project",
            "pkg",
            "--intralinks-strip-links",
            "--entrypoint",
            "bin:tool",
            "--line-terminator",
            "crlf",
            "--heading-base-level",
            "2",
            "--intralinks-features",
            "a,b",
            "-f",
        ])
        .unwrap();
        let options = CmdOptions::from(cli.run);
        assert_eq!(options.manifest_path, Some(PathBuf::from("a/Cargo.toml")));
        assert_eq!(options.workspace_project.as_deref(), Some("pkg"));
        assert!(options.intralinks_strip_links);
        assert_eq!(
            options.entrypoint,
            Some(EntrypointOpt::BinName("tool".into()))
        );
        assert_eq!(options.line_terminator, Some(LineTerminatorOpt::CrLf));
        assert_eq!(options.heading_base_level, Some(2));
        assert_eq!(
            options.intralinks_features,
            Some(vec!["a".into(), "b".into()])
        );
        assert!(options.force);
    }

    #[test]
    fn rejects_bad_values() {
        assert!(parse(&["x", "--entrypoint", "main"]).is_err());
        assert!(parse(&["x", "--line-terminator", "cr"]).is_err());
        assert!(parse(&["x", "--unknown"]).is_err());
    }

    #[test]
    fn subcommand() {
        let cli = parse(&[
            "x",
            "rdme-with-mermaid",
            "install-rust-toolchain-for-intralinks",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::InstallRustToolchainForIntralinks)
        ));
    }
}
