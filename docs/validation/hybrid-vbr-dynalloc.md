# Hybrid variable-rate dynamic-allocation policy correction

Version 0.1.80 fixes the connected high-band analysis in Hybrid VBR. The earlier
path passed nominal VBR bytes and rate targets but retained the CBR flag in
`DynallocOptions`. Original `celt_encoder.c` passes `st->vbr` into
`dynalloc_analysis` for Hybrid too. The wrong flag halved non-transient boost
contributions and applied the CBR two-thirds cap at sufficient high-band rates.

The shared analysis boundary now takes variable policy from either CELT VBR or
Hybrid VBR controls. Constrained policy comes only from CELT: Hybrid retains
native unconstrained analysis. CBR and existing public APIs remain unchanged.
Hybrid VBR packets and decoded audio can intentionally differ from 0.1.78/79;
interoperability was not the defect. No perceptual improvement is inferred from
the corrected policy alone.

## Reproduction and evidence

The unchanged scalar fixed-point original reference is Opus v1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`. The existing optional full-source
probe calls original `dynalloc_analysis`; its new focused corpus contains
32 measured contexts and 768 analyses across mono/stereo, SWB/FB, 10/20 ms,
transient/non-transient, CBR/VBR and byte budgets immediately below/at/above
the activation threshold, plus 64/128/1275 bytes. It checks depth, intended
boost, each request, importance and spreading weight through the actual shared
analysis control boundary used by the frame encoder.

The regression fails with the old wiring: context 2, VBR, 40 effective bytes
requests 672 eighth-bit units instead of the native 1392. The corrected path
matches every result. Deterministic regeneration is byte-identical; existing
fixtures and invalid-domain checks remain unchanged.

```sh
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --hybrid-dynalloc-analysis > /tmp/hybrid-dynamic.txt
diff -u tests/hybrid-dynalloc-analysis-fixtures.txt /tmp/hybrid-dynamic.txt
cargo run --offline --locked --release --example hybrid_fec_vectors -- /tmp/new-hybrid-vbr.bin vbr
python3 tools/hybrid_encoder_validation.py /path/to/opus-v1.6.1 /tmp/new-hybrid-vbr.bin /tmp/hybrid-vbr.json --profile vbr
cargo build --offline --locked --release
python3 tools/hybrid_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-hybrid-vbr-files --vbr
python3 tools/hybrid_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-hybrid-vbr-fec-files --vbr --fec 20
python3 tools/hybrid_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-hybrid-vbr-fec-dtx-files --vbr --fec 20 --dtx
```

Actual shared-stream validation matches independent native PCM and final range
on 1152 streams / 184320 decoder records, including 30528 FEC requests. Presence
counts remain 10650 FEC requests with any LBRR, 54042 protected primary records,
25534 primary DTX records and 5304 DTX-input FEC fallbacks. These counts do not
mean every request recovers protected audio. See
[raw results](results/hybrid-vbr-dynalloc-0.1.80-vectors.json).

Three 16-file speech/music matrices (182928 packets total) pass independent
native PCM, exact duration/pre-skip, variable lengths and packet caps. Relative
to the retained earlier VBR recordings, three unprotected outputs change PCM;
protected profiles remain identical on these recordings because of their
available high-band budgets. FEC-only has 56274 LBRR packets; FEC/DTX has 56275
LBRR and 1286 DTX packets. See [normal files](results/hybrid-vbr-dynalloc-0.1.80-normal-files.json),
[FEC files](results/hybrid-vbr-dynalloc-0.1.80-fec-files.json) and
[FEC/DTX files](results/hybrid-vbr-dynalloc-0.1.80-fec-dtx-files.json).

The old normal Hybrid, DTX and CBR/FEC raw corpora remain byte-identical, as does
the complete CELT VBR/CVBR raw corpus. Six full SILK/CELT recording regressions
preserve packet and WAV bytes against verified 0.1.79. See
[regressions](results/hybrid-vbr-dynalloc-0.1.80-regressions.json).
All seven required offline gates pass, including complete debug/release suites,
denied Clippy/rustdoc warnings, zero-dependency audit and 23 Python tests.
Existing whole-stream rollback, DTX/FEC, geometry/rate/cap/toggle and late
termination tests continue to pass.

Native code remains optional external validation only. Raw recordings,
vectors, binaries and logs stay local; tracked reports retain sanitized
hashes/counts. This corrects a native policy mismatch, not a demonstrated
quality or speed advantage. General mode/channel orchestration, longer packets
and competitive end-to-end evaluation remain in the full-codec goal.
