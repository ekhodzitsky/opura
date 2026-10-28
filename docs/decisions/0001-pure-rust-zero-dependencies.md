# Pure Rust with zero dependencies

Status: accepted

## Context

Opura starts as an empty repository for an Opus audio codec. The project
owner chose a pure Rust implementation with zero dependencies.

## Decision

Implement the codec in Rust. Do not add third-party Cargo dependencies,
including build, development, optional, or target-specific dependencies.
Do not use FFI or vendor a C/C++ codec implementation. Rust standard
libraries are allowed; this decision does not require no_std support.

## Consequences

Tests and benchmarks must respect the same dependency constraint. External
reference programs and published vectors can inform validation, but the
normal build and test suite must not require libopus or another codec.
Implementing the codec and its numerical primitives is project work.

The proposed first milestone is packet framing and validation, followed by
decoder development. This sequencing is a proposal, not a claim of full
codec support or conformance.
