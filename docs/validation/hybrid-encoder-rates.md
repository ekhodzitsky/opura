# Hybrid encoder rate controls

The `hybrid::encoder` control API calculates native SILK/high-band target rates
and the SILK entropy-search limit. Complete Hybrid PCM/packet/file encoding is
still under development. These controls alone do not establish interoperability,
quality or performance of an encoder.

`RateControls::bitrate` is the total payload target after subtracting TOC and any
transition-redundancy costs. `payload_bytes` is the remaining shared entropy
capacity. Channels are encoded channels, and `fec` is the selected native LBRR
policy, rather than the original user request. The caller must select SWB/FB and
10/20 ms and supply one or two channels. Rate controls reject zero or physically
oversized target bitrates, invalid channels/bands and capacities outside 2..1275.

Rate selection follows the original per-channel seven-row interpolation, half
allocation of extra rate above 64kbps/channel, CBR boost, SWB correction and stereo
adjustment. CBR Hybrid gives SILK its target bits plus 25% of the remaining bits
with original integer rounding. SILK still performs a capped variable-length
search; CELT absorbs variations in the common payload. VBR computes the SILK cap
from the maximum physical rate separately from the target rate. Tiny budgets can
yield negative high-band targets or even a nominal VBR SILK limit above physical
capacity. These original results are retained explicitly, rather than presented
as proof that the shared frame can fit. Packet admission must validate actual
entropy accounting and retain transactional state on errors.

The optional oracle uses unchanged Opus v1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`. It extracts the original
`compute_silk_rate_for_hybrid` and CBR/VBR cap blocks; the checkout must be clean
and at the pinned revision. The wrapper only supplies controls and prints
results. It does not replace the allocation algorithm with a second port.

```sh
python3 tools/hybrid_encoder_rate_fixtures.py /path/to/opus-v1.6.1 /tmp/hybrid-rates.txt
diff -u tests/hybrid-encoder-rate-fixtures.txt /tmp/hybrid-rates.txt
```

The retained 8688 rows cover both durations, bands, channel layouts, CBR/VBR and
selected-FEC states. Thirteen capacities span 2..1275 bytes; rates include table
knots and their immediate neighbors, odd stereo totals, partial capacity,
capacity and capacity-minus-one. Ordinary offline Rust tests compare all three
outputs exactly. Additional boundary tests reject extreme integer controls
before arithmetic, and a fixed 32kbps stereo example distinguishes target-rate
allocation from the cap when only the physical capacity changes.

CELT frame coding now accepts its live writer from the enclosing packet code;
initialization, finalization and TOC assembly remain outside that engine. The
public CELT API still emits CELT-only packets. Its existing exact fixture corpus,
cross-decoding tests and injected-finalization rollback tests check this
refactoring. Band17 Hybrid coding and shared SILK/CELT PCM integration remain
unfinished; this milestone makes no claim that two terminated payloads can be
combined.

Version0.1.74 passes all seven repository gates: formatting, complete offline
debug and release tests, all-target Clippy with denied warnings, rustdoc with
denied warnings, zero-dependency audit and23 Python tool tests. All checked-in
CELT packet/state fixtures and finalization-failure rollback tests pass unchanged.
