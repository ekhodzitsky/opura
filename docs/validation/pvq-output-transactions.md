# PVQ reconstruction into staged partition output

Positive-pulse CELT partition leaves now reconstruct directly into their existing
output slice. This avoids returning a fixed-storage `NormalizedBand` and copying
its active coordinates into the partition. Public `pvq::decode_band` still
returns owning output and preserves entropy after every error. Its public
1/2/4/8-block restriction is unchanged; recursive partitions retain their private
16-block support and full 16-bit collapse mask.

## Ownership and arithmetic

`decode_partition_band_into` is visible only inside `crate::celt`. It checks the
validated codebook's positive pulse count, block geometry, nonnegative gain and
exact output length before decoding. Reconstructed Q24 coordinates, inverse
spreading, pulse energy and pre-spreading collapse masks use unchanged integer
arithmetic. Zero gain still returns the pulse occupancy mask even when every
output coordinate is zero. `PulseVector` remains an owning intermediate; this
change does not remove every temporary or establish a peak-memory saving.

The private path may advance entropy and modify output before a later error.
Production callers are the public owned PVQ wrapper and the positive-pulse
branch of `partition::Decoder::partition`. The first stages entropy and owns its
local output; the second writes only unpublished output under the enclosing
public partition, band, spectrum or complete decoder transaction. Recursive
splits and stereo paths share that enclosing boundary. Borrowed folding inputs
remain immutable. Future callers must preserve the same ownership contract.

The encoder continues to call the owning reconstruction wrapper with unchanged
search, gain and spreading policies. Shared arithmetic is factored into one
output-slice helper. This does not establish encoder performance or quality.

## Fault and native coverage

Five unit tests inject failure after actual pulse decoding and Q24 writes.
A direct private call demonstrates changed entropy and output before failure.
An undersized output is rejected before either changes; a known pulse in block
15 verifies the high mask bit with zero gain and the public/private block-count
distinction. Public PVQ, partition and band calls then prove exact entropy,
shared budget/seed rollback and resumed output against saved control state.

Separate CELT, Hybrid and unified Opus tests exercise complete-owner rollback
after written leaves. The CELT and Opus tests skip two successful leaves before
failing; the Opus test also covers signed output gain and a Hybrid-to-CELT
transition. Every fault asserts it was reached. Hooks and thread-local controls
compile only in unit tests, and tests pass with the default thread stack.

Four restored source mutations are detected at runtime: bypassing the public
PVQ entropy transaction, bypassing the public partition transaction, changing
the reconstruction gain, and truncating wide collapse masks. Independent native
PVQ and partition fixtures detect the arithmetic/mask changes rather than a
comparison between two copies of the same implementation.

Focused offline reproduction:

```sh
cargo test --offline --locked --lib pvq::transaction_tests
cargo test --offline --locked --test celt_pvq --test celt_partition \
  --test celt_band_decode --test celt_spectrum --test celt_decoder \
  --test celt_loss --test hybrid_decoder --test hybrid_loss \
  --test opus_decoder --test decoder_pcm --test celt_pvq_encoder \
  --test celt_partition_encoder --test celt_band_encoder \
  --test celt_spectrum_encoder
```

These native corpora pass, including 28464 decoded PVQ bands and 196608 refined
root cases, full spectral/entropy/history checks, and 1680 unified native
gain/long-packet/PLC/FEC/transition/reset operations. All seven required offline
gates pass (181 library tests in debug/release and 44 Python tests). All 120
official conformance cases pass with every 0.2.4 PCM hash unchanged. The
[correctness record](results/decoder-pvq-0.2.5-correctness.json) and
[conformance report](results/decoder-pvq-0.2.5-conformance.json) bind this evidence
to the final codec source. The [full paired comparison](../benchmarks/decoder-pvq-0.2.5.md)
finds 4.5% less aggregate codec time versus 0.2.4; six individual intervals
exclude 1, four remain uncertain and all per-case upper bounds are below 1.05.
The exploratory pilot is superseded. This is not native, encoder-performance,
peak-memory or SOTA superiority evidence.
