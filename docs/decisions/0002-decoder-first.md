# Decoder first, then encoder

Status: accepted

## Decision

Deliver a complete pure Rust, zero-dependency Opus decoder before the encoder.
Prioritize speed and reliability. The long-term ambition is a state-of-the-art
Opus tool; this is a goal, not a description of current capabilities.

## Evidence and evaluation

The project owner explicitly selected decoder-first development with speed
and reliability as the priorities. The existing zero-dependency decision
continues to apply, including development and build dependencies.

Compatibility must be demonstrated using official conformance vectors and
reference behavior for supported configurations, including final range checks.
Robustness needs malformed-input, resource-boundary, transition and loss tests.
Performance claims require reproducible comparisons with a pinned reference,
recording hardware, compiler settings, corpus, throughput, latency and memory.
Microbenchmarks of primitives cannot establish whole-codec superiority.

Encoder quality comparisons require a complete encoder, specified bitrates,
latency constraints and a representative corpus. Those comparisons are later
work and must not be inferred from decoder correctness.

## Boundary when accepted

At acceptance, packet framing and entropy decoding were available. SILK, CELT,
Hybrid synthesis, loss concealment, full conformance and an audio tool still
required implementation and validation. Audio support must only be advertised
after an end-to-end decoded PCM path is verified. See README.md and CHANGELOG.md
for subsequent implementation milestones.
