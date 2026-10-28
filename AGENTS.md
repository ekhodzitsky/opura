agent-dev-kb: 0.10.0

# opura

## Coding principles

[Coding principles](agents/coding-principles.md) — read before acting.

## Local-only files

[Local-only files](agents/local-only-files.md) — read before acting.

## Backlog.md

This repo uses Backlog.md (not GitHub Issues, not a second spec tool).
Run `backlog instructions overview` before work. Mutate via the CLI
(`--plain` / `--json`). One task has one agreed scope; sessions may resume it.
Local research → `doc create`; draft conclusions → `decision create`;
work → `task create`. Accepted, sanitized decisions belong in tracked
project docs; local records link to them. Labels: `audit`, `research`.

## Project setup

Opura is a native, pure Rust implementation of Opus with zero third-party
dependencies, including normal, build, dev, optional, and target-specific
dependencies. Do not link or vendor C/C++ codec implementations. Rust standard
libraries are allowed; no_std support is not yet a requirement. External
reference executables and test vectors may be used for validation without
becoming build or test dependencies.

## Product direction

Build the complete decoder before the encoder, prioritizing speed and
reliability. Read [the accepted direction](docs/decisions/0002-decoder-first.md).
Claims of SOTA performance require reproducible end-to-end evidence.

## Checks

Run before marking code work complete:

- `cargo fmt --all -- --check`
- `cargo test --offline --locked`
- `cargo test --offline --locked --release`
- `cargo clippy --offline --locked --all-targets -- -D warnings`
- `RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --no-deps`
- `python3 tools/check_dependencies.py`

The toolchain file pins the minimum supported Rust version. CI runs the same
checks. Optional fixture regeneration requires an external libopus; ordinary
builds and tests never require it. Production code forbids unsafe and denies
Clippy unwrap/expect lints; do not weaken these gates.

## Versioning and history

Maintain `CHANGELOG.md` with user-visible changes and relevant validation.
Use SemVer: before 1.0, incompatible API changes increment the minor version;
compatible additions and fixes increment the patch version. Keep Cargo.toml
and Cargo.lock synchronized. Commit completed, verified changes with descriptive
English messages and push them to origin as requested by the repository owner.
Tag versioned milestones with annotated `vX.Y.Z` tags; never move published tags.
Keep local Backlog records and machine-specific state out of commits.
