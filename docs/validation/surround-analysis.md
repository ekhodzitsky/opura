# Surround spectral analysis and allocation masks

`celt::analysis::SurroundState` supplies persistent standard Vorbis-order3..8
channel analysis at all five input rates. It retains a preemphasis term and
120-sample overlap for every channel. The API accepts standard durations
2.5/5/10/20/40/60/80/100/120ms and exact interleaved signed16 PCM. Large buffers,
invalid channel counts, wrong lengths and unsupported durations reject without
changing any history. Reset retains rate and layout and clears all histories.

The implementation reuses the existing safe Rust preemphasis, forward MDCT and
band-energy primitives. Low-rate input uses native zero insertion and spectral
scaling/cutoff. Long packets take the maximum amplitude in each band over20ms
analysis windows. Mean-adjusted Q24 logs receive the native upward/downward
spectral spreading and role-based left/center/right log-sum masking. The last
channel in6/7/8channel layouts is LFE and receives zero masks; its PCM histories
still advance. Restricted-SILK must bypass this analysis in the enclosing owner.

`SurroundMask` owns21 signed Q24 values for each channel.
`analyze_surround_allocation` separately reduces one/two coded channels' masks
using previous coded-band count0..21. It preserves the native minimum two bands,
weighted Q10 accumulation/truncation, bounded slope, conservative dynamic boost
adjustment and fallback. Outputs are positive Q24 dynamic-allocation requests,
signed Q24 trim and the surround VBR masking offset. Inputs outside the selected
bands are ignored; selected values outside[-32,32] reject. Explicit zero masks
produce a positive native VBR offset and must not be confused with absent masks.
LFE and Hybrid bypass this reduction in the enclosing encoder.

These analysis APIs alone emit no audio. Version0.2's canonical
[surround owner](surround-encoder.md) consumes them through native rate allocation,
common-owner masking and actual CELT/LFE processing. Generic constructors retain
their prior coding contract. Analysis primitive agreement alone establishes no
perceptual quality or performance claim.

## Independent fixed-point evidence

`tools/surround_analysis_fixtures.py` requires an unchanged external checkout at
Opus1.6.1 `22244de5a79bd1d6d623c32e72bf1954b56235be` and a C compiler. Two
temporary translation units retain pristine and corrected-Q24 analysis. The
[precision decision](../decisions/0003-surround-q24-precision.md) describes the
three asserted return/type/scale replacements. No foreign source or executable
is a Cargo dependency; ordinary tests use captured integer data.

`tests/surround-analysis-fixtures.txt` retains2160 exact corrected masks over
six layouts, five rates, nine durations and eight signal recipes. Full-scale
random/alternating PCM, impulses, quiet, center-only input, silence and mixed
signals exercise continuous histories and periodic resets. Each record also
includes an input PCM16LE fingerprint, every preemphasis term, a full overlap
PCM32LE fingerprint and a pristine-mask fingerprint. Rust matches all corrected
values and histories. All2160 masks differ from the pristine narrowing behavior;
PCM histories match both builds. Two complete generations are byte-identical.

`tests/surround-allocation-fixtures.txt` retains2816 exact results from the
unmodified CELT mask-reduction block extracted from the original source. Both
native builds agree. The matrix covers mono/stereo coded streams, every previous
coded-band count0..21, zero/positive/negative/saturated masks and64 patterns per
geometry. Rust matches every trim, masking offset and dynamic request. The
corpus requires actual nonzero boosts and slope, rather than just accepting
zero controls. Additional Rust tests check immutable input bounds, ignored
tails, zero-mask semantics and complete analysis-state preservation/reset.

```sh
python3 tools/surround_analysis_fixtures.py /path/to/opus /new/path/analysis.txt --allocation-output /new/path/allocation.txt
cargo test --offline --locked --lib surround
```

Outputs must be new. Failed partial runs are not accepted evidence. The helper
obtains the standard CELT mode through a live public encoder owner; it does not
require optional custom-mode allocation/destruction exports. Regeneration is
optional and never required for normal builds or tests.

Six isolated source mutations are detected by running assertions: log-sum width,
channel-offset scale, long-window maxima, overlap continuity, positive-mask
halving and VBR offset. Original sources are restored and all four targeted
surround tests pass again. This supplements the independent expected outputs;
it does not establish complete encoder behavior or quality.

The0.1.99 analysis milestone passes all seven required offline checks with
167 library and36 Python tests. The [source/fixture/gate record](results/surround-analysis-0.1.99.json)
binds every Rust source, the optional oracle, precision decision, fixture hashes,
observed counts and mutation results. Pre-commit worktree provenance is explicit;
file hashes are checked again after committing the milestone. Historical0.1.98
file interoperability evidence remains scoped to its own unchanged packet path.

The later [surround encoder validation](surround-encoder.md) checks native rates,
common-owner SILK VBR/Hybrid gain/width behavior, actual CELT masks/LFE processing
and per-packet/native-receiver behavior. The0.1.99 report above remains an analysis
prerequisite record, not an encoder-quality or speed result.
