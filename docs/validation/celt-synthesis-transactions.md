# CELT synthesis transactions

The decoder stages its complete owner before processing a public call. CELT
normal frames and noise/pitch concealment now render directly into that staged
backend instead of cloning the backend a second time. Direct public
`CeltSynthesis::synthesize` retains its independent transaction. The public
API, owning PCM storage, arithmetic, validation and output geometry are unchanged.

## Ownership boundary

`synthesize_staged` and `render_concealed_staged` are visible only within
`crate::celt`. Their callers may mutate histories before returning an error and
must run on an unpublished owner. The audited production paths are:

| Backend path | Immediate caller | Complete transaction owner |
|---|---|---|
| Normal spectrum | `CeltDecoder::decode_layer` | CELT `decode`/`decode_frame`, Hybrid packet decode/FEC, or unified Opus packet decode/FEC |
| Noise concealment | `CeltDecoder::noise_loss` | CELT `conceal_samples`, Hybrid concealment/decode/FEC, or unified Opus concealment/decode/FEC |
| Pitch concealment | `CeltDecoder::periodic_loss` | The same staged public concealment/packet owners |

Private layer and configured-concealment entry points rely on their enclosing
decoder transaction. They do not acquire an independent commit boundary. Future
callers must preserve this contract. Rendering directly on a published owner
would break error recovery even if the failing call returned no PCM.

The public synthesis wrapper commits only after its private renderer succeeds.
Other public decoder wrappers retain their existing complete-owner clone and
commit. This change removes only redundant backend copies; it does not remove
all temporary buffers or establish peak-stack, heap or RSS savings.

## Fault and reference coverage

Four unit tests inject errors after real MDCT overlap updates, left-channel
postfilter updates, deemphasis updates, and pitch history/overlap updates.
The direct private backend is demonstrated to have changed before the injected
failure. The public synthesis wrapper must preserve its entire original state
and produce identical output/state when resumed against a saved control.

The complete CELT owner is tested for normal frames, periodic pitch loss and
prolonged noise loss. Hybrid normal/noise calls are tested independently. The
unified Opus test exercises signed output gain, a 120ms FEC request containing
a 100ms PLC prefix, and a Hybrid-to-CELT transition. In the prefixed FEC test,
failure skips the five PLC frames and fires during the recovered frame. Every
injection asserts it was reached, then compares full owner state and resumed
PCM/diagnostics. Hooks and their thread-local controls compile only in tests.

Three restored source mutations are rejected at runtime: bypassing public
synthesis staging, bypassing public CELT packet staging, and omitting pitch
MDCT overlap restoration. The last is detected by independent native loss PCM
fixtures rather than by comparing two instances of the same implementation.

Run the focused fault and native corpora without an external build dependency:

```sh
cargo test --offline --locked --lib synthesis::transaction_tests
cargo test --offline --locked --test celt_synthesis --test celt_loss \
  --test celt_decoder --test hybrid_decoder --test hybrid_loss \
  --test opus_decoder --test decoder_pcm
```

The repository's seven offline gates pass (176 library and 44 Python tests).
All 120 official conformance cases pass with every 0.2.3 PCM hash unchanged.
The existing 1680-operation pristine native decoder corpus
covers five rates, mono/stereo, seven gains, long packets, PLC, prefixed FEC,
transitions and reset and passes, as do all 480 native synthesis cases and
history comparisons. The [source-bound correctness record](results/decoder-staging-0.2.4-correctness.json)
and [official conformance report](results/decoder-staging-0.2.4-conformance.json)
retain the final codec validation. Retained paired timing is pending; the
exploratory 48kHz stereo pilot is not a release result.
