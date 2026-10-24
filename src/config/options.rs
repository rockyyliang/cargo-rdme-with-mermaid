//! Effective options, after merging command-line flags with `.cargo-rdme.toml`.

use super::ConfigFileOptions;
use cargo_rdme::transform::IntralinksConfig;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// Error for an option value that cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid value \"{0}\"")]
pub struct InvalidOptValue(pub String);

/// Line terminator selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineTerminatorOpt {
    /// Use whatever the existing README predominantly uses.
    #[default]
    Auto,
    /// `\n`.
    Lf,
    /// `\r\n`.
    CrLf,
}

impl FromStr for LineTerminatorOpt {
    type Err = InvalidOptValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(LineTerminatorOpt::Auto),
            "lf" => Ok(LineTerminatorOpt::Lf),
            "crlf" => Ok(LineTerminatorOpt::CrLf),
            v => Err(InvalidOptValue(v.to_owned())),
        }
    }
}

/// Source entrypoint selection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EntrypointOpt {
    /// The library target if there is one, otherwise the only binary target.
    #[default]
    Auto,
    /// The library target.
    Lib,
    /// The only binary target.
    BinDefault,
    /// The binary target with the given name.
    BinName(String),
}

impl FromStr for EntrypointOpt {
    type Err = InvalidOptValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(EntrypointOpt::Auto),
            "lib" => Ok(EntrypointOpt::Lib),
            "bin" => Ok(EntrypointOpt::BinDefault),
            v => match v.strip_prefix("bin:") {
                Some(name) if !name.is_empty() => Ok(EntrypointOpt::BinName(name.to_owned())),
                _ => Err(InvalidOptValue(v.to_owned())),
            },
        }
    }
}

impl fmt::Display for EntrypointOpt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EntrypointOpt::Auto => f.write_str("auto"),
            EntrypointOpt::Lib => f.write_str("lib"),
            EntrypointOpt::BinDefault => f.write_str("bin"),
            EntrypointOpt::BinName(name) => write!(f, "bin:{name}"),
        }
    }
}

/// Options given on the command line. `None`/`false` means "not given".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct CmdOptions {
    /// `--workspace-project`.
    pub workspace_project: Option<String>,
    /// `--entrypoint`.
    pub entrypoint: Option<EntrypointOpt>,
    /// `--line-terminator`.
    pub line_terminator: Option<LineTerminatorOpt>,
    /// `--check`.
    pub check: bool,
    /// `--no-fail-on-warnings`.
    pub no_fail_on_warnings: bool,
    /// `--intralinks-strip-links`.
    pub intralinks_strip_links: bool,
    /// `--force`.
    pub force: bool,
    /// `--readme-path`.
    pub readme_path: Option<PathBuf>,
    /// `--manifest-path`.
    pub manifest_path: Option<PathBuf>,
    /// `--intralinks-all-features`.
    pub intralinks_all_features: bool,
    /// `--intralinks-features`.
    pub intralinks_features: Option<Vec<String>>,
    /// `--intralinks-no-default-features`.
    pub intralinks_no_default_features: bool,
    /// `--heading-base-level`.
    pub heading_base_level: Option<u8>,
}

/// Fully resolved options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Options {
    /// Workspace package to document, if not the current package.
    pub workspace_project: Option<String>,
    /// Source entrypoint.
    pub entrypoint: EntrypointOpt,
    /// Line terminator for the README.
    pub line_terminator: LineTerminatorOpt,
    /// Only check freshness; never write.
    pub check: bool,
    /// In check mode, do not fail because of warnings.
    pub no_fail_on_warnings: bool,
    /// Write even if the README has uncommitted changes.
    pub force: bool,
    /// README path override.
    pub readme_path: Option<PathBuf>,
    /// Path to `Cargo.toml`.
    pub manifest_path: Option<PathBuf>,
    /// Intralink configuration passed to cargo-rdme.
    pub intralinks: IntralinksConfig,
    /// Heading base level override.
    pub heading_base_level: Option<u8>,
}

/// Merges command-line options with configuration-file options. Command-line options win.
#[must_use]
pub fn merge_options(cmd: CmdOptions, config: Option<ConfigFileOptions>) -> Options {
    let config = config.unwrap_or_default();
    let intralinks = config.intralinks.unwrap_or_default();

    Options {
        workspace_project: cmd.workspace_project.or(config.workspace_project),
        entrypoint: cmd.entrypoint.or(config.entrypoint).unwrap_or_default(),
        line_terminator: cmd
            .line_terminator
            .or(config.line_terminator)
            .unwrap_or_default(),
        check: cmd.check,
        no_fail_on_warnings: cmd.no_fail_on_warnings,
        force: cmd.force,
        readme_path: cmd.readme_path.or(config.readme_path),
        manifest_path: cmd.manifest_path,
        intralinks: IntralinksConfig {
            docs: intralinks.docs,
            strip_links: cmd
                .intralinks_strip_links
                .then_some(true)
                .or(intralinks.strip_links),
            all_features: cmd
                .intralinks_all_features
                .then_some(true)
                .or(intralinks.all_features),
            features: cmd.intralinks_features.or(intralinks.features),
            no_default_features: cmd
                .intralinks_no_default_features
                .then_some(true)
                .or(intralinks.no_default_features),
            rustdoc_toolchain: intralinks.rustdoc_toolchain,
        },
        heading_base_level: cmd.heading_base_level.or(config.heading_base_level),
    }
}

/// Applies environment-variable overrides, as cargo-rdme does (`CARGO_RDME_RUSTDOC_TOOLCHAIN`).
pub fn apply_env_overrides(options: &mut Options, var: impl Fn(&str) -> Option<String>) {
    if let Some(toolchain) = var("CARGO_RDME_RUSTDOC_TOOLCHAIN") {
        options.intralinks.rustdoc_toolchain = Some(toolchain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entrypoints() {
        assert_eq!("auto".parse(), Ok(EntrypointOpt::Auto));
        assert_eq!("lib".parse(), Ok(EntrypointOpt::Lib));
        assert_eq!("bin".parse(), Ok(EntrypointOpt::BinDefault));
        assert_eq!(
            "bin:tool".parse(),
            Ok(EntrypointOpt::BinName("tool".into()))
        );
        assert!("bin:".parse::<EntrypointOpt>().is_err());
        assert!("main".parse::<EntrypointOpt>().is_err());
    }

    #[test]
    fn parses_line_terminators() {
        assert_eq!("crlf".parse(), Ok(LineTerminatorOpt::CrLf));
        assert!("cr".parse::<LineTerminatorOpt>().is_err());
    }

    #[test]
    fn command_line_wins_over_config() {
        let config = ConfigFileOptions {
            workspace_project: Some("from-config".into()),
            heading_base_level: Some(2),
            line_terminator: Some(LineTerminatorOpt::CrLf),
            intralinks: Some(IntralinksConfig {
                strip_links: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cmd = CmdOptions {
            workspace_project: Some("from-cli".into()),
            intralinks_strip_links: true,
            ..Default::default()
        };
        let options = merge_options(cmd, Some(config));
        assert_eq!(options.workspace_project.as_deref(), Some("from-cli"));
        assert_eq!(options.heading_base_level, Some(2));
        assert_eq!(options.line_terminator, LineTerminatorOpt::CrLf);
        assert_eq!(options.intralinks.strip_links, Some(true));
    }

    #[test]
    fn env_override_sets_toolchain() {
        let mut options = Options::default();
        apply_env_overrides(&mut options, |k| {
            (k == "CARGO_RDME_RUSTDOC_TOOLCHAIN").then(|| "default".into())
        });
        assert_eq!(
            options.intralinks.rustdoc_toolchain.as_deref(),
            Some("default")
        );
    }
}
