# Canonical surround encoding

`MultistreamEncoder::with_surround_application` adds persistent native surround
coding for canonical1..8channel Vorbis layouts. Generic `new`/`with_application`
continue to support their existing arbitrary mapped input contract. The new
constructor accepts Vorbis-order PCM; the file CLI converts canonical WAVE-order
input in a bounded per-packet buffer. Mono/stereo keep their original path.

## Contracts

Canonical maps, stream counts and coupled ordering follow the original public
surround factory. Layouts3..8 use the [validated spectral masks](surround-analysis.md).
Layouts6/7/8 identify the final coded mono stream as LFE. Native allocation gives
LFE its own offset and Q8 ratio32, distinct from ordinary mono/coupled streams.
Whole-packet CBR rounding, self-delimited headers,100ms reservations, resource
caps and final padding retain the common multistream transport contract.

The owner analyzes the complete input packet once. It maps spatial masks to
each coded stream and supplies a private, neutral-by-default context to the
actual common and CELT owners. Common application remains immutable. Coupled
surround streams preserve CELT/stereo spatial coding; restricted-SILK retains
its application constraint and skips surround spectral analysis/mask forwarding.
Native equivalent total-rate thresholds select bandwidth, then the common
owner applies standard input-rate/application limits and LFE policy.

SILK VBR target masking applies after the base rate/Hybrid split, including
speech prefills. Masked Hybrid bypasses high-band attenuation and masked stereo
bypasses PCM width fading. CELT uses actual masks for dynamic allocation, trim
and VBR targeting, including the native constrained/temporal policy differences.
LFE additionally changes mode/band selection, transient/TF/pitch gates, measured
band energies, coarse energy, spreading/tapset, first-band boosts, trim and
signal-bandwidth allocation. It is not simulated by a bandwidth switch alone.

LFE energy suppression happens before normalization. Consequently its upper-band
coordinates can exceed the public normalized-vector norm domain. A separate
internal spectrum entry accepts only the immutable analyzer-owned LFE result
with matching mono/frame/band geometry. Public spectrum encoding keeps its
original norm checks; regression tests reject those LFE coordinates through the
public entry and reject ordinary analyzer results through the private LFE entry.
No unsafe code, lint exception or general input-check relaxation is introduced.

Common and CELT masks have distinct lifetimes. Native CELT reset clears its mask
pointer while retaining LFE metadata; the common owner retains the current
packet's mask through transition prefills, redundant tails and long components.
The Rust owners follow these observed lifetimes. A complete multistream reset
clears histories and retains canonical role/application/rate. Any error discards
the staged analysis, all constituent owners and pending output together. A
late-stream regression changes failed PCM so unchanged repeated overlap cannot
conceal an accidental partial analysis commit.

## Version0.2 CLI migration

Positioned multichannel `--mode auto` now selects the native surround owner.
Unpositioned family255 and the generic library constructors retain generic
coding. WAVE/OpusHead mapping, signed gain, pre-skip/end trim and Ogg/RIFF limits
are unchanged. PCM reordering is bounded to one packet, not a whole recording.

Native surround forces stereo in coupled streams. The CLI therefore explicitly
rejects `--channels 1` for positioned multichannel input before output creation.
Previously that combination used generic per-stream mono coding. This changed
control contract requires the pre-1.0 minor increment to0.2.0. Existing positioned
mono/stereo input still supports its prior channel override. Generic mapped
library callers can continue using `with_application` for explicit custom wire
channel choices; family255 remains unpositioned generic file coding.

## Independent native evidence

All optional native tools require unchanged original Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`. Native builds stay outside Cargo;
ordinary tests use captured synthetic numerical/wire data. The spatial analysis
[Q24 correction](../decisions/0003-surround-q24-precision.md) is explicit. Actual
CELT/common observation uses only the previously disclosed Q24 spectral-patch
local-variable correction; native decoding remains unchanged. There is no
claim of pristine full surround encoder byte identity or perceptual improvement.

| Evidence | Scope |
|---|---|
|1920actual CELT operations|Five rates, mono/stereo, LFE, masks absent/zero/nonzero, CBR/VBR/CVBR, complexity/prediction changes and reset. Every packet byte, decoded PCM hash, final range and14persistent state fields match.|
|3240short common-owner operations|Exact SILK target, Hybrid gain, width, mode/band and every native CELT call's size/start/prediction/entry capacity/input PCM hash/LFE/mask hash. Coupled profiles follow actual forced-CELT surround policy; mono profiles include SILK/Hybrid/CELT transitions.|
|3240long common-owner operations|10..120ms packets check the same values over prefills, redundant tails and shared-mask component schedules, across applications and rate modes.|
|3240factory/rate cases|Pristine canonical layouts and complete LFE/non-LFE rate arrays match at all five rates, nine durations and low/high/threshold rates.|
|1290pristine wire-policy cases|All rates, layouts3..8, applications and valid durations. Native TOCs verify coupled/LFE policy. An explicit caller mono request still produces native forced-stereo coupled streams.|
|204surround cases/6760native receiver operations|Actual Rust output at all standard rates/applications/durations and CBR/VBR/CVBR, including96drops and real PLC/FEC/DTX. Both sides match full PCM/ranges. Initial run observes14FEC-step LBRR streams and2508suppressed frames.|
|2160actual CLI files|Canonical positioned surround and unpositioned generic files match native trimmed PCM, headers, channel order and complete packet budgets. Source tones for LFE are actual bass within its passband; channel correlations verify identity, not perceptual quality.|

The common oracle's original arbitrary masked-stereo SILK experiment is outside
the canonical surround contract: native surround always forces its coupled
streams to CELT, except restricted-SILK which never receives masks. Accepted
profiles enforce this actual policy and retain all mono speech/Hybrid paths.
Unmasked generic stereo/SILK coverage remains in the existing normal suite.

Optional reproduction:

```sh
python3 tools/surround_celt_fixtures.py /path/to/opus /new/path/celt.txt
python3 tools/surround_common_fixtures.py /path/to/opus /new/path/common.txt
python3 tools/surround_common_fixtures.py /path/to/opus /new/path/long.txt --long
python3 tools/surround_rate_fixtures.py /path/to/opus /new/path/rates.txt
python3 tools/surround_policy_fixtures.py /path/to/opus /new/path/policy.txt
python3 tools/multistream_encoder_interop.py /path/to/opus /new/path/interop --surround
python3 tools/mapped_audio_interop.py /path/to/opus target/release/opura /new/path/files.json
```

Each output must be new; failed/partial experiments are not accepted evidence.
The native receiver validator requires the exact204-case/6760-operation protocol,
rejects wrong expected PCM/ranges and requires actual DTX/FEC coverage. Its header
is distinct from the retained generic protocol; the generic229-case validator
and checks remain available.

Fourteen isolated source mutations are detected by running assertions, covering
coupled mode/channels, LFE rates/energies, SILK masking, Hybrid gain, width fading,
CELT mask reset, dynamic/trim/VBR masks, partial analysis commit, WAVE input order
and the public norm guard. Restored library/file suites pass again.

These results establish the specified classic surround path. Representative
matched-rate/latency listening comparisons, current performance/memory studies,
other competitors and portability acceptance remain work under the full goal.

## Final release verification

All seven required offline gates pass at the final0.2.0 source, including172
library tests and37 Python tests. The full120-case RFC8251 decoder matrix passes
with every encoded nonzero final range checked; all PCM hashes remain identical
to0.1.97. The retained generic229-case/7405-operation native replay also passes,
and its packet/PCM export body is identical to0.1.97 after the version header.

The [release evidence](results/surround-encoder-0.2.0.json) records gates, source
hashes and all14 detected mutations. Final independent reports retain
[surround replay](results/surround-encoder-interop-0.2.0.json),
[generic replay](results/multistream-generic-0.2.0.json),
[file interoperability](results/surround-files-0.2.0.json) and
[RFC8251 conformance](results/codec-readiness-0.2.0-conformance.json).
These reports explicitly record dirty pre-commit provenance; their source
fingerprints match the committed source. The file report's legacy generic scope
label is retained as generated: this release routes positioned multichannel
files through actual surround coding, as the source and file regressions show.
