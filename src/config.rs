//! Reading `.cargo-rdme.toml`, with the same keys and validation as cargo-rdme.

use crate::options::{EntrypointOpt, LineTerminatorOpt};
use cargo_rdme::transform::{IntralinksConfig, IntralinksDocsConfig};
use std::path::{Path, PathBuf};
use toml::Value;

/// Name of the configuration file shared with cargo-rdme.
pub const CONFIG_FILE_NAME: &str = ".cargo-rdme.toml";

/// Errors reading or validating the configuration file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file exists but cannot be read.
    #[error("failed to read configuration file \"{}\": {source}", path.display())]
    Read {
        /// Configuration file path.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The file is not valid TOML.
    #[error("failed to parse \"{}\": {source}", path.display())]
    Parse {
        /// Configuration file path.
        path: PathBuf,
        /// Underlying error.
        source: Box<toml::de::Error>,
    },
    /// A field has an invalid value.
    #[error("invalid field \"{0}\"")]
    InvalidField(&'static str),
    /// The `[entrypoint]` table is inconsistent.
    #[error("invalid entrypoint table")]
    InvalidEntrypointTable,
    /// Both a deprecated and a current option were given.
    #[error("`{new}` and `{old}` cannot be set at the same time")]
    ConflictingDocsOptions {
        /// Current option.
        new: &'static str,
        /// Deprecated option.
        old: &'static str,
    },
    /// `layout = "flat"` without `base-url`.
    #[error("`intralinks.docs.base-url` is required when `intralinks.docs.layout = \"flat\"`")]
    FlatLayoutRequiresBaseUrl,
    /// `layout = "flat"` with `version`.
    #[error("`intralinks.docs.version` cannot be set when `intralinks.docs.layout = \"flat\"`")]
    FlatLayoutRejectsVersion,
}

/// Options read from the configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigFileOptions {
    /// `line-terminator`.
    pub line_terminator: Option<LineTerminatorOpt>,
    /// `workspace-project`.
    pub workspace_project: Option<String>,
    /// `[entrypoint]`.
    pub entrypoint: Option<EntrypointOpt>,
    /// `readme-path`, resolved relative to the configuration file's directory.
    pub readme_path: Option<PathBuf>,
    /// `[intralinks]`.
    pub intralinks: Option<IntralinksConfig>,
    /// `heading-base-level`.
    pub heading_base_level: Option<u8>,
}

/// Finds the nearest `.cargo-rdme.toml` in `dir` or its ancestors.
#[must_use]
pub fn find_config_file(dir: &Path) -> Option<PathBuf> {
    cargo_rdme::find_first_file_in_ancestors(dir, CONFIG_FILE_NAME)
}

/// Finds and parses the nearest configuration file, if any.
pub fn load_config(
    dir: &Path,
    warn: impl FnMut(&str),
) -> Result<Option<ConfigFileOptions>, ConfigError> {
    let Some(path) = find_config_file(dir) else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    let mut options = parse_config(&text, warn).map_err(|e| match e {
        ConfigError::Parse { source, .. } => ConfigError::Parse {
            path: path.clone(),
            source,
        },
        e => e,
    })?;

    if let (Some(readme), Some(base)) = (&options.readme_path, path.parent()) {
        options.readme_path = Some(base.join(readme));
    }

    Ok(Some(options))
}

fn str_field<'a>(table: Option<&'a toml::Table>, key: &str) -> Option<&'a str> {
    table.and_then(|t| t.get(key)).and_then(Value::as_str)
}

fn bool_field(table: Option<&toml::Table>, key: &str) -> Option<bool> {
    table.and_then(|t| t.get(key)).and_then(Value::as_bool)
}

/// Parses the `[intralinks]` table.
fn parse_intralinks(
    table: &toml::Table,
    warn: &mut impl FnMut(&str),
) -> Result<IntralinksConfig, ConfigError> {
    let table = Some(table);
    let docs_table = table.and_then(|t| t.get("docs")).and_then(Value::as_table);

    let old_base_url = str_field(table, "docs-rs-base-url");
    let old_version = str_field(table, "docs-rs-version");
    let new_base_url = str_field(docs_table, "base-url");
    let new_version = str_field(docs_table, "version");

    if new_base_url.is_some() && old_base_url.is_some() {
        return Err(ConfigError::ConflictingDocsOptions {
            new: "intralinks.docs.base-url",
            old: "intralinks.docs-rs-base-url",
        });
    }
    if new_version.is_some() && old_version.is_some() {
        return Err(ConfigError::ConflictingDocsOptions {
            new: "intralinks.docs.version",
            old: "intralinks.docs-rs-version",
        });
    }

    let base_url = new_base_url.or(old_base_url);
    let version = new_version.or(old_version);

    let docs = match str_field(docs_table, "layout") {
        Some("flat") => {
            if version.is_some() {
                return Err(ConfigError::FlatLayoutRejectsVersion);
            }
            IntralinksDocsConfig::Flat {
                base_url: base_url
                    .ok_or(ConfigError::FlatLayoutRequiresBaseUrl)?
                    .to_owned(),
            }
        }
        Some("docs-rs") | None => IntralinksDocsConfig::DocsRs {
            base_url: base_url.map(ToOwned::to_owned),
            version: version.map(ToOwned::to_owned),
        },
        Some(_) => return Err(ConfigError::InvalidField("intralinks.docs.layout")),
    };

    if old_base_url.is_some() {
        warn(
            "`[intralinks].docs-rs-base-url` is deprecated: use `[intralinks.docs].base-url` instead.",
        );
    }
    if old_version.is_some() {
        warn(
            "`[intralinks].docs-rs-version` is deprecated: use `[intralinks.docs].version` instead.",
        );
    }

    Ok(IntralinksConfig {
        docs,
        strip_links: bool_field(table, "strip-links"),
        all_features: bool_field(table, "all-features"),
        features: table
            .and_then(|t| t.get("features"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            }),
        no_default_features: bool_field(table, "no-default-features"),
        rustdoc_toolchain: str_field(table, "rustdoc-toolchain").map(ToOwned::to_owned),
    })
}

/// Parses configuration text. `readme-path` is returned as written.
pub fn parse_config(
    text: &str,
    mut warn: impl FnMut(&str),
) -> Result<ConfigFileOptions, ConfigError> {
    let root: toml::Table = toml::from_str(text).map_err(|source| ConfigError::Parse {
        path: PathBuf::new(),
        source: Box::new(source),
    })?;
    let root = Some(&root);

    let line_terminator = root
        .and_then(|t| t.get("line-terminator"))
        .map(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .ok_or(ConfigError::InvalidField("line-terminator"))
        })
        .transpose()?;

    let entrypoint_table = root
        .and_then(|t| t.get("entrypoint"))
        .and_then(Value::as_table);
    let entrypoint = match (
        str_field(entrypoint_table, "type"),
        str_field(entrypoint_table, "bin-name"),
    ) {
        (Some("lib"), None) => Some(EntrypointOpt::Lib),
        (Some("bin"), None) => Some(EntrypointOpt::BinDefault),
        (Some("bin"), Some(name)) => Some(EntrypointOpt::BinName(name.to_owned())),
        (None, None) => None,
        _ => return Err(ConfigError::InvalidEntrypointTable),
    };

    let heading_base_level = match root
        .and_then(|t| t.get("heading-base-level"))
        .and_then(Value::as_integer)
    {
        None => None,
        Some(level) => {
            Some(u8::try_from(level).map_err(|_| ConfigError::InvalidField("heading-base-level"))?)
        }
    };

    let intralinks_table = root
        .and_then(|t| t.get("intralinks"))
        .and_then(Value::as_table);
    let intralinks = intralinks_table
        .map(|table| parse_intralinks(table, &mut warn))
        .transpose()?;

    Ok(ConfigFileOptions {
        line_terminator,
        workspace_project: str_field(root, "workspace-project").map(ToOwned::to_owned),
        entrypoint,
        readme_path: str_field(root, "readme-path").map(PathBuf::from),
        intralinks,
        heading_base_level,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<ConfigFileOptions, ConfigError> {
        parse_config(text, |_| {})
    }

    #[test]
    fn empty_config() {
        assert_eq!(parse("").unwrap(), ConfigFileOptions::default());
    }

    #[test]
    fn full_config() {
        let options = parse(
            r#"
            readme-path = "MY-README.md"
            line-terminator = "crlf"
            workspace-project = "sub"
            heading-base-level = 1

            [entrypoint]
            type = "bin"
            bin-name = "tool"

            [intralinks]
            strip-links = true
            features = ["a", "b"]

            [intralinks.docs]
            layout = "flat"
            base-url = "https://example.org/doc"
            "#,
        )
        .unwrap();

        assert_eq!(options.readme_path, Some(PathBuf::from("MY-README.md")));
        assert_eq!(options.line_terminator, Some(LineTerminatorOpt::CrLf));
        assert_eq!(options.workspace_project.as_deref(), Some("sub"));
        assert_eq!(options.heading_base_level, Some(1));
        assert_eq!(
            options.entrypoint,
            Some(EntrypointOpt::BinName("tool".into()))
        );
        let intralinks = options.intralinks.unwrap();
        assert_eq!(intralinks.strip_links, Some(true));
        assert_eq!(intralinks.features, Some(vec!["a".into(), "b".into()]));
        assert_eq!(
            intralinks.docs,
            IntralinksDocsConfig::Flat {
                base_url: "https://example.org/doc".into()
            }
        );
    }

    #[test]
    fn invalid_values() {
        assert!(matches!(
            parse("line-terminator = \"cr\""),
            Err(ConfigError::InvalidField(_))
        ));
        assert!(matches!(
            parse("heading-base-level = 300"),
            Err(ConfigError::InvalidField(_))
        ));
        assert!(matches!(
            parse("[entrypoint]\nbin-name = \"x\""),
            Err(ConfigError::InvalidEntrypointTable)
        ));
        assert!(matches!(
            parse("[intralinks.docs]\nlayout = \"flat\""),
            Err(ConfigError::FlatLayoutRequiresBaseUrl)
        ));
        assert!(matches!(
            parse("not toml ="),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn deprecated_keys_warn() {
        let mut warnings = Vec::new();
        parse_config("[intralinks]\ndocs-rs-version = \"1.0\"", |w| {
            warnings.push(w.to_owned());
        })
        .unwrap();
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn load_resolves_readme_relative_to_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CONFIG_FILE_NAME),
            "readme-path = \"docs/R.md\"",
        )
        .unwrap();
        let nested = dir.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        let options = load_config(&nested, |_| {}).unwrap().unwrap();
        assert_eq!(options.readme_path, Some(dir.path().join("docs/R.md")));
    }
}
