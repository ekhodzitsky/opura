# Current codec readiness and remaining work

The classic signed16 mono/stereo PCM decoder and shared encoder are implemented
for the advertised library and WAV/Ogg workflows. This is not a complete
competitive acceptance result. Production behavior remains0.1.95; the following
audit adds evidence and tests without changing the encoder/decoder algorithms.

## Integrated contract and evidence

| Contract | Observed evidence |
|---|---|
| Decoder: SILK, CELT, Hybrid, five rates, mono/stereo | Fresh full RFC8251 matrix:120 vector/rate/layout cases and every encoded nonzero final range pass. Corrupted-range and zero-PCM probes are rejected. See the [current source-fingerprinted report](results/codec-readiness-0.1.95-conformance.json). |
| Encoder: one PCM/SILK/CELT owner, five applications, all five input rates | [Application validation](encoder-applications.md) checks original input, HP/delay, active resamplers, speech PCM and real CELT call traces. CLI regressions check exact duration and lookahead. |
| Mode/wire/band continuity and packets2.5..120ms | [Long-packet validation](encoder-long-packets.md) checks real prefills, bandwidth handshake/acknowledgement, native component geometry and transition-tail placement. Shared-owner tests check transition ranges and transaction rollback. |
| CBR/VBR/CVBR, DTX/FEC, loss and tiny budgets | [Budget validation](encoder-budgets.md) checks early PLC, completed speech overflow, physical admission, retained histories and resume. Independent short/long/pressure corpora each cover9,600 receiver calls including800 PLC and800 FEC. [0.1.95 replay evidence](results/celt-rate-history-0.1.95.json) records the final hashes and fresh native PCM/range replay. |
| CELT complexity, transients, spreading and rate feedback | [Quality](celt-quality.md), [rate/intensity](celt-rate-history.md) and [prediction/loss/band](celt-controls.md) audits compare actual stateful encoding. The normal test suite now checks every payload byte, PCM and final range over21,836 actual CELT calls, plus768 paired spectral-helper decisions. |
| Invalid input and late errors | Shared-owner regressions compare the complete owner against its pre-call clone after failed short/long packets, invalid geometry/controls and impossible budgets. Successful insufficient-budget PLC follows its distinct documented state-retention contract. |
| File timing and fixed paths | Full Cargo integration tests retain empty/partial, silence/resume, all applications/rates, whole CBR budgets and error-before-creation checks. The [0.1.95 recording report](results/celt-rate-history-0.1.95-recordings.json) checks12 paired full-recording CBR profiles,24 native PCM matches and exact durations. Prior complete fixed corpus identity is recorded in0.1.94; those corpora were not regenerated in this validation-only audit. |
| Build constraints | The 0.1.95 control audit passed all seven offline gates with 162 library/23 Python tests; the 0.1.96 benchmark tooling also passes the gates, with 31 Python tests. No third-party Cargo dependency, unsafe policy exception or ordinary external codec requirement was added. |

The encoder's private reference build corrects only Q24 spectral-patch local
types, as disclosed in the linked audits. Its original checkout and reference
decoder remain unchanged. Original common-owner input/control fixtures use the
unchanged reference. These results do not claim global byte identity with every
native encoder configuration, floating-point build or neural enhancement.

Detailed component sections in README retain earlier development milestones.
Their old statements about missing later stages are historical, not the current
integrated contract in the table above.

## Open work toward the full goal

- Multistream channel mapping and self-delimiting subpacket framing are absent.
  Current packet/PCM and Ogg paths handle one mono/stereo stream; they must not
  be advertised as general multichannel support. The competitive contract names
  multistream explicitly, so it remains an open capability target.
- The Ogg CLI has a documented single-stream subset, and rejects multiplexed or
  chained streams. RTP is not implemented. These transport/container limits are
  separate from successful classic mono/stereo audio decoding.
- No representative matched-rate/latency encoder quality comparison exists.
  Two full recordings and waveform SNR are diagnostics, not perceptual evidence.
  The next comparison needs several speakers/languages, music, transients,
  noise and silence with recorded licenses/hashes and explicit loss schedules.
- The [in-process encoder baseline](../benchmarks/encoder-0.1.96.md) now covers
  60 cases/15 paired rounds against pinned scalar native encoding, with separate
  initialization, call latency and scoped memory. Its aggregate time ratio is
  1.244; broader competitor/perceptual acceptance and performance improvement
  remain work. File wall timings are not used as core-codec speed evidence.
- The [decoder0.1.41 baseline](../benchmarks/decoder-0.1.41.md) lost to the pinned
  scalar native reference in all ten configurations. This is a historical
  baseline, not a current0.1.95 speed measurement. Current scalar and best
  available competitor comparisons, including the other direct implementations
  in the [comparison contract](../benchmarks/competitive-landscape.md), remain
  necessary before any leadership claim.
- Cross-platform/target coverage and malformed-input/resource measurements must
  accompany a competitive release. Optional QEXT/neural/OSCE paths and no_std
  are not implemented; no_std is explicitly not a current project requirement.

Next work should move to these integration and measurement gaps rather than
repeat the closed CELT control matrix without a new failure or changed code.
The full codec goal stays open; no SOTA, universal superiority or whole-project
completion claim follows from this audit.
