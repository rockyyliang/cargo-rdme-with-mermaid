//! The `cargo rdme-with-mermaid` command.

use cargo_rdme_with_mermaid::cli;
use cargo_rdme_with_mermaid::readme::generate::{self, Generated};
use cargo_rdme_with_mermaid::readme::update::{self, Outcome};
use cargo_rdme_with_mermaid::{Error, resolve_options};
use std::process::ExitCode;

fn print_warnings(generated: &Generated) {
    for warning in &generated.warnings {
        eprintln!("warning: {warning}");
    }
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

    match main_inner(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(1))
        }
    }
}
