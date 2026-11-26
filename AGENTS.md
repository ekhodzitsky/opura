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

### Milestone workflow

Start from Git status, the architecture and the outstanding Backlog tasks.
Record confirmed defects, evidence-backed improvements and untested hypotheses
separately. Group related tasks in a milestone. Give each task a priority,
owner, acceptance criteria, expected evidence and any dependencies.

Assign independent tasks to separate agents with explicit file ownership.
Each agent records its plan, implementation and verification in its task.
For a confirmed defect, run a failing regression before fixing the cause.
For a performance change, set the acceptance criteria before timing baseline
and candidate on the same inputs. Record negative results and drop changes
that miss those criteria.

The coordinator reviews the combined diff, runs the project checks, builds
from clean sources and exercises the installed product. Update the relevant
documentation and release metadata. Close tasks against their evidence and
record external blockers separately. Verify authorization before publishing a
release or changing repository visibility.

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
- `python3 -m unittest discover -s tools -p 'test_*.py'`
- `python3 tools/check_package.py`

The toolchain file pins the minimum supported Rust version. CI runs the same
checks. Optional fixture regeneration requires an external libopus; ordinary
builds and tests never require it. Production code forbids unsafe and denies
Clippy unwrap/expect lints; do not weaken these gates.

## Versioning and history

Maintain `CHANGELOG.md` with user-visible changes and relevant validation.
Use SemVer: before 1.0, incompatible API changes increment the minor version;
compatible additions and fixes increment the patch version. Keep Cargo.toml
and Cargo.lock synchronized. Commit completed, verified changes using Conventional
Commits in English (for example, `feat(encoder): add Hybrid encoding`).
The owner resumed remote work on 2026-11-02. Push completed, verified commits
and annotated version tags to the configured remote.
Tag versioned milestones with annotated `vX.Y.Z` tags; never move published tags.
Keep local Backlog records and machine-specific state out of commits.
