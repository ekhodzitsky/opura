# Native CELT prediction, loss and spectral-band controls

This audit changes validation, not production codec behavior. The implementation
remains version0.1.95 (`d7fe1bf`). Existing rate-history regressions are reused
for a separate6,144-call control-history fixture, with every payload byte, PCM
hash, final range and captured history compared against actual native encoding.

## Reference and workload

The pinned Opus1.6.1 reference is revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, scalar fixed fast64 Q24 with floating
analysis disabled. As in the [preceding audits](celt-quality.md), only the
spectral-patch local types are corrected in a private source copy. The original
checkout and all decoding remain unchanged. This does not claim pristine native
encoder byte parity or a perceptual advantage.

Thirty192-frame streams span five input rates,1/2 API channels and noise,
periodic200Hz waveform with a small noise component, or impulse-train input.
They combine:

- Prediction0/1/2, loss0/1/5/10/20/50/100 and complexity0..10.
- Legal NB/WB/SWB/FB end bands, mono/stereo wire changes and rate histories
  retained across narrowing and subsequent widening.
- CBR/VBR/CVBR,2.5/5/10/20ms, changing hard capacities2..1275, nominal target
  choices16..510kbps limited by capacity, recurring silence and resumption.
  The impulse streams use32-byte caps; embedded CBR uses BITRATE_MAX and its
  supplied physical capacity, as in the common owner.

Two additional192-frame48kHz mono/stereo API streams sustain periodic input at
20ms/complexity10 with160-byte capacities and64kbps VBR targets. Longer voiced
runs, prediction changes, loss, slower wire/band changes and silence/resumption
exercise pitch filtering beyond the initial mixed-pressure cases. The Rust
prefilter retains nonzero gain in67 frames; the regression requires at least50.

All6,144 complete payloads match the corrected native encoder byte for byte.
They also match decoded PCM/final range and spreading, tonal/HF averages,
tapset, temporal spectral average, intensity and reservoir/drift/offset/count.
The earlier10,560 quality,5,132 rate-history and768 helper cases are preserved;
all four fixture files regenerate identically. Ordinary tests require no native
library, C compiler or third-party Rust dependencies.

## Verification sensitivity

The new control test detects each isolated wrong implementation below; all
mutations are restored before required checks:

- Ignore prediction0's forced intra coding.
- Keep pitch filtering enabled for prediction0/1.
- Ignore packet loss in pitch filtering.
- Ignore packet loss in coarse energy coding.
- Retain inactive reconstructed-band energies during bandwidth narrowing.

```sh
python3 tools/celt_quality_fixtures.py target/reference/opus \
  target/quality.txt --patch-output target/patch.txt --correct-q24 \
  --rate-output target/rate.txt --control-output target/control.txt
cmp target/control.txt tests/celt-control-history-corrected-q24-fixtures.txt
cargo test --offline --locked --lib rate_history_tests
```

The [audit report](results/celt-controls-0.1.95.json) records fixture identity,
mutations and offline checks. This closes the specified CELT control matrix;
it does not prove all possible input/control combinations, broader reference
builds or representative audio quality. See [codec readiness](codec-readiness.md)
for the integrated path and remaining project work.
