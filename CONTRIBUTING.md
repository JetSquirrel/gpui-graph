# Contributing

## Development setup

Use the stable Rust toolchain with Clippy and rustfmt, as specified in
[`rust-toolchain.toml`](rust-toolchain.toml). macOS is the primary development
platform for the examples; install the Xcode Command Line Tools before building.

```sh
cargo run --locked                 # Interactive query application
cargo run -p basic --locked        # Minimal embedding example
```

The query application uses an in-memory graph. It does not require a database,
credentials, or an `.env` file.

Linux CI tests the library. Its native dependencies are listed in
[`.github/workflows/ci.yml`](.github/workflows/ci.yml). The example applications
are not currently checked on Linux, and Windows is not covered by CI.

## Project conventions

- Keep reusable graph behavior in `crates/gpui-graph`; application UI and data
  sources belong in `examples/`.
- Keep shared dependency versions and package metadata in the workspace
  `Cargo.toml`. Update the pinned GPUI and gpui-base versions together after
  checking compatibility.
- Use `cargo fmt` and the checked-in `rustfmt.toml` for Rust formatting.
- Follow the existing public API conventions: `with_*` for value builders,
  field names for boolean builders, `is_*` / `has_*` for boolean readers, and
  `set_*` for mutations. Public structs and enums are `#[non_exhaustive]`.
- Use gpui-base theme tokens for UI styling. Keep layout and geometry logic
  independent of application-specific data sources.
- Keep documentation and new code comments in English. Update both READMEs
  when a change affects their usage examples or documented behavior.
- Commit `Cargo.lock` so examples and CI use the same dependency resolution.

## Validation

Run these checks from the repository root before submitting changes:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- --deny warnings
cargo test --workspace --locked
cargo package -p gpui-graph --locked
```

Packaging expects a clean working tree. During local development, use
`cargo package -p gpui-graph --locked --allow-dirty` to validate uncommitted
changes. This builds a local package without publishing it.

Add focused regression tests for behavior changes. For visual or interaction
changes, also run the relevant example and describe the manual checks in the
pull request. Explain the problem, the resulting behavior, and validation.

## Releases

The library version lives in `crates/gpui-graph/Cargo.toml`; example packages are
not published. Maintainers update the library version and `Cargo.lock`, run the
checks above, and push a tag matching `gpui-graph-v<version>`.

The [release workflow](.github/workflows/release.yml) verifies the tag against
the crate version, runs formatting, linting, and tests, publishes to crates.io
using the repository's `CRATE_TOKEN` secret, and creates a GitHub release.
