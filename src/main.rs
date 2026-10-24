//! The `cargo rdme-with-mermaid` command.

use cargo_rdme_with_mermaid::cli::{self, Command};
use cargo_rdme_with_mermaid::generate::{self, Generated};
use cargo_rdme_with_mermaid::update::{self, Outcome};
use cargo_rdme_with_mermaid::{Error, resolve_options};
use std::process::ExitCode;

fn print_warnings(generated: &Generated) {
    for warning in &generated.warnings {
        eprintln!("warning: {warning}");
    }
}

fn install_rust_toolchain_for_intralinks() -> Result<(), cargo_rdme::transform::IntralinkError> {
    use cargo_rdme::transform::{
        EXPECTED_RUST_TOOLCHAIN, install_expected_rust_toolchain,
        is_expected_rust_toolchain_installed,
    };

    if is_expected_rust_toolchain_installed()? {
        eprintln!("info: rust toolchain `{EXPECTED_RUST_TOOLCHAIN}` is already installed.");
        return Ok(());
    }
    eprintln!("info: installing rust toolchain `{EXPECTED_RUST_TOOLCHAIN}`...");
    install_expected_rust_toolchain()?;
    eprintln!("info: rust toolchain `{EXPECTED_RUST_TOOLCHAIN}` installed.");
    Ok(())
}

fn main_inner(cli: cli::Cli) -> Result<(), Error> {
    let current_dir = std::env::current_dir().map_err(Error::CurrentDir)?;
    let options = resolve_options(cli.run.into(), &current_dir, |w| eprintln!("warning: {w}"))?;

    let generated = generate::generate(&options, &current_dir)?;
    print_warnings(&generated);

    if !generated.had_marker {
        eprintln!(
            "info: no marker found in the README file ({}).\n\n\
             cargo-rdme-with-mermaid expects a marker in the README where the crate’s documentation\n\
             will be inserted. This is the marker you should add to your README:\n\n{}",
            generated.readme_path.display(),
            cargo_rdme::MARKER_RDME,
        );
    }

    if options.check {
        update::check(&generated, !options.no_fail_on_warnings)?;
    } else if update::write(&generated, options.force)? == Outcome::Updated {
        eprintln!("info: updated {}", generated.readme_path.display());
    }

    Ok(())
}

fn main() -> ExitCode {
    let cli = match cli::parse_from(std::env::args_os()) {
        Ok(cli) => cli,
        Err(e) => e.exit(),
    };

    if let Some(Command::InstallRustToolchainForIntralinks) = cli.command {
        return match install_rust_toolchain_for_intralinks() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: failed to install rust toolchain: {e}");
                ExitCode::FAILURE
            }
        };
    }

    match main_inner(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(1))
        }
    }
}
