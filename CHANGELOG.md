# Changelog

## 0.1.0 — unreleased

- Group configuration options under `config::options` and README stages under `readme`, preserving
  the original top-level library paths as re-exports.
- Remove the toolchain installation subcommand. Intralink resolution uses installed toolchains;
  missing-toolchain errors explain manual setup and explicit link stripping.
- Clarify that `--check` never writes the README, but intralink resolution can invoke Cargo/rustdoc
  and write build artifacts.
- Clarify when consumers need `simple-mermaid`: README extraction does not require it, while
  macro-based rustdoc builds and intralink resolution do. Include setup for both workflows.
- Fix quote escaping in the pipeline diagram for GitHub Mermaid rendering.
- Add inline comments identifying the README generation pipeline stages.
- Initial release: `cargo rdme-with-mermaid` and `--check`, built on the cargo-rdme 2.2 library,
  with inline expansion of `simple_mermaid::mermaid!` crate-level doc attributes.
