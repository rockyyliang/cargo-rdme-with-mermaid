//! Locating the target package, its entrypoint and its README.
//!
//! Discovery runs `cargo metadata --no-deps`, so dependencies are never resolved or downloaded.

use crate::config::options::EntrypointOpt;
use cargo_metadata::{Metadata, MetadataCommand, Package, TargetKind};
use cargo_rdme::PackageTarget;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Errors locating the package.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// `cargo metadata` failed.
    #[error("failed to get cargo metadata: {0}")]
    Metadata(#[from] cargo_metadata::Error),
    /// No `Cargo.toml` was found.
    #[error("could not find `Cargo.toml` in \"{}\" or any parent directory", .0.display())]
    NoManifest(PathBuf),
    /// The manifest is a virtual workspace manifest, or is not a workspace member.
    #[error(
        "\"{}\" is not a package manifest (for a virtual workspace, select a package with \
         `--workspace-project <name>` or `workspace-project` in .cargo-rdme.toml)",
        .0.display()
    )]
    NoCurrentPackage(PathBuf),
    /// `--workspace-project` names no workspace member.
    #[error("project has no package \"{0}\"")]
    NoSuchPackage(String),
    /// The package has more than one library-like target.
    #[error("package \"{0}\" has more than one library target")]
    MultipleLibTargets(String),
    /// The selected entrypoint does not exist.
    #[error("failed to get crate's entry source file (entrypoint `{0}`)")]
    NoEntrypoint(EntrypointOpt),
}

/// A package whose documentation should be rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Package name.
    pub package_name: String,
    /// Absolute path to the package's `Cargo.toml`.
    pub manifest_path: PathBuf,
    /// The package's `readme` field, as reported by cargo.
    pub readme: Option<PathBuf>,
    /// Source path of the library target.
    pub lib_path: Option<PathBuf>,
    /// Source paths of binary targets, by name.
    pub bin_paths: BTreeMap<String, PathBuf>,
}

/// A source file to read documentation from.
#[derive(Debug)]
pub struct Entrypoint {
    /// Rust source file.
    pub path: PathBuf,
    /// Target kind, as needed by cargo-rdme's intralink resolution.
    pub target: PackageTarget,
}

impl Project {
    /// Runs `cargo metadata --no-deps` and selects the package.
    ///
    /// Without `workspace_project`, the package is the one owning `manifest_path` or, if none is
    /// given, the nearest `Cargo.toml` above `current_dir`. This matches cargo's notion of the
    /// current package.
    pub fn discover(
        manifest_path: Option<&Path>,
        workspace_project: Option<&str>,
        current_dir: &Path,
    ) -> Result<Project, ProjectError> {
        let manifest = match manifest_path {
            Some(path) => current_dir.join(path),
            None => cargo_rdme::find_first_file_in_ancestors(current_dir, "Cargo.toml")
                .ok_or_else(|| ProjectError::NoManifest(current_dir.to_owned()))?,
        };

        let metadata = MetadataCommand::new()
            .no_deps()
            .manifest_path(&manifest)
            .current_dir(current_dir)
            .exec()?;

        Project::from_metadata(&metadata, &manifest, workspace_project)
    }

    /// Selects the package from already-obtained metadata.
    pub fn from_metadata(
        metadata: &Metadata,
        manifest: &Path,
        workspace_project: Option<&str>,
    ) -> Result<Project, ProjectError> {
        let members = metadata.workspace_packages();

        let package = if let Some(name) = workspace_project {
            members
                .into_iter()
                .find(|p| p.name.as_str() == name)
                .ok_or_else(|| ProjectError::NoSuchPackage(name.to_owned()))?
        } else {
            let wanted = canonical(manifest);
            members
                .into_iter()
                .find(|p| canonical(p.manifest_path.as_std_path()) == wanted)
                .ok_or_else(|| ProjectError::NoCurrentPackage(manifest.to_owned()))?
        };

        Project::from_package(package)
    }

    fn from_package(package: &Package) -> Result<Project, ProjectError> {
        const LIB_KINDS: [TargetKind; 6] = [
            TargetKind::Lib,
            TargetKind::DyLib,
            TargetKind::StaticLib,
            TargetKind::CDyLib,
            TargetKind::RLib,
            TargetKind::ProcMacro,
        ];

        let mut libs = package
            .targets
            .iter()
            .filter(|t| t.kind.iter().any(|k| LIB_KINDS.contains(k)));
        let lib_path = libs.next().map(|t| t.src_path.clone().into_std_path_buf());
        if libs.next().is_some() {
            return Err(ProjectError::MultipleLibTargets(package.name.to_string()));
        }

        Ok(Project {
            package_name: package.name.to_string(),
            manifest_path: package.manifest_path.clone().into_std_path_buf(),
            readme: package
                .readme
                .clone()
                .map(cargo_metadata::camino::Utf8PathBuf::into_std_path_buf),
            lib_path,
            bin_paths: package
                .targets
                .iter()
                .filter(|t| t.kind.contains(&TargetKind::Bin))
                .map(|t| (t.name.clone(), t.src_path.clone().into_std_path_buf()))
                .collect(),
        })
    }

    /// The package directory.
    #[must_use]
    pub fn directory(&self) -> &Path {
        self.manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
    }

    fn default_bin(&self) -> Option<(&String, &PathBuf)> {
        match self.bin_paths.len() {
            1 => self.bin_paths.iter().next(),
            _ => None,
        }
    }

    /// Selects the source entrypoint, with cargo-rdme's rules.
    pub fn entrypoint(&self, opt: &EntrypointOpt) -> Result<Entrypoint, ProjectError> {
        let lib = || {
            self.lib_path
                .as_ref()
                .filter(|p| p.is_file())
                .map(|p| Entrypoint {
                    path: p.clone(),
                    target: PackageTarget::Lib,
                })
        };
        let bin = |entry: Option<(&String, &PathBuf)>| {
            entry
                .filter(|(_, p)| p.is_file())
                .map(|(name, p)| Entrypoint {
                    path: p.clone(),
                    target: PackageTarget::Bin {
                        crate_name: name.clone(),
                    },
                })
        };

        match opt {
            EntrypointOpt::Auto => lib().or_else(|| bin(self.default_bin())),
            EntrypointOpt::Lib => lib(),
            EntrypointOpt::BinDefault => bin(self.default_bin()),
            EntrypointOpt::BinName(name) => bin(self.bin_paths.get_key_value(name)),
        }
        .ok_or_else(|| ProjectError::NoEntrypoint(opt.clone()))
    }

    /// The README path: the manifest's `readme` field, or `README.md`, relative to the package.
    ///
    /// Returns `None` if the file does not exist.
    #[must_use]
    pub fn readme_path(&self) -> Option<PathBuf> {
        let relative = self
            .readme
            .clone()
            .unwrap_or_else(|| PathBuf::from("README.md"));
        Some(self.directory().join(relative)).filter(|p| p.is_file())
    }
}

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}
