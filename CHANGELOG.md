# Changelog

Versions follow SemVer. Before 1.0, incompatible public API changes increment
the minor version; compatible additions and fixes increment the patch version.
Each version has an annotated Git tag. Entries describe implemented behavior
and validation, without implying full Opus compatibility before PCM decoding.

## [Unreleased]

## [0.1.96] - 2026-11-03

- Add optional in-process unified-encoder and pristine scalar libopus benchmark
  adapters with matched prepared PCM/controls, separate initialization and frame
  latency, independent cross-decoder PCM/range checks and live corruption probes.
  Native compilation remains external to Cargo; no dependencies or unsafe Rust
  are added. Codec algorithms are unchanged from0.1.95.
- Add calibrated randomized paired measurements, source/binary/corpus provenance,
  explicit memory/API output scopes, actual packet diagnostics and fail-closed
  report validation that recomputes confidence intervals. The full protocol has
  60 cases including all applications/rate modes, five rates, packet durations
  through120ms, loss/DTX and complete speech/music recordings. Retained results
  are recorded after measurement; benchmark availability is not a speed claim.
- Expose both endpoints of the existing paired percentile interval and correct
  its description: the unchanged upper gate is the97.5th percentile, the upper
  end of an equal-tail95% interval. Existing competitive thresholds are retained.

- Audit native CELT prediction0/1/2, packet loss0..100, changing spectral bands
  and mono/stereo wire layout over6,144 actual calls. Every payload byte, PCM,
  final range and captured rate/intensity history matches the disclosed
  Q24-corrected encoder oracle. Periodic and impulse inputs supplement noise;
  sustained voiced schedules include active prefilter histories. Five isolated
  mutations are detected. The audited codec algorithms match0.1.95.
- Extend ordinary Rust regressions and preserve the prior quality/rate/helper
  fixtures unchanged. Record current capabilities and outstanding multistream
  and comparative evaluation work in docs/validation/codec-readiness.md.
- Preserve the post0.1.95 complete official RFC8251 decoder audit: all120
  cases/ranges and corruption probes pass. The unchanged decoder is covered by
  current required regressions. All seven offline gates pass, including162
  library and30 Python tests; benchmark/report tests account for the increase.

## [0.1.95] - 2026-11-03

- Retain the actual CELT allocation intensity boundary for the next native
  frame's hysteresis, including zero after mono coding. Previously the shared
  owner retained the requested boundary even when a tight budget clipped it.
  A stateful12kHz stereo VBR sequence reproduced different PCM/final range;
  retaining the realized boundary corrects that mismatch. Fixed public paths
  retain their existing policy.
- Extend the explicitly Q24-corrected optional encoder oracle with actual
  CBR/VBR/CVBR switches, changing rates/caps/durations/complexity, silence,
  mono/stereo wire changes and a1100-frame saturated-feedback stream. Compare
  packet bytes, PCM, final range, spreading, spectral average, realized
  intensity and reservoir/drift/offset/count directly with native state.
- Match every payload byte, PCM and final range across5,132 actual rate-history
  calls. This also eliminates the three byte differences retained in0.1.94:
  the existing10,560-frame fixture now matches every byte. Strengthen its
  regression assertion to require complete byte identity.
- Three isolated mutations are detected and restored; all three fixtures
  regenerate identically. Fresh native decoding matches each complete9,600-call
  short/long/pressure corpus, with hashes unchanged from0.1.94. All seven
  required gates pass, including161 library and23 Python tests.
- Twelve paired full-recording CBR cases retain0.1.94 payloads/PCM; all24
  outputs match unchanged-native decoding, exact duration and byte budgets.
- See docs/validation/celt-rate-history.md for coverage and validation.
  This fixes native control continuity; it does not establish SOTA quality.

## [0.1.94] - 2026-11-03

- Correct native complexity-zero CELT spreading and skip temporal masking
  analysis while retaining tone detection. Preserve fixed public defaults.
- Enable the missed-transient detector with Q24 local differences at native
  complexity5 and above. Preserve the long-window dynalloc comparison,
  compensate short-window scaling and set the patched TF estimate. Update
  temporal VBR history before the spectral window switch.
- Document an intentional departure from pristine fixed-point libopus: its
  int16 local narrowing disables this detector. The optional encoder oracle
  corrects only those local types in a private copy; the reference checkout
  and all reference decoders remain unchanged.
- Verify768 paired spectral decisions and10,560 actual CELT frames across
  input rates, channels, durations, complexity0..10 and changing capacities.
  Controls, temporal VBR history, decoded PCM and final ranges match the
  corrected encoder oracle.10,557 payloads are byte-identical; three different
  codewords produce identical PCM and final range. Five mutations are detected.
- Add reproducible before/after recording diagnostics for12 automatic AUDIO
  CBR profiles on complete speech/music recordings. All24 outputs match the
  unchanged native decoder, exact durations and physical byte budgets.
  Waveform metrics and process timings do not establish perceptual quality
  or competitive superiority. See docs/validation/celt-quality.md.
- Independently replay all three9,600-call shared-owner corpora with exact
  native PCM/final ranges, including PLC/FEC. All24 complete fixed SILK/Hybrid
  corpora retain0.1.93 hashes, sizes and statistics (1,384,320 records).
  All seven required offline gates pass:160 library and23 Python tests.

## [0.1.93] - 2026-11-03

- Support native early PLC for1/2-byte caps and nominal rates below useful
  admission. Construct the correct retained-mode/band/wire empty-frame packet
  through120ms and whole CBR padding, without advancing PCM/layer/policy
  histories. Retain actual selected mode separately from logical outgoing-tail
  history. Reject the unrepresentable100ms/one-byte combination atomically.
- Complete outer SILK encoding beyond a soft budget and replace a completed
  overshoot with a native PLC payload containing one zero byte, retaining states.
  Preserve one pending-FEC prefix-free retry, including allocation failures.
  Add `EncodedPacket::used_plc` for either early or component loss signalling.
  Other coding failures remain transactional errors.
- Clamp positive user rates to at least500bps and at most750000bps per API
  channel before physical/CBR admission. Apply effective SILK internal-rate ceilings for small
  budgets, retain CELT BITRATE_MAX when a forced Hybrid high-band setter is
  rejected, and skip raw stereo-width analysis on forced mono as native does.
  Fixed public owners retain their existing contracts.
- Connect automatic file caps down to1 byte and low nominal rates, including
  CBR caps, with exact granules and pre-skip. Reject100ms/one-byte files before
  output creation. Keep legacy fixed-mode option admission unchanged.
- Match5,630 original early PLC packets and126 completed-overflow packets,
  plus live state/input/call traces across7,200 original calls. Preserve all
  five raw width histories and100 explicit impossible-cap errors. Both native
  fixtures reproduce identically; five isolated mutations are detected.
- Fresh unchanged-native decoding matches9,600 pressure calls/100 streams,
  with800 explicit PLC/800 FEC calls and7,955 encoded packets signalling PLC;
  corpus regeneration is identical. Prior complete short and long automatic
  corpora remain byte-identical. All seven required offline gates pass,
  including158 library and23 Python tests.85 tiny CBR/VBR CLI cases pass.
- Bound recorded PCM counts before optional native replay, reject an oversized
  FEC-count regression and repeat final valid native replay after the guard.
- All24 complete fixed SILK/Hybrid corpora match committed0.1.92 SHA256,
  sizes and record statistics:1,384,320 records/5,088 streams. See
  docs/validation/encoder-budgets.md and its versioned evidence reports.
- Remaining quality/complexity/control review and competitive real-audio
  evaluation remain open; budget compatibility is not a SOTA claim.

## [0.1.92] - 2026-11-03

- Add compatible `encoder::PacketSize` and `OpusEncoder::encode_packet` for
  complete 2.5..120ms packets. Analyze and select policy once per packet;
  retain the short-frame `encode` API. Schedule native SILK 40/60ms units,
  20ms transform units, first incoming and final outgoing transition tails,
  final-only bandwidth acknowledgement, native toMono handling and shared
  prefills. Reserve framing from the whole CBR/VBR budget and repacketize
  transactionally; report the final component range.
- Preserve repeated native prefill initialization, mono resampler internal
  frame boundaries, inactive side state during rate changes and side LP
  initialization when stereo returns. Fixed public owners retain their paths.
- Connect automatic WAV/Ogg packets through120ms via `--packet-ms` or
  `--frame-ms`, with exact duration and whole-packet CBR caps. Read large PCM
  windows through bounded WAV reads; reject invalid duration combinations.
- Compare 5,248 original long-packet input/control states and actual CELT
  call traces; both native fixtures regenerate identically. Four isolated
  schedule mutations are detected and restored. Fresh native decoding matches
  9,600 actual received calls, including 800 PLC and 800 FEC; corpus repeats
  byte-identically. The complete 0.1.91 short corpus is unchanged.
- CLI checks cover250 long files in addition to300 prior short empty/partial
  files; both duration flags and whole CBR budgets are checked. All seven
  required offline gates pass, including157 library and23 Python tests.
- All24 complete fixed SILK/Hybrid corpora match committed0.1.91 hashes,
  sizes and record statistics:1,384,320 records in5,088 streams. See
  docs/validation/encoder-long-packets.md and its versioned evidence reports.
- Native tiny-budget PLC, remaining quality/complexity/rate-control review
  and competitive evaluation remain open. Long packet compatibility does not
  establish perceptual superiority or SOTA performance.

## [0.1.91] - 2026-11-03

- Support all five immutable applications in the common packet owner via
  `OpusEncoder::with_application`, with application-preserving reset and native
  lookahead reporting. Connect zero-compensation restricted input paths, native
  retained-history bounds and restricted-SILK bandwidth switching without CELT
  tails. Existing `new` remains AUDIO; fixed-mode defaults remain unchanged.
- Connect native adaptive VOIP high-pass filtering to actual previous SILK pitch,
  quality and activity histories, outer cutoff smoothing and per-API-channel
  fixed-point biquads. Preserve rate-switch and full-reset initialization rules.
- Correct embedded CELT CBR digital-silence signalling and entropy accounting.
  Reserve the remaining budget without extra symbols, retain the CBR size and
  include preceding overlap in the silence decision. Standalone fixed defaults
  remain compatible; silent embedded CBR packets now match original bytes/ranges.
- Add usable WAV/Ogg `--mode auto` through the shared owner, with application,
  signal, complexity, encoded-channel and bandwidth controls, CBR/VBR/CELT CVBR,
  DTX and FEC. Respect application-specific pre-skip and exact empty/partial-file
  duration. Reject unsupported long automatic packets before output creation;
  legacy file modes/defaults are unchanged.
- Compare 5,120 original application histories/call traces, 640 independently
  varied VOIP cutoff/filter cases and 2,560 silence/resume rows. All 662 quiet
  packets match original bytes/ranges; four isolated mutations are detected.
  All four fixture files regenerate identically from the pinned reference.
- Fresh unchanged-native decoding matches 9,600 received calls in 100 streams,
  including 800 PLC and 800 FEC calls; final corpus regeneration is identical.
  CLI checks preserve duration/pre-skip in 300 empty/partial files and exercise
  controls in five longer files. All seven required offline gates pass,
  including 155 library and 23 Python tests; Cargo remains dependency-free.
- All 24 fixed SILK/Hybrid corpora match committed 0.1.90 evidence exactly:
  1,384,320 records in 5,088 streams. See
  docs/validation/encoder-applications.md and its versioned results.
- Full long-packet scheduling, native insufficient-budget PLC, remaining quality
  control review and competitive evaluation remain work. No full-codec readiness
  or state-of-the-art performance claim is made.

## [0.1.90] - 2026-11-03

- Add `encoder::OpusEncoder`, a real shared AUDIO owner over one PCM/DC/delay,
  speech/resampler/bandwidth and CELT history. Automatically select primary
  mode, band and wire channels at8/12/16/24/48kHz for2.5/5/10/20ms frames.
  Support nominal-rate CBR/VBR and CELT CVBR, packet caps, loss, DTX and FEC;
  stage the whole stream and retry pending FEC once on a capacity failure.
- Connect actual native SILK prefills, CELT2.5ms dummy encodes, incoming/outgoing
  5ms transition tails, SILK bandwidth handshakes, gain/stereo fades, transition
  syntax placement and final-range XOR. Preserve separate speech wire-channel
  history through early SILK DTX. Hybrid accepts common filtered PCM without
  repeating DC filtering and separates immutable API from actual wire channels.
- Apply root-supplied nominal/FEC/entropy budgets and CELT prediction/complexity/
  loss controls while retaining existing fixed public APIs and defaults. Match
  native digital-silence activity for band switching, complexity-zero tone
  filtering, and prediction retention through bandwidth transition tails.
- The new owner currently accepts AUDIO and single frames only. VOIP/restricted
  applications, long-packet scheduling, automatic WAV/Ogg CLI integration and
  competitive quality/speed evidence remain work. Low-budget admission can
  return an error. This is not a complete adaptive encoder or a SOTA claim.
- Compare1,408 original cross-mode/short-switch speech and CELT input states,
  3,072 original bandwidth histories and all corresponding actual CELT call
  traces (1,136 calls, including160 real dummy encodes). Four isolated mutations
  fail these tests; all five native fixture files regenerate identically.
- Fresh unchanged-native decoding matches11,520 actual calls in120 streams,
  with CBR/VBR/CVBR, mode/layout changes, DTX,960 PLC and960 FEC calls. Corpus
  regeneration is byte-identical. All seven required offline gates pass,
  including23 Python tests; Cargo remains dependency-free.
- Preserve CELT wire/band/prediction and retained PCM on successful early SILK
  DTX through layout changes; a new regression reproduces the prior failure at
  frame33. All gates and native replay were repeated after correction. All24
  fixed corpora match committed0.1.89 (1,384,320 records/5,088 streams); all four
  Hybrid corpora were additionally regenerated from the final build and match.
  See docs/validation/unified-audio-owner.md for scope and reproducible evidence.

## [0.1.89] - 2026-11-03

- Separate private SILK payload preparation from entropy termination and packet
  formatting. Keep the range writer open for outer transition symbols, and
  capture final range after those symbols. Fixed public owners keep their
  existing whole-stream transactions, retry, DTX, overflow and padding behavior.
- Separate Hybrid low-band preparation from high-band completion. The low-band
  phase leaves prior CELT, committed input history and gain/width fades untouched,
  allowing the future common owner to place transition syntax before CELT.
  Fixed Hybrid still writes the no-redundancy flag between these phases.
- Check live-writer continuation and phase/history boundaries across mono/stereo
  and 10/20ms frames. Preserve existing native fixture and rollback assertions.
  These private integration boundaries do not implement automatic encoding,
  native cross-mode prefills or redundancy tails. Public APIs are unchanged.
- All seven required offline gates pass, including 23 Python tests. All 24
  fixed-mode corpora match an isolated committed 0.1.88 baseline byte-for-byte:
  1,384,320 records across 5,088 streams, including 220,896 FEC requests and
  768 zero-length PLC records. This matrix repeats exact prior native-validated
  bytes; native replay was not rerun. Cargo remains dependency-free.

## [0.1.88] - 2026-11-03

- Replace duplicated mono/stereo SILK packet coding with one borrowed generic
  payload engine. Reuse the caller's speech and bandwidth histories, actual
  wire-channel FEC policy, entropy ordering, LP placement, budget/retry behavior,
  DTX decisions and final packet formatting. Statically dispatched analysis and
  prefix callbacks retain the existing public mono/stereo error types and APIs.
- Expose private Hybrid preparation/finalization over borrowed PCM, speech,
  CELT and fade histories. Keep whole-stream staging, FEC retries, reset and
  commit in existing public owners. Validate borrowed Hybrid geometry and active
  layer configurations before mutation; preserve the unfinished entropy writer.
- Verify 128 paired packet/whole-history results through capacity-two mono and
  stereo entry points, including DTX/FEC and wire changes. A low-rate capacity-two
  mono case catches an isolated mutation using capacity instead of wire count
  for FEC eligibility. Existing native fixtures and failure assertions remain
  unchanged. Fresh native replay matches the unchanged 11,520-call layout corpus.
- These are actual fixed-path entry points for the shared-owner integration.
  The complete adaptive owner, application processing, native prefills/tails and
  automatic library/CLI path remain work. See docs/validation/borrowed-packet-engines.md.
- All 24 complete fixed-mode corpora match the isolated committed 0.1.87
  baseline byte-for-byte: 1,384,320 records across 5,088 streams, including
  220,896 FEC requests and 768 zero-length PLC records. All seven required
  offline gates pass, including 23 Python tests; Cargo remains dependency-free.

## [0.1.87] - 2026-11-03

- Add stateful mono/stereo wire-layout requests to the actual stereo-input SILK
  packet encoder. `set_encoded_channels` preserves live mid/input histories;
  `encoded_channels` and `encoded_channels_target` distinguish the last packet
  from the request. A live mono request first collapses width in a stereo packet;
  a fresh stream starts mono directly. Reset retains the request, and errors
  preserve transition phase and all histories.
- Share native mono/stereo mid buffering. On stereo return, initialize side
  prediction/VAD/stereo fields while retaining mid/silence memories, and clone
  the live mid resampler before consuming input. Downmix before resampling;
  average both departing resampler outputs for the first mono internal frame.
  Resample 40/60ms input by internal frames, including unused native delay slots.
  Use actual wire channels for packet headers, FEC policy, DTX and LP scheduling;
  clear incompatible pending FEC when wire shape changes.
- Verify 4,608 original public Opus input/layout histories, including complete
  mid-PCM fingerprints and both resampler states; 32 original partial stereo
  initializers and 120 original stereo-analysis/prefix cases cover width collapse
  and stereo return. Eight isolated mutations fail assertions. A fresh native
  decoder replay matches 11,520 calls across 120 actual streams, with 1,620 wire
  changes, PLC, FEC and silence/resume. Optional fixture/corpus regeneration is
  byte-identical. The replay tool now supplies the requested PLC duration.
- Keep default stereo bytes/APIs compatible. This connects actual speech layout
  ownership; the complete adaptive SILK/CELT/Hybrid owner, cross-mode prefills/tails
  and automatic library/CLI path remain work. See docs/validation/silk-channel-layout.md.
- All 24 prior corpora match 0.1.86 byte-for-byte: 1,384,320 records across
  5,088 streams, including 220,896 recovery requests and 768 zero-length PLC
  records. All seven final offline gates pass, including 23 Python tests.

## [0.1.86] - 2026-11-03

- Connect common AUDIO input filtering and retained PCM history to fixed mono
  SILK, stereo SILK and Hybrid encoding. Retain the native unmodified 10ms input
  window; derive Hybrid CELT's 4ms compensation before high-band/stereo fades.
  Filter before resampling, skip history advancement on early SILK-DTX, and
  advance it on outer digital-silence suppression. Stage input history with
  packet state and preserve FEC retry/reset/error contracts and fixed APIs.
- Add 1,440 unchanged-native filter/alignment/history snapshots across all five
  rates, both API layouts and 2.5..120ms frames, including short/long boundaries
  and skipped history commits. Native public DTX schedules verify actual mono
  and stereo packet history completion. Five isolated filter/history/DTX
  mutations fail assertions; optional native fixture regeneration is identical.
- Shared input processing is connected to real fixed packet paths; a complete
  adaptive encoder, VOIP high-pass processing, channel/resampler handshakes and
  cross-mode prefills/tails still require integration. See
  docs/validation/encoder-pcm-history.md for evidence and remaining boundaries.
- All 24 fixed-mode corpora match 0.1.85 byte-for-byte: 1,384,320 records across
  5,088 streams, including 220,896 FEC requests and 768 zero-length PLC records.
  Corpus hashes also match the prior native-replayed evidence. All seven final
  offline gates pass, including 23 Python tests; native PCM fixture generation
  remains optional and ordinary builds/tests use no external implementation.

## [0.1.85] - 2026-11-03

- Factor a common speech-state implementation and frame analysis out of mono
  SILK, stereo SILK and Hybrid PCM encoding. Centralize resampler/channel/VAD/stereo
  histories, mono sample buffering, side restart, silence/activity policy,
  DTX/FEC state and frame completion. Keep DC/delay, bandwidth permission and
  packet-specific entropy/budgets in their existing enclosing owners.
- Retain public fixed-mode APIs/defaults and original native fixture expectations.
  Mono buffering now advances per staged analysis frame, preserving one
  continuous sample delay through long packets and prefix-free FEC retries.
  This is private state/processing reuse, not the complete unified encoder;
  shared outer input ownership and native transition prefills/tails remain work.
- Compare 24 complete corpora byte-for-byte against 0.1.84 and replay all of them
  with unchanged native decoding: 1,384,320 records across 5,088 streams, including
  220,896 FEC requests and 768 zero-length PLC records. Retain all fixture values
  and fault-injection assertions. All seven required offline gates pass,
  including 23 Python tests. See docs/validation/shared-speech-state.md.

## [0.1.84] - 2026-11-03

- Add CELT `set_bandwidth`/`bandwidth` controls for NB/WB/SWB/FB bounded by the
  input-rate default. Connect actual spectral end and packet TOC, retaining
  input filters, active-band and rate histories across changes. Clear inactive
  reconstructed energies in both API channels after packet completion; preserve
  state on invalid controls or early/late errors. Reset retains selection.
  Reject unsupported MB/above-input bands; preserve all existing defaults.
- Match640 additional original-component packets/state snapshots and6400
  unchanged public native decoder records through coupled band/channel changes.
  Stereo/mono-input VBR/CVBR/CBR-toggle/silence/loss replay matches20480 native
  records, including880 losses,11152 received band changes and2240 wire changes.
  Five isolated source mutations are detected. See
  docs/validation/celt-bandwidth.md for reproducible evidence and limits.
  All seven required offline gates pass, including23 Python tests; fixed-layout
  and channel-only VBR corpora remain byte-identical to their prior milestones.
  The shared SILK/Hybrid/input owner and cross-mode transitions remain work.

## [0.1.83] - 2026-11-03

- Separate fixed CELT PCM input channels from encoded channels. Add compatible
  `set_encoded_channels`/`encoded_channels` controls for stereo input emitting
  mono/stereo CBR/VBR/CVBR packets without resets. Preserve both input filter,
  overlap and pitch histories, downmix after MDCT, mirror reconstructed mono
  energies and use coded channels for TF/rate/allocation/VBR decisions. Reset
  retains selection; invalid controls and early/late encoding errors preserve
  complete state. Existing fixed-layout encoder/CLI behavior remains unchanged.
- Validate640 exact original-component packets/state histories and6400 unchanged
  public native decoder PCM/range records. Actual VBR/CVBR/silence/CBR-toggle
  replay matches10240 native records including440 losses and2240 received wire
  layout changes. Four isolated source mutations are detected; previous fixed
  VBR corpus and optional pitch fixtures remain byte-identical. See
  docs/validation/celt-channel-layout.md for evidence and remaining scope.
  All seven required offline gates pass, including23 Python tests.
  General unified PCM ownership and SILK/Hybrid/mode transitions remain work.

## [0.1.82] - 2026-11-03

- Add shared scalar encoder frontend primitives: transactional raw-PCM stereo
  width analysis and native application/signal/mode/channel/band/FEC decisions.
  Preserve equivalent-rate recalculation, hysteresis, delayed stereo downmix,
  transition requests and SILK readiness constraints. Tiny budgets explicitly
  require outer PLC handling. Existing fixed PCM encoder paths remain unchanged.
- Validate16444 unchanged-native policy snapshots and4320 stereo-width history
  records, including all standard rates/durations, fixed-point edge signals and
  rare12kHz/2.5ms remainder handling. See docs/validation/encoder-frontend.md.
  Six isolated arithmetic/state mutations are detected by assertion failures;
  all seven required offline gates pass, including23 Python tests.
  These are preparatory control APIs: the unified PCM owner, actual adaptive
  transitions and complete encoder evaluation remain unfinished.

## [0.1.81] - 2026-11-03

- Add allocation-free borrowed Opus repacketization with canonical code0/1/2/3
  framing, range selection and exact padding, up to48 compatible frames/120ms.
  Append/write errors preserve buffered state and destination bytes.
- Connect actual CELT/SILK/Hybrid CLI grouping through `--packet-ms`. Preserve
  per-frame histories, exact aggregate CBR credit, VBR/CVBR group caps, compact
  all-DTX output and source duration/pre-skip. Final or incompatible-TOC groups
  close early; one-frame defaults retain exact audio bytes. Bound encoder Ogg
  packets to61248 bytes. General cross-mode/channel adaptation remains work.
- Validate32816 unchanged-native framing/padding results,216 grouped CLI cases,
  empty/partial/silence/configuration boundaries,34 native recording files
  (33636 packets) and9672 native PCM/range/loss/FEC replay records.18 default
  full-file regressions preserve packets/PCM against rebuilt0.1.80. All seven
  required offline gates pass, including23 Python tests. See
  docs/validation/grouped-packets.md for reproducible evidence and limits.

## [0.1.80] - 2026-11-02

- Fix Hybrid VBR high-band dynamic analysis incorrectly using CBR halving and
  boost caps. Select variable analysis for both CELT and Hybrid VBR controls;
  keep constrained feedback exclusive to CELT. Hybrid VBR packets/audio may
  intentionally change; all CBR/SILK/CELT behavior and APIs remain compatible.
- Retain768 unchanged-native high-band policy cases; the connected old path
  fails before the fix (672 versus1392 eighth-bit boost units in one case).
  Independent native PCM/ranges match184320 actual decoder records, and48
  recording files preserve interoperability/duration over182928 packets.
  Three unprotected recordings change PCM; existing protected profiles, old
  Hybrid CBR/DTX/FEC, CELT VBR/CVBR and six SILK/CELT file regressions retain bytes.
  All seven required offline gates pass, including23 Python tests. See
  docs/validation/hybrid-vbr-dynalloc.md for reproducible evidence and limits.

## [0.1.79] - 2026-11-02

- Add actual CELT VBR/CVBR PCM/packet encoding at all five standard input rates,
  mono/stereo and2.5/5/10/20ms. Expose compatible `encode_vbr`/`VbrOptions` and
  CLI `--vbr`/`--cvbr` with a separate hard `--max-packet-bytes` cap. Keep CBR
  defaults and fixed SILK/CELT/Hybrid packet behavior.
- Connect native fixed-point stereo/depth/boost/TF/temporal targets, constrained
  pre-cap, post-trim shrink, reservoir/drift/count and spectrum histories. Deduct
  TOC bits from the nominal target using native CELT signalling accounting.
  Native silence retains the previous raw input overlap and advances analysis;
  all rate/audio histories roll back on errors, including late termination.
- Validate8192 unchanged-original target/state decisions and1024 temporal
  history snapshots, actual native PCM/ranges/loss and recording budgets/duration.
  General mode/channel adaptation, longer packets and competitive evaluation
  remain unfinished. See docs/validation/celt-vbr.md for final evidence and limits.
- Independent native PCM/ranges match10240 records including440 losses and2299
  silence packets.32 recording files preserve native PCM/duration over304816
  packets; measured CVBR bitrate error stays below0.09% on this corpus. Six
  fixed SILK/CELT file regressions and all four Hybrid raw corpora retain bytes.
  All seven required offline gates pass, including23 Python tests.

## [0.1.78] - 2026-11-02

- Add actual unconstrained Hybrid VBR through compatible `encode_vbr` and CLI
  `--mode hybrid --vbr`, separating nominal TOC-inclusive bitrate from the hard
  packet cap (`--max-packet-bytes`, default1276). Keep existing CBR APIs/defaults.
- Select final length after dynamic requests/trim, before fine/PVQ allocation.
  Match original offset/temporal/transient targets, two-byte safety margin and
  shared37-bit redundancy-syntax floor; relocate raw suffix bits transactionally
  and allocate spectra against the actual new capacity. Carry VBR through SILK
  caps, fixed-band FEC selection and high-band analysis without weakening DTX/FEC
  rollback or one-time termination. Native CELT target-domain errors are explicit.
- Retain20160 original VBR size snapshots and25600 additional original VBR FEC
  policy cases, alongside unchanged CBR controls. Validate actual variable packets,
  caps, toggles, normal/loss/DTX/FEC, late errors and recording outputs. See
  docs/validation/hybrid-vbr.md for final evidence and limits. General mode/channel
  orchestration, longer packets and competitive evaluation remain unfinished.
- Independent native PCM/ranges match184320 records including30528 FEC requests;
  48 actual recording files preserve native PCM/duration over182928 packets.
  All three previous Hybrid raw corpora and six SILK/CELT recording regressions
  remain byte-identical. All seven required offline gates pass, including23
  Python tests; both new native policy fixtures regenerate identically.

## [0.1.77] - 2026-11-02

- Add opt-in shared-stream Hybrid LBRR generation and previous-frame protection
  for mono/stereo, SWB/FB and10/20ms. Expose `set_inband_fec`, `has_fec` and CLI
  `--mode hybrid --fec LOSS_PERCENT`, including DTX combinations. Retain fixed
  bandwidth; native equivalent-rate/FEC eligibility must keep the requested band.
- Reuse SILK redundant NSQ/entropy/mask/stereo/gain/bit-accounting state. Share
  budgets with primary SILK and CELT, clear incompatible packet geometry and
  preserve DTX early/outer ordering. Prepare the frame before one-time entropy
  finalization; a pending-prefix budget failure retries once from identical
  whole-stream state without that prefix. Failed calls preserve all histories.
- Validate25600 unchanged-native fixed-band policy cases, normal/loss/FEC/DTX
  cross-decoding, retained reset/rollback and CLI behavior. Hybrid VBR, general
  mode/channel selection and competitive evaluation remain work. See
  docs/validation/hybrid-fec.md for final evidence and limits. Independent native
  PCM/ranges match184320 records including30528 FEC requests.32 recording files
  preserve native PCM/duration over121952 packets; default Hybrid/DTX and six
  SILK/CELT full-file regressions remain unchanged. All seven required offline
  gates pass, including23 Python tests.

## [0.1.76] - 2026-11-02

- Add opt-in Hybrid DTX through `HybridEncoder::set_dtx` and CLI `--mode hybrid
  --dtx`. Suppressed packets contain one TOC byte and final range zero, including
  CBR. `in_dtx` reports the last successful result; reset retains the setting.
- Preserve distinct native state ordering: SILK inactivity returns before CELT,
  input-delay copy and gain/width fades; outer digital silence completes both
  layers and entropy termination first. All DTX counters and both layers roll
  back together on errors, including forced termination failure.
- Retain10800 original forced-Hybrid suppression and layer-call snapshots,
  covering24/48kHz, mono/stereo, SWB/FB,10/20ms, complexity0/6/10, silence,
  quiet input, refresh, disabling/re-enabling and speech resume. Default normal
  Hybrid behavior remains compatible; Hybrid FEC and general adaptation remain
  unfinished. Independent native decoding matches21600 normal/DTX/loss calls
  and16 real-recording files (60976 packets,1284 DTX), with exact PCM/ranges/
  duration and fractional byte budgets. All seven required offline gates pass,
  including23 Python tests. See docs/validation/hybrid-dtx.md.

## [0.1.75] - 2026-11-02

- Connect fixed-band mono/stereo Hybrid PCM encoding at24/48kHz,10/20ms and
  SWB/FB with a single shared range writer: WB SILK analysis/quantization,
  native rate/cap control, band17 CELT analysis/energy/TF/allocation/spectra and
  shared final range. Retain native high-band transient/spreading/trim rules.
- Align layers with4ms high-band input delay, DC rejection, input resampling,
  stereo predictor/width histories and Q15 gain/stereo fades. Whole-stream
  staging preserves both layers on geometry, budget and finalization errors;
  reset restores a fresh stream. Tiny budgets can fail explicitly.
- Expose `HybridEncoder` and CLI `--mode hybrid --bandwidth swb|fb` with10/20ms
  frames, native312-sample pre-skip and exact WAV/Ogg duration/CBR budgets.
  Hybrid DTX/FEC, VBR and general mode/channel switching remain unfinished.
- Validate actual shared packets against original libopus, including loss,
  full/partial-frame file boundaries, forced finalization rollback and native
  Q15 gain/fade fingerprints:240 streams/5760 normal/loss records,480 lost
  frames,15 gains/656 fades,60 CLI boundary cases and16 real-recording files
  (60976 packets). Independent native decoding matches actual PCM/ranges;
  no native-encoder quality/speed claim. All seven required offline gates pass,
  including23 Python tests. See docs/validation/hybrid-pcm-packets.md.

## [0.1.74] - 2026-11-02

- Add Hybrid rate-control calculations for mono/stereo, SWB/FB and 10/20 ms:
  native per-channel interpolation, selected-FEC/stereo/bandwidth corrections,
  CBR 25% borrowing and VBR SILK bit caps. Keep target bitrate separate from
  physical shared payload capacity; reject invalid controls before arithmetic.
- Separate CELT frame coding from entropy initialization/termination and packet
  assembly, preserving existing bytes and whole-stream error rollback. This
  prepares a shared Hybrid writer; complete Hybrid PCM encoding remains work.
- Retain 8688 decisions from unchanged pinned original libopus, including table
  boundaries, odd stereo rates, capped targets and tiny/maximum budgets. Optional
  fixture generation adds no Cargo dependency. Validation details are recorded
  in docs/validation/hybrid-encoder-rates.md. All seven required offline gates
  pass, including 23 Python tests.

## [0.1.73] - 2026-11-02

- Extend the optional recording baseline with matched SILK controls, explicit
  band/FEC/DTX/PLC inspection, packet budgets, reference artifacts and deterministic
  repetitions. Add loss replay with a checked output timeline, burst handling,
  zero-loss baseline equality and identical one-packet FEC wait for both encoders.
- Retain the measured 0.1.72 codec baseline: 88 SILK recording cases plus four
  unchanged CELT speech regressions. Actual Opura output decodes exactly through
  pinned original libopus; all 40 unprotected/fixed SILK cases also equal native
  encoder PCM. Protected matrices retain every quality loss and backup tradeoff.
- Retain 96 deterministic loss scenarios: 48 normal cases equal native output;
  48 protected cases span -0.376 to +5.031 dB SNR difference (6 lower, 18 equal,
  24 higher). These are two-recording waveform diagnostics with restricted-SILK
  reference controls, not perceptual, full-Opus or SOTA claims. Process timing
  includes startup/WAV/Ogg I/O; no native speed ratio is claimed.
- Rust codec behavior is unchanged. Validation tools use only Python's standard
  library and optional external reference tools; ordinary builds/tests retain
  zero third-party dependencies. Resume remote publication and require
  Conventional Commits as requested by the owner. All seven required offline
  gates pass, including 23 Python tests.

## [0.1.72] - 2026-11-02

- Add opt-in automatic SILK bandwidth selection to mono/stereo PCM encoders and
  CLI `--bandwidth auto`. Match original fixed-point AUDIO equivalent-rate,
  threshold/hysteresis, activity/WB-LP gates, API bounds, bandwidth-reducing FEC
  and hard byte-budget limits. Fixed channel layout is retained; fixed defaults
  and externally targeted behavior remain compatible. Reset retains the mode.
- Report automatic-mode native budget-overflow concealment via packet
  `used_plc()` and the CLI PLC packet count, separately from DTX. Keep native RDO
  channel quotas soft, retry queued FEC once without its prefix before falling
  back to a one-byte zero SILK payload with final range zero. Preserve actual
  encoded-bit reservoir and next-packet backup history; fixed-mode errors remain
  unchanged. The original 27200bps/10ms stereo overflow is retained/reproducible.
- Match 12480 original automatic policy/effective-rate/FEC snapshots across
  rates, budgets, complexity, loss, channel layouts and hysteresis history.

- Connect native SILK LP bandwidth transitions to raw mono/stereo PCM encoding.
  `set_bandwidth_target(Some(rate))` supplies an external target; actual changes
  follow native activity permission and packet acknowledgements, retain converted
  history, clear incompatible FEC and cap the desired rate by the PCM input rate.
  VAD sees unfiltered input; omitted stereo side frames do not advance their LP.
  `None` stops the taper at the current rate; reset retains the requested target.
  Errors roll back switching/filter state along with the rest of the stream.
- Match 60 unchanged native control/LP/activity sequences of 900 packets each,
  including 176 rate changes, 164 readiness events, direction reversals, duration
  changes, shared stereo bit reservations and 31635 omitted side frames.
  General mode/channel selection, signal classification, cross-mode transition
  redundancy, Hybrid encoding and quality/speed leadership remain unfinished.
- Independent unchanged libopus decoding matches 432000 gradual-band normal/loss
  records, including 72000 FEC requests. The workload guards completed NB↔MB and
  MB↔WB changes rather than counting requests as transitions. All seven offline
  gates pass; 19 Python tests and all 12 previous fixed/FEC/forced-band packet
  corpora remain passing and byte-identical.
- Independent native decoding matches 432000 automatic normal/loss records and
  72000 FEC requests. Count completed transitions separately from cold selection.
  Retain stress concealment: 9 mono / 5664 stereo PLC outputs per replay at the
  deliberately low total budgets; this is not a quality advantage claim.
- All seven mandatory offline gates pass on the versioned implementation. All
  16 prior packet/replay corpora remain byte-identical. Exact pinned native PCM
  matches 640 CLI files (480 fixed regressions, 160 automatic), with current
  binary version/SHA verified and zero PLC outputs at the tested CLI rates.
  All 480 old CLI PCM hashes remain unchanged. Retain FFmpeg-embedded libopus DTX
  PCM differences as measured deltas and the known independently reproduced
  builtin FFmpeg duration issue; neither is hidden by the exact pinned check.

## [0.1.71] - 2026-11-02

- Add `bandwidth` / `set_bandwidth` to mono/stereo SILK PCM encoders for forced
  packet-boundary NB/MB/WB changes. Convert buffered history through the API rate,
  prime live input resamplers, reset native prediction/shape/NSQ state and retain
  stream counters, VAD/DC, stereo, DTX and reservoir history. Clear incompatible
  queued FEC; same-band calls are no-ops and failed changes preserve all state.
- Add native SILK bandwidth handshake and variable-cutoff low-pass transition
  components. Retain readiness, direction reversals, signed interpolation and
  transition budget reservation; raw automatic policy/taper scheduling remains
  separate from the forced setters.
- Match 2400 unchanged native control decisions and 4626 complete LP PCM/state
  fingerprints, plus all 90 history migration combinations and 720 subsequent
  resampler PCM blocks. The existing channel golden now exercises actual rate
  migration instead of injecting native-converted history at rate changes.
- Validate dynamic-band normal/loss/FEC/DTX streams across all input rates and
  packet geometries, including simultaneous duration changes, stereo omission,
  direct and neighboring rate jumps, toggles and abrupt byte budgets.
- All seven mandatory offline gates pass, including 19 Python tests. Independent
  libopus matches 48000 dynamic-band decoded records and 7920 FEC requests.
  Existing 28320 no-FEC packets, 48000 FEC replay records and all 480 CLI PCM
  hashes remain unchanged; every CLI report matches the current executable.
- Automatic bandwidth/mode selection, Hybrid encoding and representative quality/
  speed evidence remain unfinished. Forced-rate interoperability is not a
  click-free quality or SOTA claim.

## [0.1.70] - 2026-11-02

- Add opt-in in-band FEC to mono/stereo SILK PCM encoders through
  `set_inband_fec` and `has_fec`, and CLI `--mode silk --fec LOSS_PERCENT`.
  Carry compatible prior-packet backups before current audio with native
  masks/stereo predictors, conditional coding, gain policy and bit-cost averaging.
- Preserve explicitly selected bandwidth. Evaluate native threshold/hysteresis
  policy at the per-channel packet payload rate; nonzero loss and sufficient
  activity/rate are required. Disabling FEC can still send one queued backup;
  reset retains FEC/DTX settings and clears all stream/queue history.
- Bound packet admission: if pending FEC prevents primary encoding from fitting,
  retry once from identical post-resampling state without that prefix. Preserve
  strict byte caps, atomic errors and all channel/VAD/quantizer histories. Clear
  incompatible queue geometry and suppress FEC diagnostics on DTX packets.
- Match 576 unchanged native fixed-bandwidth eligibility/gain cases; cover all
  input/internal rates, packet durations, CBR/VBR, sparse stereo masks, DTX,
  geometry/toggle transitions and abrupt budgets in packet/recovery validation.
- Independently match 48000 normal/loss/recovery decoded records with pinned
  libopus, including 7920 FEC requests. Verify 240 FEC CLI files with independent
  packet probes and FFmpeg/libopus, plus 240 no-FEC/DTX regression files.
- Add native pre-search LBRR quantization to the SILK channel core, using copied
  NSQ state and separate redundant gain history. Return saved redundant frames
  and serialize them with active redundancy entropy contexts in a later packet.
- Distinguish native per-frame RDO targets from an explicit hard packet bound
  in the new channel API; preserve the original `encode` strict-bound contract.
- Match 483 main frames, 243 redundant frames and 281 complete native packets,
  including exact symbols/pulses, entropy bytes and full main NSQ fingerprints.
  Match 280 independent recovery/continuation PCM results, including 93 loss
  requests (46 with LBRR, 47 without). Retain the native oversized-LBRR/PLC case.
- All seven required offline checks pass, including 19 Python tool tests;
  native FEC traces/policy regenerate exactly. All 28320 default/no-FEC packets
  are byte-identical and all 240 regression CLI PCM hashes match 0.1.69.
- Adaptive bandwidth, Hybrid encoding, complete automatic mode/rate policy and
  representative quality/performance evaluation remain unfinished. Fixed-mode
  FEC interoperability does not establish perceptual or speed leadership.

## [0.1.69] - 2026-11-02

- Add opt-in mono/stereo SILK DTX through `set_dtx` / `in_dtx` and CLI
  `--mode silk --dtx`. Preserve native inactive-frame hangover, periodic refresh,
  packet-sticky per-channel decisions and outer digital-silence timing.
  Suppressed output is one TOC byte with final range zero, including in CBR.
  Keep full analysis/quantizer histories running through pauses and preserve
  configured DTX on reset; defaults remain unchanged.
- Fix near-silent Burg analysis rejecting legitimate signed residual roundoff.
  Retain an unchanged native probe returning -4 in Q16 with exact coefficients;
  preserve that estimate in spectrum interpolation instead of rejecting PCM.
- Match 4220 native VAD/counter transitions, 400 outer clock decisions and
  all 2400 complete native mono/stereo suppression decisions across packet
  durations, silence, quiet nonzero input, speech resumption and control toggles.
- Independently decode 12000 mono and 12000 stereo packets across every input
  rate, SILK bandwidth, packet duration and CBR/capped-variable mode, including
  6720 suppressed packets per layout. Compare full PCM fingerprints/final ranges.
- Validate 60 mono and 60 stereo DTX CLI files with independent FFmpeg/libopus
  for exact durations, pre-skip, packet counts and fractional CBR/DTX budgets.
  Add silence byte-saving, reset, late-failure rollback and native regressions.
- All seven required offline gates pass, including 19 Python tool tests. Native
  traces regenerate exactly; both default packet corpora and all 120 default
  CLI PCM hashes remain unchanged from 0.1.68.
- FEC, adaptive bandwidth, Hybrid encoding and representative quality/speed
  evaluation remain open; DTX does not imply a computational speed improvement.

## [0.1.68] - 2026-11-02

- Add stereo SILK PCM-to-packet encoding at all five input rates, fixed
  NB/MB/WB bandwidths and 10/20/40/60 ms durations. Connect adaptive M/S
  prediction, width/rate allocation, mid-only omission and side resumption
  with independent channel analysis, exact CBR budgets or capped variable output.
- Extend `--mode silk` WAV/Ogg conversion to stereo with a 48 kb/s default;
  retain mono's 24 kb/s default and 312-sample pre-skip for both channel layouts.
- Match 144 unchanged public native stereo analysis calls, including transformed
  PCM, complete state, rate allocation, predictor indices and entropy bytes.
  Independently decode 120 stereo streams / 2880 packets with exact native
  sample counts, PCM fingerprints and final ranges. Fixtures/tables reproduce.
- Validate 60 stereo and 60 mono CLI files with FFmpeg/libopus for exact
  duration and pre-skip; ordinary CLI tests cover 120 geometries including
  empty/one-sample/boundary inputs. Retain and independently reproduce the
  builtin FFmpeg tail-trimming issue in each channel layout.
- All seven required offline gates pass, including 19 Python tool tests;
  mono packet corpus and all 60 mono CLI audio/PCM results remain unchanged.
- DTX/FEC, adaptive bandwidth, Hybrid encoding and representative quality/speed
  evaluation remain open. See docs/validation/silk-stereo-packets.md.

## [0.1.67] - 2026-11-02

- Add mono SILK PCM-to-Opus packet encoding at all five standard input rates,
  fixed NB/MB/WB bandwidth and 10/20/40/60 ms durations. Support exact CBR packet
  byte budgets or capped variable lengths, native DC rejection/resampling,
  one-sample channel carry, VAD and empirical SNR/bit-reservoir control.
- Expose mono SILK WAV-to-Ogg conversion with `--mode silk` and `--bandwidth`.
  Preserve default CELT behavior. Write the 312-sample pre-skip, flush lookahead
  and honor final granules for exact source duration, including empty/short files.
  Extend bounded WAV reads for 60 ms frames; refuse unsupported stereo SILK.
- Implement the connected SILK channel pipeline: native gain/NLSF quantization,
  Burg LPC/root recovery, interpolation, pitch/LTP, regular/warped noise shaping,
  residual energies, scalar/delayed-decision NSQ and full frame serialization.
  Retain fixed storage, zero dependencies, safe Rust and atomic failure history.
- Add native fixed 5 ms buffering/prefill, LTP loss scaling and seven-iteration
  gain/NSQ/entropy budget search. Cache gain costs, restore fitting snapshots,
  lock subframe gains and expose final damage-control pulse clearing explicitly.
  Preserve cumulative LTP gain across independent packet boundaries.
- Match 504 unchanged public native frame calls and 288 own decoded packets,
  including complete bytes, selected indices/pulses, entropy range/tell and every
  NSQ state fingerprint. Compare connected CBR/VBR analysis and retain encoder
  resampler/serialization/analysis fixtures without native Cargo dependencies.
- Independently cross-decode 120 raw-PCM streams / 1440 packets with pinned
  libopus: all PCM fingerprints, sample counts and final ranges match. Match
  40 native DC output/state cases; regenerate DC fixtures and SNR tables exactly.
- Verify 60 SILK CLI files with FFmpeg/libopus for exact duration/pre-skip, plus
  ordinary geometry, padding, impulse, reset and late-failure rollback tests.
  Retain one default-FFmpeg decoder tail-trimming failure, reproduced with its
  own libopus encoder; see docs/validation/silk-mono-packets.md for scope.
- Mono encoding is usable; stereo SILK, DTX/FEC, adaptive bandwidth, Hybrid
  encoding and broader quality/performance evaluation remain unfinished.

## [0.1.66] - 2026-11-02

- Add an optional reproducible real-recording CELT file evaluator using hashed
  official Opus speech/music examples and unchanged pinned fixed-point libopus.
  Keep downloaded recordings and listening outputs local; normal builds/tests
  gain no dependencies. Compare aligned signal diagnostics at matched packet
  budgets and retain repeated Opura CLI wall times, hardware and tool provenance.
- All seven offline gates pass, including 19 Python tool tests; signal
  diagnostics and PCM hashes reproduce across runs.
- Cover two recordings, two bitrates each and 5/20 ms frames. Validate native
  cross-decoding, exact sample counts, matching packet sizes and deterministic
  file output. Diagnostic SNR is not a perceptual quality score; process timing
  is not a competitive core-codec speed comparison. Full SILK/Hybrid encoding
  and broader subjective/competitive evaluation remain work.

## [0.1.65] - 2026-11-02

- Add the native `opura encode` / `opura decode` CLI with bounded RIFF/WAVE
  PCM16 and Ogg Opus I/O, adaptive CELT encoding and full-mode decoding.
  Support five input rates, mono/stereo, four frame durations and fractional
  packet-byte budgets. Flush encoder delay and honor pre-skip/EOS trimming.
- Validate CRC, page sequences, continuations, header interpretation and
  granule timing; reject unsupported mappings/gain/chaining and oversized
  packets. Refuse existing outputs and document partial-output failure behavior.
- Verify 51 encoded files with independent FFmpeg/FFprobe duration/geometry
  and packet accounting, plus three pinned native SILK/Hybrid/CELT files with
  exact reference PCM. Retain parser, malformed-input, boundary, timestamp
  origin, impulse-delay and actual CLI tests without external test dependencies.
- All seven required offline gates pass with zero dependencies; all seven
  optional native fixture artifacts reproduce byte for byte.
- Audio quality/speed leadership and SILK/Hybrid encoding remain unproven or
  unfinished; this milestone supplies usable file conversion for evaluation.

## [0.1.64] - 2026-11-02

- Integrate independent opt-in native pitch prefilter and comb signaling into
  CELT PCM encoding. Analyze raw context before filtering, transform filtered
  overlap/current samples (including dynamic-allocation long comparison), and
  preserve exact period/gain/tap signaling and post-comb transient availability.
- Keep raw/filtered histories continuous through disabling and reset; old gain
  ramps out. Drive spreading HF/tap updates from actual active long filtering,
  propagating the selected tap to the next frame. Reset retains all six settings;
  whole-frame rollback includes every prefilter/history field.
- Match all 96,828 original comb-field byte/range/accounting cases and paired
  decoding. Match 1,920 packets, state/history fingerprints and 19,200 unchanged
  public native PCM/range/history decodes; 382 frames use new gain, including
  53 short frames, with tap1/tap2 propagation and zero native energy differences.
- Verify 8,192 additional frames across all 64 settings, reset/small budgets and
  pre/post-finalize failure rollback with positive gain histories. All 16 focused
  integration mutations are detected. All 24 optional encoder evidence modes
  reproduce; seven offline gates pass with zero dependencies. Default packet
  corpora remain unchanged. Audio tooling, SILK/Hybrid encoding and representative
  quality/performance evaluation remain work; compatibility is no SOTA claim.

## [0.1.63] - 2026-11-02

- Add safe fixed-storage CELT encoder prefilter state and immutable filtered
  MDCT contexts. Preserve distinct raw1024/filtered120 histories and internal
  pitch/gain/tap across durations, reset and disabled transitions. All errors
  preserve complete state and inputs, including injected pitch-analysis failure.
- Match native tone shortcuts, ordinary pitch/loss attenuation, continuity/TF/
  byte thresholds, gain snapping/quantization, separate-source comb rounding,
  window/bias/saturation and cancellation. Preserve original signed16 stereo
  threshold narrowing. Share only immutable tap/window data with the decoder.
- Match every filtered sample, parameter and history in 2,048 unchanged-original
  stateful contexts (515 active), including mixtures, strong tones at low
  complexity, TF0.98 and byte25/35 boundaries, loss, extremes and toggles/reset.
  Another 1,024 bounded contexts and invalid/domain/reset/error checks pass.
- All 62 targeted mutations are detected, including feedback lag reads, raw vs
  filtered memory, crossfade/bias/saturation and stereo narrowing. All 22 optional
  encoder evidence modes reproduce; seven offline gates pass with zero
  dependencies. Packet comb signaling/integration, SILK/Hybrid encoding and
  representative quality/performance evaluation remain work.

## [0.1.62] - 2026-11-02

- Add fixed-storage native CELT encoder pitch search and remove-doubling
  period/gain diagnostics with checked previous comb histories and immutable Q12
  input. Share bounded whitening/search kernels with PLC, preserving its output.
  Check unsupported inner-product/rolling-energy/normalization domains explicitly.
- Match 11,972 original contexts, full active whitening fingerprints, eight
  continuity sequences and 10,407 refined periods. Match 82,025 scalar gain
  cases and 203,729 isolated refinement cases, including last-lag and continuity
  boundaries and a minimized one-unit threshold regression (seed 8382).
- Verify geometry/history/signal endpoints, checked accumulator failures and
  1,024 bounded random contexts. All 42 non-equivalent targeted mutations are
  detected; the 979-to-978 search-bound change is equivalent after native floors
  and is replaced by a 977-bound mutation. Four optional modes reproduce,
  including unchanged prior PLC evidence. All seven offline gates pass with
  zero dependencies.
- These standalone stages do not yet apply tone shortcuts, gain thresholds,
  quantization/cancellation, prefilter comb or packet signaling. Complete encoder
  quality/performance evaluation remains work; no SOTA claim is established.

## [0.1.61] - 2026-11-02

- Add independent opt-in CELT PCM allocation analysis: native equivalent rate,
  desired intensity hysteresis, LM-dependent L1 stereo and trim from adjusted
  energy logs/temporal estimates. Invoke trim lazily at the actual post-dynamic
  budget guard; preserve saving when skipped and desired history before clipping.
- Preserve all five settings on reset and clear allocation histories. Turning
  analysis off retains history; whole-frame rollback includes these new states.
  Existing public fixed allocation and default packet behavior remain compatible.
- Match 1,440 original packets, complete state/controls/accounting and 14,400
  unchanged public native PCM/range/history decodes under six policies/toggles.
  Add a one-active-channel LM0 regression. Match 1,056 isolated entropy tails
  including boost reservations, saving and all per-band allocation budgets.
- Verify another 4,096 frames under all 32 settings and small-budget/NB/FB/reset
  boundaries, plus 32-setting pre/post-finalize rollback and lazy callback errors.
  All 36 targeted mutations are detected. All 21 optional evidence modes reproduce
  and all seven offline gates pass with zero dependencies.
- Pitch prefilter, SILK/Hybrid encoding, DTX/VBR/FEC and representative quality/
  performance evaluation remain work; these results establish no SOTA claim.

## [0.1.60] - 2026-11-02

- Add standalone native CELT byte-equivalent rate, bounded L1 stereo modelling,
  checked intensity hysteresis and transactional trim/saving analysis. Preserve
  full-Q24 correlation order, signed averaging/log/tilt rounding and unused-input
  semantics. Reject unsupported final signed32 tilt accumulations atomically.
- Match all 525,825 correlation-log pairs, 10,192 byte-derived rates and 1,441,792
  intensity rate/history cases, original constants/defaults and 2,016 L1 spectra
  with complete costs. Match 16,233 trim outputs over 1,090 contexts, including
  negative LF averaging and a 40-step saving sequence; 1,216 out-of-domain sums
  reject without state changes. All 11 indices occur.
- Verify invalid/unused-prefix/exact-norm/reset/late-overflow domains and 1,024
  bounded contexts. All 44 targeted mutations are detected. New/prior optional
  evidence reproduces and all seven offline gates pass with zero dependencies.
- Packet integration remains subsequent work at actual post-dynamic trim-symbol
  availability; current trim/stereo packet policies remain unchanged. These
  component results establish no whole-encoder quality or performance lead.

## [0.1.59] - 2026-11-02

- Add bounded stateful native CELT pulse-spreading analysis with checked history,
  Q14 CDF/weighting, recursive/hysteresis decisions and optional HF/tapset updates.
  Expose complete immutable planar band normalization for frame composition.
- Add independent opt-in packet spreading with perceptual masking, native
  budget/window/byte gates and actual fallback/toggle history. Dynamic requests
  and TF importance retain their own setting. Reset retains all settings and
  restores numeric histories; the previous four packet corpora remain unchanged.
- Match 41,486 original analyses, native constructor defaults and all hysteresis/
  CDF/HF boundaries. Match 960 packets under four combined policies and spread
  toggles, full state/accounting and 9,600 public native PCM/range/history decodes.
  Another 768 threshold/setting frames and 16-setting finalization rollback pass.
- Match 72 isolated original entropy tails across 0..8 remaining bits, preserving
  numeric history when the spreading field cannot fit. This retained stage test
  closes a mutation gap not reached by a 66,640-context physical PCM search.
  All 40 targeted mutations are detected. Fifteen optional evidence modes
  reproduce and all seven offline gates pass with zero dependencies.
- Adaptive trim/stereo/pitch, SILK/Hybrid encoding and representative quality/
  performance comparisons remain work; compatibility establishes no SOTA claim.

## [0.1.58] - 2026-11-02

- Add bounded immutable native CELT dynamic-allocation analysis: noise/masking,
  median/stereo followers, importance/spread weights, tone compensation and
  CBR/CVBR caps. Preserve exact raw requests, including negative cap remainders;
  prove 0..255 writer adaptation equivalent across all 168 standard cap cells.
- Add independent opt-in dynamic packet analysis, native transient long-window
  comparison/scaling, previous-energy ordering, perceptual TF weights and boost
  requests. Reset retains all settings; the three previous packet modes remain
  byte-exact. Normal spreading and trim five remain explicit policies.
- Match 21,889 original analyses, 24,579 importance scalars, 270 median cases and
  canonical constants; verify bounded random/invalid contexts and cap endpoints.
  Match 960 full packets and 9,600 original public PCM/range/history decodes;
  512 additional threshold frames and eight-setting late rollback/reset pass.
  All 40 targeted mutations are detected. Previous and new optional evidence
  reproduce and all seven offline gates pass with zero dependencies.
- Further adaptive spreading/trim/pitch, SILK/Hybrid encoding and representative
  quality/performance comparisons remain work. Compatibility establishes no
  encoder quality or speed superiority.

## [0.1.57] - 2026-11-02

- Add bounded immutable CELT TF analysis: native biased L1/Haar levels,
  narrow-band midpoint metrics and weighted Viterbi/select decisions with exact
  tie behavior. Expose fixed-storage results and documented normalized domains.
- Add independent opt-in TF packet analysis with native budget/adjusted-tone
  gating and switching penalty, using explicit neutral importance. Reset retains
  both analysis settings; default and transient-only packets remain unchanged.
- Match 7,056 analyses over 672 original contexts, all 16,385 bias inputs and
  2,400 L1 scalars. Match 960 full TF-policy packets, effective TF/gate diagnostics
  and 9,600 original public PCM/range/energy-history decodes. Exact 15*C thresholds,
  144 additional boundary packets, reset/toggles and all four setting combinations
  under injected finalization errors pass. All 31 non-equivalent targeted mutants
  are detected; a one-unit clamp mutation is equivalent after quantization.
- Optional evidence reproduces and seven offline gates pass with zero dependencies.
  Perceptual dynamic/spread/pitch analysis, SILK/Hybrid encoding and measured
  quality/performance comparisons remain work.

## [0.1.56] - 2026-11-02

- Add bounded fixed-storage CELT tone/temporal-mask analysis with native LPC,
  frequency, strong/weak attack, channel and TF diagnostics. Keep existing public
  math contracts while adding private wide-Q29 division for tone estimation.
- Add opt-in transient detection to `CeltEncoder`, short MDCT/TF/allocation and
  native anti-collapse streak control. Default baseline packets remain unchanged;
  reset retains the setting and errors preserve all stream histories.
- Match 1,788 native contexts under both weak policies, 65,537 acos inputs and
  4,290 ratios, including exact mask thresholds and stereo/paired-energy rounding.
  Match 960 full packets (611 transient) and 9,600 original public PCM/range/history
  decodes, alongside the unchanged 720-packet baseline. All 31 targeted mutations,
  toggles/reset and injected pre/post-finalization rollback are detected.
- Optional evidence reproduces and seven offline gates pass with zero dependencies.
  Further adaptive CELT analysis, SILK/Hybrid encoding and quality/performance
  evaluation remain work; compatibility results establish no SOTA claim.

## [0.1.55] - 2026-11-02

- Add public fixed-storage `celt::encoder::CeltEncoder` and complete one-frame
  `EncodedPacket`: signed16 mono/stereo at five input rates, four durations,
  2..1275-byte payloads, correct TOC/bandwidth, reset and final-range diagnostics.
- Retain overlap/pre-emphasis, energy residual feedback, two-pass quantization,
  smoothed coded-band history and packet final-range seeds atomically. Initial
  policy uses long windows, no pitch/silence suppression, Normal spreading,
  trim five/zero boosts and stereo theta RDO; adaptive and SILK/Hybrid encoding
  remain under development.
- Match 720 complete original component-policy packets and full state/accounting,
  plus 7,200 independent public libopus PCM/range/history decodes. Native unused
  two-pass residual scratch is canonicalized outside selected bands only.
- Verify 2,560 additional public budget/rate/channel/duration frames, typed
  geometry/reset and injected pre/post-finalization rollback. All 22 targeted
  mutations detected. Optional evidence reproduces and seven offline gates pass
  with zero dependencies. These results establish compatibility, not quality or
  performance superiority over the reference adaptive encoder.

## [0.1.54] - 2026-11-02

- Add transactional native CELT dynamic/trim and retained-band/stereo allocation
  encoding with immutable boost-count, trim, intensity/dual and skip-hysteresis
  policies. Encoder/decoder share cap/interpolation/PVQ/fine/priority/balance math.
- Match 26,480 complete original allocation streams/arrays/counters across all
  3,696 legal geometry/interval/transient cells. Dense unboosted byte budgets
  and exact trim boundaries cover four gaps found by mutation testing; all 20
  targeted mutations and injected boost/skip/final-dual rollback are detected.
- Compose 160 PCM/control/energy/allocation/spectrum/final-energy test packets
  through the public decoder, matching component reconstruction PCM and ranges.
  Packet seed uses final range, independently of the local spectrum folding seed.
- New/prior fixtures reproduce and all seven offline gates pass with zero
  dependencies. A public audio encoder, adaptive policy analysis and quality
  evaluation remain work; test packet composition alone establishes no quality lead.

## [0.1.53] - 2026-11-02

- Add atomic native TF-control encoding from immutable binary band decisions,
  returning effective standard resolutions after differential signalling,
  reserved-select and scarce-budget fallback. Add spreading-field encoding with
  effective Normal fallback when fewer than four whole bits remain.
- Match 34,944 complete original control packets/mappings/counters, including
  8,736 empty intervals and 15,345 spreading fallbacks; paired decoding agrees.
  Validate domains, LM0/transient exclusion, finished no-symbol paths and injected
  first/middle/select-symbol failures. All 16 targeted mutations are detected.
- New and previous control fixtures reproduce, and all seven required offline
  gates pass with zero dependencies. Analysis decisions, allocation writing and
  audio packet assembly remain subsequent work.

## [0.1.52] - 2026-11-02

- Add atomic fixed-storage encoding of complete allocated CELT normalized spectra:
  shared budget/balance, folding histories, Hybrid-like first-band tail, dual to
  intensity transitions, collapse masks and final seed. Encoder/decoder share
  one statically dispatched all-band driver.
- Add native two-candidate stereo theta RDO with amplitude-weighted correlation,
  down-on-tie selection and complete winning writer/state/channel/fold restoration.
- Match 3,456 original RESYNTH whole packets/reconstructions/states, including
  629 dual and 751 eligible RDO frames, plus 4,271 native weights/correlations.
  Verify paired decoding, invalid selected domains, inactive-bin isolation and
  injected late capacity/vector errors after all shape work. Detect 21 targeted
  mutations, including premature commit before propagating a late failure.
- New/prior optional fixtures reproduce; all seven required offline gates pass
  with zero dependencies. Control/allocation encoding and complete audio packet
  assembly remain work; these spectral results do not establish encoder quality.

## [0.1.51] - 2026-11-02

- Add atomic fixed-storage CELT mono/stereo band encoding with raw signs,
  forward TF, stereo theta candidates, intensity mixing, inversion and native
  N=2 orientation. Return reconstructed channels, folding basis and collapse mask.
- Share validated band domains and TF reconstruction between encoder and decoder.
  Keep public PVQ and mono partition norm bounds; bound band inputs at squared
  norm 5/4 and private mixed-vector search at 3.
- Match 22,176 complete original band encodings across 924 configurations and
  5,376 intermediate stereo mixing cases. Verify 6,720 PCM-analysis band round
  trips, invalid/late rollback and 23 targeted mutations, including a constant
  error hidden by subsequent quantization in the initial complete-band corpus.
- New/prior optional fixtures reproduce and all seven offline gates pass with
  zero dependencies. Full-spectrum decisions, packet encoding and encoder quality
  comparisons remain subsequent work.

## [0.1.50] - 2026-11-02

- Add safe fixed-storage recursive mono CELT partition encoding: measured theta,
  triangular/time-uniform coding, noise avoidance, split/rebalance budgets, PVQ
  leaves and exact folding/noise reconstruction with atomic writer/shared state.
- Add normalized Q30 atan/positive atan2 helpers. Match74154 independent scalar
  inputs and9440 complete original partition bitstreams/states across286 geometries;
  native/Rust paired decoding agrees, and all64 standard split resolutions pass.
- Retain two minimized valid-budget regressions for the three-bit rebalance reserve
  after a targeted mutation exposed a coverage gap. All twenty mutations, invalid
  domains and late capacity rollback are detected. New and prior partition/math
  corpora reproduce; all seven mandatory offline gates pass with zero dependencies.

TF/stereo/full-spectrum encoding and complete audio packet encoding remain work.

## [0.1.49] - 2026-11-02

- Add safe allocation-free normalized CELT PVQ band encoding with native forward
  spreading, adaptive Q14 headroom, pyramid/greedy pulse selection and sign/tie rules.
- Stage resynthesized Q24 shape/collapse output before atomic CWRS writing; preserve
  input/writer on errors and validate normalized energy/block/gain domains. Reuse
  unchanged decoder reconstruction and support the internal sixteen-block mask.
- Match16368 original encoded bytes/counters/chosen pulses/reconstructions/masks,
  including exact projection boundaries; verify mixed public round trips and
  malformed/resource/lifecycle failures, and detect fourteen targeted mutations.
  New and previous PVQ corpora reproduce; all seven mandatory offline gates pass.

Recursive band/control encoding and complete audio encoding remain in development.

## [0.1.48] - 2026-11-02

- Add allocation-free native CWRS vector ranking and atomic pulse entropy encoding
  to validated codebooks, including the existing zero-pulse extension.
- Validate exact dimensions/L1 sum before ranking; preserve input and complete
  writer state on malformed vectors, capacity and lifecycle failures.
- Match all1834 representable books and13395 original encoder vectors/streams/
  counters. Verify70423 exhaustive small-book inverses, deterministic wide cases,
  boundaries and nine detected mutations. New and prior decoder pulse corpora
  reproduce; all seven mandatory offline checks pass with zero dependencies.

PVQ pulse search, recursive band coding and complete audio encoding remain work.

## [0.1.47] - 2026-11-02

- Add fixed-storage CELT energy encoding with temporal/inter-band coarse prediction,
  inter/intra two-pass RDO, loss bias, decay/scarcity/LFE policy and fine/final bits.
- Preserve energy/residual/delayed state and the entropy writer together on errors;
  validate physical Q24 targets/options/budgets and canonicalize inactive residuals.
- Match388 original full energy bitstreams and per-stage state/range/accounting,
  including212 intra/176 inter choices. Retain both reference sides of11 artificial
  scarce-prefix history differences; separately verify96 normal-budget matching
  encoder/decoder sequences, late rollback and sixteen detected mutations.
- Optional energy and previous entropy/decoder-energy evidence reproduce; all seven
  mandatory offline checks pass without foreign dependencies or unsafe code.

Complete compressed audio encoding remains under development.

## [0.1.46] - 2026-11-02

- Add safe fixed-storage CELT band analysis: native amplitudes, normalized Q24
  vectors and mean-adjusted Q24 log energies for all sizes and mono/stereo.
- Match canonical headroom, refined square root/reciprocal, maximum-coefficient
  floor, low-energy protection and effective-end tails; expose bounded views.
  Reject unrepresentable active arithmetic explicitly without changing input.
- Add refined wide square-root and normalized Q31 reciprocal APIs while retaining
  old scalar results. Match640 independent original complete band results and
  104711 refined scalar inputs; verify PCM composition, invalid/overflow domains
  and fourteen detected mutations. New and previous scalar/analysis evidence
  reproduce; all seven mandatory offline gates pass with zero dependencies.

Complete audio quantization and packet encoding remain under development.

## [0.1.45] - 2026-11-02

- Add fixed-storage CELT frame analysis over validated planar channel contexts,
  orchestrating long or interleaved short MDCTs and exposing bounded coded spectra.
- Match native stereo-to-mono separate-half rounding and low-rate spectral gain/
  cutoff at all four durations, five input rates and three channel layouts.
  Preserve input and return no partial output on invalid contexts.
- Match1080 independent original complete spectra, verify PCM/context/transform
  composition and malformed domains, and detect ten targeted mutations. New and
  previous input/transform corpora reproduce; all seven mandatory offline gates
  pass with no dependencies or unsafe code.

Transient/pitch-prefilter decisions, band analysis/quantization and complete audio
encoding remain under development.

## [0.1.44] - 2026-11-02

- Add safe allocation-free CELT signed16 input pre-emphasis at all five standard
  rates and four block sizes, with selected mono/stereo interleaved channels.
- Match Q12 conversion, low-rate zero insertion and Q15 filter memory across
  frames/rates; expose typed input rates, memory diagnostics and reset. Preserve
  complete output/state on errors and retain ignored output slots.
- Match512 independent original sample/memory cases, including both native
  fast/general paths, extreme PCM, rate/channel changes and resets. Verify
  duration chunking/channel isolation, detect eleven targeted mutations and
  reproduce optional fixtures. All seven mandatory offline gates pass.

Pitch prefilter/overlap staging, further analysis/quantization and complete audio
encoding remain under development; no Cargo dependency is introduced.

## [0.1.43] - 2026-11-02

- Add safe allocation-free standard CELT forward MDCT analysis for all four
  block sizes and ten valid interleaved size/stride combinations.
- Match original window/folding, pre-rotation/Q16 scale, signal headroom,
  staged FFT downshift and rounded spectral post-rotation using existing tables.
  Preserve input, ignored output slots and complete output on geometry errors.
- Match304 independent original spectra and observed headroom decisions, including
  signed extremes, impulses, random scales and sequential short blocks; cover
  headroom0..7 and detect eleven targeted mutations. Forward and prior inverse
  corpora reproduce; all seven mandatory offline checks pass with zero dependencies.

Complete audio encoding remains under development.

## [0.1.42] - 2026-11-02

- Add a safe fixed-capacity Opus range encoder: CDF/ICDF, probability bits,
  raw suffix bits, uniform integers and clipped classic Laplace values.
- Match native normalization/carry chains, whole/eighth-bit accounting, uniform
  prefix patching, raw-suffix relocation and minimal endpoint termination with
  partial-byte overlap. Expose finalized bytes, idempotent finish and reset.
- Preserve all state/bytes on invalid models, lifecycle, capacity and late write
  failures using a bounded write journal without per-symbol frame copies.
- Match607 independent original encoder cases and10969 intermediate states/full
  final bytes; verify mixed round trips, all Laplace zero-frequency extremes,
  resource rollback and twelve detected mutations. Optional fixtures reproduce;
  all mandatory offline gates pass with zero Cargo dependencies.

Audio analysis, quantization and complete packet encoding remain pending.

## [0.1.41] - 2026-11-02

- Add matched optional Rust/C complete-decoder replay benchmarks over all twelve
  official streams and all ten output configurations, with no Cargo dependency.
- Verify all 120 accepted PCM SHA256 hashes and full PCM/range fingerprints
  before 15 randomized CPU-pinned paired rounds; retain throughput, latency,
  decoder-state sizes and source/compiler/corpus provenance in a versioned report.
- Record every measured loss: Opura takes about 1.54–1.80 times the pinned scalar
  reference time and uses more decoder state. The scalar speed target fails;
  SIMD, other competitors, explicit PLC/FEC speed and encoder quality are unmeasured.
- Add fail-closed report/protocol/correctness fault tests and update stale
  full-codec gate diagnostics. All seven mandatory offline gates pass.

## [0.1.40] - 2026-11-02

- Validate the unified decoder against all120 official RFC8251 combinations:
  12 streams at five output rates and mono/stereo,200750 decoded packets and
  exact nonzero encoded final ranges, with original PCM conformance metrics.
- Add optional bounded vector-file decoding, archive/RFC file-hash verification,
  unchanged external comparator integration and atomic sanitized JSON reports.
- Reproduce complete PCM fingerprints/counts/metrics in two full runs. Prove live
  gates reject a corrupted range and zeroed PCM; add framing/endian/range/output
  and archive/metric/report fault tests. No decoder DSP fix was required.
- Record reproducible conformance evidence and update decoder capability docs;
  all mandatory offline gates and example/tool checks pass without Cargo deps.

Decoder performance validation and encoding remain pending; conformance does
not establish SOTA speed or encoder quality.

## [0.1.39] - 2026-11-02

- Add a transactional fixed-storage unified Opus decoder with shared SILK/CELT
  histories across all standard modes, rates and encoded/output channel layouts.
- Reconstruct both5ms redundant CELT tail directions, shrink primary entropy
  storage, apply native mode resets and2.5ms fades, and retain final-range XOR.
- Orchestrate all-mode explicit/implicit loss/DTX and requested FEC intervals,
  including initial silence, short SILK state advancement, prefix PLC/fallback,
  Hybrid-to-SILK fade-out, resets and native diagnostics.
- Match2440 public libopus PCM/range and independent complete CELT/PLC/context
  snapshots, including stale tails and48-frame/120ms packets. The unified API
  also matches3002 previous CELT/Hybrid loss/FEC cases unchanged.
- Verify512 corrupt mixed-mode sequences, retained deep/late transition failures,
  FEC-prefix rollback and22 detected mutations. Optional corpora reproduce and all
  mandatory offline checks pass.

Official conformance, full decoder performance validation and encoding remain
pending; SOTA performance is not yet established.

## [0.1.38] - 2026-11-01

- Add transactional Hybrid explicit/implicit loss, DTX, initial silence and
  first-frame in-band FEC with exact SILK/CELT concealment and recovery.
- Schedule120..5760 samples in2.5ms steps on the last packet clock; advance full
  SILK10ms state for shorter output and preserve Hybrid high-band noise context.
  Distinguish FEC persistent RNG diagnostics from zero loss final range.
- Fix SILK PLC tiny-gain LTP normalization: preserve original signed16 scale
  truncation and negative coefficients. Accept CNG's positive signed16 noise
  scale domain through32767; match192 additional original PCM/full-state cases.
- Match2130 public libopus PCM/range and independent complete CELT/PLC snapshots,
  including initial FEC, LBRR gaps, irregular losses, channel/bandwidth changes
  and resets. Verify128 corrupt mixed sequences, duration rollback and FEC
  first-frame behavior; thirteen targeted mutations are caught.
- Existing corpora remain unchanged and reproduce; all mandatory offline gates pass.

CELT transition tails, unified cross-mode orchestration, standalone SILK implicit
DTX, full conformance and encoding remain pending.

## [0.1.37] - 2026-11-01

- Add transactional fixed-storage normal Hybrid decoding: shared SILK/CELT
  entropy, 16kHz low band, SWB/FB high bands and exact signed16 accumulation.
- Support10/20ms frames, mono/stereo encoded and output channels, all five PCM
  rates, normal LBRR, padding and multi-frame packets up to120ms.
- Reuse positioned layer kernels without changing standalone SILK/CELT evidence;
  retain CELT start-band diagnostics and history clearing/reset semantics.
- Match960 public libopus PCM/range results and independent complete CELT state
  snapshots, including clipping and channel/bandwidth changes. Verify packet
  versus individual-frame equivalence, deep/late failure rollback,512 corrupt
  packets and twelve detected mutations. Optional corpora reproduce; all offline gates pass.

Hybrid loss/FEC/DTX, transition tails, unified cross-mode orchestration, full
conformance and encoding remain pending.

## [0.1.36] - 2026-11-01

- Add transactional CELT periodic/noise PLC, explicit48kHz-clock loss scheduling,
  initial silence, empty/one-byte DTX frames and normal recovery across mixed
  frame/channel/bandwidth and multi-frame packet contexts.
- Preserve full signal/future/LPC history; connect pitch/excitation decay, bounded
  synthesis stabilization/energy guards, negative comb prefilter/TDAC, noise RNG
  and background/recovery updates. Public range is zero for loss; RNG persists.
- Expose read-only concealment diagnostics and retain native reset band-limit
  semantics. Every error rolls back the complete frame/packet/loss operation.
- Match public libopus PCM, range and complete state on872 scenarios, including
  long bursts, mixed implicit loss, resets and210x120ms stress reaching caps.
- Match208 original bandwidth and232 energy-guard cases; verify256 adversarial
  normal/loss/recovery sequences, guard/duration rollback and a retained late
  corrupt-uniform regression. Fifteen mutations are caught.
- Existing normal corpus remains unchanged; all optional corpora reproduce and
  mandatory offline gates pass.

Hybrid, cross-mode transitions, full conformance and encoding remain pending.
Standalone SILK implicit DTX also awaits unified packet orchestration.

## [0.1.35] - 2026-11-01

- Add fixed-storage classic CELT PLC pitch analysis from2048 mono/stereo internal
  history samples: factor2 lowpass/LPC4/FIR5 whitening, coarse quarter-rate and
  fine candidate correlation, normalized pitch ranking and pseudo interpolation.
- Validate decoder signal bounds and return both the period and full1024-sample
  whitened signal without mutating input or allocating.
- Match all samples and periods on2202 original cases covering every target
  period100..720, channel combinations, phase/noise/silence and amplitude bounds.
- Domain/silence checks and seven meaningful mutations pass; optional corpus
  reproduces and all mandatory offline gates pass.

Full CELT periodic/noise loss synthesis and recovery, Hybrid, conformance and
encoding remain pending; normal CELT packets already decode to final PCM.

## [0.1.34] - 2026-11-01

- Add bounded allocation-free CELT autocorrelation/window/scaling, fast64 LPC
  analysis and Q25→Q12 coefficient fitting, Q12 FIR and canonical in-place IIR
  primitives for upcoming pitch search and periodic loss concealment.
- Validate supported geometry/correlation/reflection domains and preserve all
  outputs on errors; retain original FIR saturation and IIR memory semantics.
- Match 1601 correlations, 2680 LPC analyses, 832 FIR and 336 IIR original cases.
  Add power-of-two estimator boundaries and clustered positive covariance cases
  after mutation testing exposed missing scale/fitting coverage.
- Independent zero-filter/correlation bounds, invalid/unstable rollback and seven
  arithmetic mutations pass; corpus reproduces and mandatory offline gates pass.

Normal CELT PCM remains supported; periodic/noise PLC, Hybrid and complete Opus
conformance/encoding are still pending.

## [0.1.33] - 2026-11-01

- Add stateful allocation-free normal CELT-only frame/packet decoding to final
  mono/stereo signed-16 PCM at 8/12/16/24/48kHz, including silence, all encoded
  frame sizes/bandwidths and multi-frame packets up to 48 frames/120ms.
- Connect every verified entropy/spectral/anti-collapse/synthesis stage, preserve
  mono/stereo energy/log/background/RNG and synthesis histories, and apply
  current reference mono-API inversion policy. Frame/packet errors roll back.
- Match public pinned opus_decode PCM/final range and complete independently
  observed energy/filter/overlap/de-emphasis state on 960 real encoder packets.
- Verify packet/frame equivalence, reset, late-frame rollback, 512 adversarial
  payloads and eight integration mutations. Optional full-reference corpus
  reproduces; all mandatory offline checks pass.

CELT PLC/DTX, Hybrid, cross-mode transitions, full conformance and encoding remain
pending. End-to-end normal CELT evidence does not establish SOTA performance.

## [0.1.32] - 2026-11-01

- Add atomic transient anti-collapse on normalized spectra: bit-depth thresholds,
  current/two prior energy attenuation, mono history max, 20ms amplitude scaling,
  interleaved per-block noise and legacy vector renormalization.
- Validate matching transient 10/20ms allocation/reservation and bounded Q24
  energies; preserve the complete spectrum on errors and retain the stored seed
  because anti-collapse consumes a local RNG copy.
- Match 1850 original complete-stage cases through reserved control/final energy
  and optional anti-collapse, including 395 enabled restorations (135 changing
  coefficients); reject 166 corrupt streams across 2016 total cases.
- Check invalid context/energy rollback and seven targeted mutations; all prior
  spectral evidence regenerates unchanged and mandatory offline gates pass.

Stateful compressed CELT PCM integration, loss recovery and Hybrid remain pending.

## [0.1.31] - 2026-11-01

- Assemble complete normalized CELT mono/stereo spectra from immutable
  allocation/TF contexts, with all-band budget carry, folding-source/mask
  selection, Hybrid fold tails and dual-to-intensity folding conversion.
- Return fixed-storage Q24 spectra, channel masks and folding seed; inactive
  channels/bins stay zero and failures preserve entropy. Accept real CELT/Hybrid
  starts 0/17 and validate the TF/frame context and physical payload budget.
- Match selected spectra, masks, seed and entropy on 3779 original full-stage
  sequences from coarse energy through all bands, including 1008 prefix cases;
  reject 253 corrupt streams atomically across 4032 total cases.
- Verify invalid TF/start/payload/cursor rollback and eight mutations. Reference
  regeneration and all mandatory offline gates pass.

Anti-collapse, frame energy history and compressed CELT PCM integration remain
pending; Hybrid and complete Opus conformance still require further work.

## [0.1.30] - 2026-11-01

- Add transactional mono/stereo CELT band decoding over recursive partitions:
  one-coordinate signs, TF Haar/Hadamard reorganization and collapse masks,
  scaled folding output, stereo theta/intensity/inversion, N2 orthogonal-side
  signs and general stereo allocation/merge.
- Preserve the existing partition API and fixed-storage bounds; validate TF,
  initial blocks, stereo gain/intensity and fill domains before decoding.
- Match both channels, folding output, masks, budget/seed and entropy on 15680
  original band cases covering every frame/band/TF/channel context; reject 28
  corrupt uniform cases atomically. Domain rollback and nine mutations pass.
- Break a correlated oracle parameter combination exposed by mutation testing
  to verify inversion override independently. Fixtures/tables reproduce and all
  mandatory offline gates pass.

All-band folding/budget orchestration, anti-collapse and complete CELT/Hybrid
packet decoding remain required before full Opus conformance.

## [0.1.29] - 2026-11-01

- Add transactional classic CELT mono partition decoding: standard pulse-cache
  lookup, uniform/triangular theta, recursive gains/transient allocation and
  rebalancing, pulse budget fallback, deterministic noise/folding and masks.
- Support sixteen blocks internally with a u16 partition mask, preserving the
  existing public eight-block PVQ contract. Bound recursion to four split levels.
- Match complete coordinates, masks, seed/budget and entropy on 9409 original
  cases covering every valid standard geometry and fresh/positioned cursors;
  reject 29 corrupt uniform cases atomically. Check malformed domains, negative
  budget continuation and eight targeted mutations.
- Reference tables/fixtures reproduce; all mandatory offline checks pass.

Time-frequency/stereo band orchestration and complete CELT/Hybrid remain pending.

## [0.1.28] - 2026-11-01

- Add bounded allocation-free CELT coded-theta gains/log2tan, Haar and
  Hadamard/interleave operations, legacy folded/noise vector normalization and
  normalized stereo merge. Invalid geometry/gain/vector domains preserve inputs.
- Match 16259 coded angles, 68644 positive log-ratio pairs, 1531 vector transforms,
  420 normalizations and 400 stereo merges from unchanged pinned reference code.
- Check permutation inverses, invalid-input rollback and seven arithmetic/order
  mutations. Constants/fixtures reproduce and all mandatory offline gates pass.

Recursive spectral entropy decoding and full CELT/Hybrid remain incomplete.

## [0.1.27] - 2026-11-01

- Add bounded transactional classic CELT allocation: caps, dynamic boosts, trim,
  anti-collapse reservation, interpolated curves, skip/intensity/dual fields,
  PVQ eighth-bit budgets, fine-energy bits/priorities and excess balancing.
- Preserve encoded frame/layout/transient context in immutable allocation results
  and expose fine arrays directly to existing energy stages.
- Match every result/entropy field and following fine-energy output on 16296
  original cases, including every nonempty band interval at every LM/channel.
- Verify budget conservation, malformed rollback and 512 deterministic adversarial
  cases; six mutations are detected. Tables/fixtures reproduce and all gates pass.

Recursive CELT band/shape decoding is still required to feed compressed audio
through the verified synthesis backend; full Opus remains incomplete.

## [0.1.26] - 2026-11-01

- Add transactional CELT control prefix decoding with typed silence/postfilter/
  transient/intra fields, positioned start0/17 contexts and CELT Packet wrapper.
- Preserve cached-tell decisions and physical silence accounting; add exact
  differential TF/select reservation and spreading models/budget fallbacks.
- Match fields and range/tell/tell_frac on 3360 original prefix/coarse/TF/spread
  sequences and 1824 standalone TF boundary cases, including all postfilter
  octave/pitch/gain/tap boundaries and 2..1275-byte payload budgets.
- Exhaust every two-byte prefix at all frame sizes/start contexts; validate
  malformed rollback and silence accounting overflow. Six mutations are caught;
  fixtures/tables reproduce and all mandatory offline gates pass.

These control stages still need CELT allocation and recursive spectral decoding
before compressed frames can use the verified PCM backend.

## [0.1.25] - 2026-11-01

- Add atomic fixed-rate/channel `CeltSynthesis` from decoded normalized spectra
  and Q24 energies through scaling, mono/stereo projection, long/transient MDCT,
  synthesis clipping, comb postfilter and final de-emphasis/decimation to PCM.
- Preserve independent output-channel overlap/filter histories across encoded
  channel changes; match separate-half downmix rounding and short-block stride.
- Match final PCM and all overlap/postfilter/de-emphasis state on 480 original
  complete-backend cases across every rate/frame/channel/transient combination.
- Verify malformed rollback/reset and five integration mutations. Oracle data
  reproduces byte-identically; all mandatory offline checks pass.

Input is already decoded spectra. CELT entropy/allocation/recursive band decoding,
loss concealment, Hybrid and complete Opus conformance remain incomplete.

## [0.1.24] - 2026-11-01

- Add bounded per-channel classic CELT comb postfilter with validated periods,
  Q15 gains/tapsets, processed history and old/current parameter generations.
- Match constant/windowed in-place arithmetic, distinct transition/constant
  biases, clipping and 120-sample versus long-frame parameter scheduling.
- Match every output, all 1026 history entries and six parameter fields on 576
  chained original cases across tapsets, gains, periods, sizes, resets/extremes.
- Verify delayed short parameters, disable-tail passthrough, malformed rollback,
  five detected mutations and reproducible fixtures. All offline gates pass.

End-to-end CELT synthesis integration, allocation/recursive bands and full Opus remain incomplete.

## [0.1.23] - 2026-11-01

- Add fixed-storage CELT spectral denormalization using Q24 band/mean energies,
  reference mantissa rounding/gain caps, selected bands, silence and rate cutoff.
- Add canonical per-channel output de-emphasis and signed-16 PCM conversion at
  8/12/16/24/48 kHz. Filter every internal sample before decimation, retain both
  memories across layout/rate changes and validate geometry before mutation.
- Match every scaled spectrum on 640 original cases and PCM/memories on 384
  chained de-emphasis cases across all frame/rate/channel combinations.
- Check malformed rollback/reset, five detected mutations and byte-identical
  table/fixture regeneration; all mandatory offline checks pass.

Comb postfilter, CELT recursive bands/allocation and complete packet output remain incomplete.

## [0.1.22] - 2026-11-01

- Add per-channel inverse CELT MDCT and standard 120-sample TDAC window with
  persistent 60-sample overlap across 120/240/480/960-sample block changes.
- Support validated strided spectra for interleaved short blocks; preserve exact
  headroom selection, rotations, FFT scaling and pathological signed wrapping.
  Return internal signed-32 signal, before postfilter/de-emphasis/final PCM.
- Match all 198360 time samples and overlap values in 398 original chained cases,
  including every size/stride geometry, resets, silence, impulses and extremes.
- Verify invalid-input rollback/reset, five detected mutations and byte-identical
  table/fixture regeneration. All mandatory offline checks pass.

CELT allocation/recursive bands, postfiltering and full packet decoding remain incomplete.

## [0.1.21] - 2026-11-01

- Add allocation-free standard CELT complex FFT plans for 60/120/240/480 points,
  with exact radix 2/3/4/5 arithmetic, bit reversal and shared Q15 twiddles.
- Expose forward transforms with 0..31 per-stage downshifts, canonical scaled
  forward and unscaled inverse wrappers. Validate buffers before any write and
  preserve output tails; intentional reference wrapping is explicit.
- Match every complex output on 1632 original FFT results across all plans,
  shifts, impulses, DC/Nyquist tones, full-range extremes and deterministic input.
  Independently check order/sign by DFT and 1/N scaling with exact impulses.
- Detect five mutations; extracted plans and fixtures regenerate identically.
  All mandatory offline gates pass; transform synthesis still needs inverse MDCT.

## [0.1.20] - 2026-11-01

- Add atomic normalized CELT PVQ bands from positive-pulse codebooks: Q31 gain,
  high-precision Q24 normalization, all spreading modes and 1/2/4/8-block masks.
- Implement refined Q31-input/Q29-output reciprocal square root with explicit
  normalized-domain validation and reference Newton arithmetic.
- Match every coordinate, mask and entropy diagnostic in 28464 original
  alg_unquant results across 4116 streams; match 196608 refined-root inputs.
- Verify invalid gains/geometry and raw-bit rollback; five mutations detect root
  refinement, rotation order/rounding/stride coefficients and collapse mask.
  Reference corpus reproduces and all required offline checks pass.

Recursive CELT bands, allocation, transforms and complete Opus decoding remain incomplete.

## [0.1.19] - 2026-11-01

- Add fixed-storage CELT algebraic pulse codebooks with compile-time symmetric U
  counts, checked 32-bit cardinality and exact CWRS index/entropy decoding.
- Support 2..176 coordinates and 0..128 pulses when representable; zero pulses
  consumes no bits. Expose pulse vectors and exact squared norms.
- Match all 22575 configuration outcomes, 70423 exhaustive small-codebook indices,
  13401 boundary/random vectors and 566 entropy steps across 73 streams against
  original generic cwrs.c; independently verify cardinalities combinatorially.
- Verify invalid indices/geometries, raw-suffix rollback and pulse invariants;
  four targeted mutations fail. Fixtures reproduce and all mandatory gates pass.

CELT band normalization/rotation, allocation and transforms remain incomplete.

## [0.1.18] - 2026-11-01

- Add transactional CELT Laplace range symbols and persistent Q24 band-energy
  reconstruction for one/two channels, all four frame sizes and selected bands.
- Decode separate coarse, fine and final refinements, with exact budget
  fallbacks, priority ordering, inter-frame/inter-band prediction and clamps.
  Preserve reference signed-32 truncation before coarse energy saturation.
- Match all 42 energies and range/tell/tell_frac after each stage in 768 chained
  reference frames, plus 2688 standalone Laplace symbols. Exercise every two-byte
  prefix with three boundary Laplace models and verify late refinement rollback.
- Five mutations are detected; model tables and fixtures regenerate identically.
  All mandatory offline checks pass; no third-party dependencies are added.

CELT shape decoding/transforms, Hybrid and full Opus orchestration remain future work.

## [0.1.17] - 2026-11-01

- Start CELT with allocation-free fixed-point scalar math: exact integer square
  root, approximate square root/normalized reciprocal square root, periodic
  cosine, log2/exp2, reciprocal and fractional Q29/saturated-Q31 division.
- Expose Q formats, reference clamp/sentinel behavior and typed domain errors.
- Match 638975 exhaustively hashed results and 29164 scalar reference cases,
  including full normalized reciprocal-root, exponent and two cosine periods.
  Independently bracket every 16-bit square and 65536 deterministic wide inputs.
- Detect five rounding/refinement/normalization mutations; regenerate fixtures
  byte-identically and pass all mandatory offline checks without dependencies.

These are scalar primitives; CELT/Hybrid audio decoding remains incomplete.

## [0.1.16] - 2026-11-01

- Add bounded, atomic `SilkDecoder` for encoded mono/stereo and mono/interleaved
  stereo final PCM at every standard output rate, including loss and in-band FEC.
- Integrate stereo/LBRR field ordering, mid-only side suppression and restart,
  channel/rate transitions and separate left/right resampler histories.
- Match every PCM sample and public final range on 1738 normal/FEC/loss results
  plus 30 transactional transition rejections. Cover all 64 mid/side LBRR mask
  pairs per internal rate/output layout, all 15 rate pairs and 120 ms output.
- Retain 260 earlier mono oracle cases; verify reset, malformed requests, late
  internal/Opus-frame rollback and 1024 adversarial calls. Six targeted mutations
  fail the oracle tests; the new reference corpus regenerates byte-identically.
- Run all mandatory offline debug/release, lint, docs and dependency/tool checks.

CELT/Hybrid, implicit DTX and transition reconstruction remain incomplete.
Full Opus conformance and SOTA performance are not yet established.

## [0.1.15] - 2026-11-01

- Add transactional SILK stereo predictor and optional mid-only entropy decoding,
  with typed optimized Q13 predictions and strict primitive budget checks.
- Reconstruct buffered internal-rate left/right PCM from mid/side frames using
  reference 8 ms interpolation, signed-16 predictor-difference arithmetic,
  saturation and two-sample raw histories. Preserve history across rate changes.
- Match all 5625 quantizer pairs, mid-only flags and entropy counters against
  upstream; match both PCM channels and persistent reconstruction state on 192
  independent extreme, correlated/opposed, quiet, clipping and reset/rate cases.
- Verify short-prefix/geometry rollback and reset; mutations detect omitted
  predictor subtraction, incorrect interpolation arithmetic and predicted-side
  history retention. Reference tables and fixtures regenerate byte-identically.

Stereo packet/channel scheduling is not yet integrated. Existing final-rate mono
SILK remains supported; full Opus conformance/performance remain unestablished.

## [0.1.14] - 2026-11-01

- Add `MonoOutputDecoder` for final mono SILK PCM at 8/12/16/24/48 kHz,
  composing normal/loss/FEC decoding, one-sample mono buffering and resampling.
- Preserve the buffered channel sample across internal-rate switches while
  reinitializing the resampler; commit all packet/delay/filter history atomically.
- Match every final PCM sample and public OPUS_GET_FINAL_RANGE on 255 successful
  public opus_decode results across all 15 rate pairs, frame durations, multi-frame
  120 ms packets, explicit loss, FEC/recovery and input-rate changes. Five real
  primary transitions reject without state updates.
- Verify reset, invalid requests, late FEC resource rollback and unchanged output
  histories on errors. Mutations detect missing mono delay and per-packet filter
  resets; the independent public-output fixture mode regenerates byte-identically.

This delivers final-rate PCM for the supported mono SILK subset. Stereo,
CELT/Hybrid, transition reconstruction and implicit DTX remain incomplete;
full Opus conformance and competitive performance are not established.

## [0.1.13] - 2026-11-01

- Add a fixed-storage SILK decoder resampler for all 15 combinations of
  8/12/16 kHz internal input and 8/12/16/24/48 kHz output rates.
- Match reference delay compensation, three-section allpass x2 upsampling,
  fractional FIR interpolation and AR/FIR downsampling with rounded-up Q16 ratios.
- Validate whole-ms input bounds (1–120 ms) and output capacity before modifying
  state or samples; retain filter history across calls and reset on rate changes.
- Match PCM and every active filter/delay/configuration field on 360 independent
  upstream cases, including impulses, DC extremes, alternating limits, random
  input, clipping, resets and rate transitions. Verify chunking invariance and
  invalid-buffer rollback; mutations detect allpass, FIR mirror and delay errors.

This component does not yet add Opus mono/stereo buffering or integrate output
resampling into packet decoding. Full Opus compatibility remains unestablished.

## [0.1.12] - 2026-11-01

- Add transactional mono in-band FEC recovery with `MonoDecoder::decode_fec`,
  recovering the preceding interval represented by the first Opus frame and
  leaving primary audio for a subsequent normal decode of the same packet.
- Decode LBRR in temporal order, using conditional coding only for adjacent
  redundant frames and PLC/CNG for gaps. Preserve reference gain history for
  FEC gaps, which differs from explicit packet-loss gain reset.
- Verify 111 FEC results and 215 subsequent/regular PCM results against the
  complete pinned upstream decoder, plus a rejected real transition and a
  late over-budget FEC regression; cover all LBRR masks, rates and durations.
- Test multi-frame first-frame semantics, reset, unsupported/malformed input and
  atomic failure recovery. Mutations detect gapped-context coding, premature
  commits and incorrect FEC gain reset. Earlier packet/loss fixtures are unchanged.

Output remains before mono buffering/resampling. CELT/Hybrid, stereo and implicit
DTX handling remain future work; full-codec conformance/performance are unestablished.

## [0.1.11] - 2026-11-01

- Add `MonoDecoder::conceal(duration_ms)` for explicit missing intervals of
  10–120 ms in 10 ms steps at the last successfully decoded SILK rate.
- Match upstream loss scheduling after 10 ms and longer Opus frames; apply
  channel PLC/CNG/recovery bookkeeping and report final range zero. Invalid
  durations and missing stream history preserve all state.
- Verify 288 real encoded packet-stream steps, including 120 scheduled losses
  covering every supported duration, burst loss, silence, FEC-bearing packets
  and recovery, against completed original decode_frame PCM and public final range.
- Retain the existing default raw-core fixture output unchanged; add an optional
  loss oracle mode, transactional error/reset tests and scheduling/history mutations.

PCM still precedes mono buffering/resampling. Empty/one-byte DTX packets require
explicit handling, and FEC recovery, stereo and CELT/Hybrid remain future work.

## [0.1.10] - 2026-11-01

- Add bounded internal-channel loss concealment with reference fixed-point
  PLC: random/voiced excitation, gain attenuation, pitch drift, rewhitening and
  LPC synthesis. Preserve shared prediction histories between losses and good frames.
- Mix CNG and apply recovery energy glue in upstream order; clear gain clamping
  after loss and apply post-loss LPC bandwidth expansion and voiced-to-unvoiced
  recovery prediction. Expose transactional `ChannelState::conceal` for 10/20 ms.
- Match all PCM samples, entropy counters, gain indices, shared synthesis history
  and complete PLC state against 240 pinned original decode_frame steps, including
  cold loss, bursts, recovery, rates/durations, inactive/unvoiced/voiced and clipping.
- Verify long loss bursts, reset and failed-recovery rollback; detect omitted PLC
  expansion, recovery fade, post-loss filter expansion and recovery prediction
  with targeted mutations. Reference regeneration is byte-identical.

This is an internal channel loss API. Packet loss scheduling, FEC recovery,
mono buffering/resampling, stereo and CELT/Hybrid still require integration.

## [0.1.9] - 2026-11-01

- Add fixed-storage SILK comfort-noise estimation from decoded inactive frames
  and CNG mixing into 10/20 ms concealed PCM at 8/12/16 kHz.
- Match reference spectral/gain smoothing, excitation-bank selection, wrapping
  PRNG, LPC filtering, PLC noise-energy compensation and signed saturation.
- Include CNG estimates in channel state transactions without changing existing
  loss-free mono PCM/final range; validate geometry and PLC noise context before
  changing state or output. Reset and rate changes follow upstream semantics.
- Compare every PCM sample and retained CNG state field on 192 independent
  upstream cases, covering cold loss, burst losses/recovery, rates/durations,
  gain branches and clipping. Typed tests verify error transactions and reset;
  targeted mutations detect mask, noise compensation and filter-reset errors.

CNG is a component for loss handling; a complete PLC implementation and packet
loss API are still required. Mono output remains before buffering/resampling;
stereo and CELT/Hybrid remain future work.

## [0.1.8] - 2026-11-01

- Decode loss-free mono SILK-only Opus packets to raw internal-rate PCM at
  8/12/16 kHz, including 10/20/40/60 ms Opus frames, packet codes 0–3, padding
  and multi-frame packets up to 120 ms with fixed output storage.
- Consume LBRR before regular audio with correct adjacent-frame conditional
  coding, isolated from regular gain, spectral and synthesis histories.
- Commit all packet histories and output atomically. Reject unsupported stereo,
  CELT/Hybrid, loss/DTX frames and CELT transition tails with typed errors.
- Handle legitimately stripped trailing zero bytes with a bounded packet-only
  virtual entropy budget; preserve strict public primitive budget contracts.
- Verify raw PCM samples and public final range against the pinned fixed-point
  libopus decoder on 222 real encoded periodic/noise, silence and FEC packets,
  including repacketized 120 ms inputs. Another 48 synthetic symbol packets cover
  all LBRR masks; six real transition packets verify unsupported-input rollback.
- Add rollback/retry, reset, malformed-input, resource-boundary and zero-trimming
  regressions and targeted mutations for gapped LBRR, premature commits and
  missing zero extension, with a reproducible external-reference generator.

Output precedes Opus mono delay buffering and resampling. Loss recovery,
CNG/PLC, stereo and CELT/Hybrid decoding remain unimplemented. Full Opus
conformance and competitive PCM performance are not established.

## [0.1.7] - 2026-11-01

- Add transactional single-channel SILK decoding from signal-type symbols through
  gains, NLSF, pitch/LTP, excitation and raw internal-rate PCM synthesis.
- Commit entropy and all channel histories together; failed frames leave them
  unchanged. Support reset, 10/20 ms duration changes and 8/12/16 kHz rate changes,
  including spectral-history reset for 8↔12 kHz and conditional-start rejection.
- Match every PCM sample and final range/tell/tell_frac across 144 consecutive
  synthetic upstream-reference frames, including all activity/coding modes.
- Verify rollback at each entropy stage, retry recovery, reset and rate changes;
  detect missing spectral resets and premature commits with targeted mutations.

This is a positioned-cursor channel API, before CNG/PLC, stereo and resampling;
complete Opus packet-to-PCM decoding and full-codec performance remain unverified.

## [0.1.6] - 2026-10-29

- Add loss-free one-channel SILK PCM synthesis at 8/12/16 kHz for 10/20 ms
  internal frames, consuming existing decoded gains, NLSFs, pitch and excitation.
- Apply reference fixed-point gain adjustment, LTP rewhitening/prediction, LPC
  filtering and signed-16 saturation with persistent histories and reset support.
- Reject incompatible geometry or voiced pitch before mutating state; define
  rate-switch history clearing and preserve allocation-free, safe Rust behavior.
- Match every PCM sample and persistent LPC/PCM history value across 192 pinned
  Opus 1.6.1 reference frames; exercise typed integration and state rollback.

Full frame orchestration, stereo, CNG/PLC, resampling and CELT/Hybrid mixing are
still absent. This milestone does not establish full-codec conformance or speed.

## [0.1.5] - 2026-10-29

- Share the validated one/two-frame paths across all packet codes, including
  padding; avoid the general length buffer for these cases.
- Build multi-frame CBR views without an intermediate length array. Store VBR
  lengths in u16 only after validating the final frame size; retain usize sums.
- Add eight-frame inline storage to reduce initialization/copying for shorter
  multi-frame packets, preserving the public API, no allocation and safe Rust.
- Expand exact-span regression coverage to all counts 1–48, CBR/VBR, padding
  and frame-length boundaries, including oversized lengths that could truncate.
- Record fresh paired benchmarks against the unchanged corpus and thresholds.
  Full packet and full-codec superiority remain unachieved; see the results.

## [0.1.4] - 2026-10-29

- Reduce packet parsing overhead with one/two-frame storage, short-form fast
  paths and cross-crate inlining, preserving the public API and validation.
- Add a machine-readable registry of 25 pinned competitor repositories and an
  offline packet-framing benchmark against an explicitly supplied libopus.
- Record paired distributions, corpus and binary hashes, compiler/CPU details,
  and before/after measurements; keep full-codec comparisons visibly missing.
- Add a competitive gate requiring a 10% aggregate win and no >5% critical-case
  regression at the bootstrap upper bound, plus negative tests for false wins.
  The current full packet gate still FAILS; PCM superiority is unestablished.
- Preserve local-only development until the owner resumes GitHub activity.

## [0.1.3] - 2026-10-28

- Decode SILK pulse rate levels, bounded LSB escapes, shell trees and signs,
  including the padded 120-sample internal frame.
- Reconstruct deterministic Q14 excitation from pulses, quantization offset
  and seed; reject invalid context and preserve entropy on failure.
- Match upstream pulse/excitation fingerprints and entropy counters across
  1584 rate/duration/signal/offset/seed cases; test malformed inputs and four
  targeted arithmetic, padding and state mutations.
- Document 25 GitHub comparison baselines with pinned evidence and measurable
  compatibility, speed, memory, audio-quality and usability targets. No
  comparative performance or overall leadership claim is made.

LTP/LPC synthesis, complete frame orchestration and PCM output remain unimplemented.

## [0.1.2] - 2026-10-28

- Decode SILK pitch, five-tap LTP coefficients, scaling and excitation seed for
  8/12/16 kHz and 10/20 ms internal frames.
- Support conditional lag history, absolute lag escapes and independent coding
  with or without transmitted LTP scaling. Reset history on rate changes.
- Preserve entropy and pitch history together on errors; clamp reconstructed
  sample lags and bound malformed conditional chains.
- Match 3618 upstream reference steps, covering every pitch contour and LTP
  codebook entry; exercise all two-byte inputs across 18 configurations.

Excitation decoding, post-loss processing and PCM synthesis remain unimplemented.

## [0.1.1] - 2026-10-28

- Add fixed-point NLSF-to-LPC conversion for both SILK filter orders and frame
  halves, including coefficient fitting and bounded stability correction.
- Expose prediction filters through `FrameNlsf::to_lpc()` and `FrameLpc`.
- Verify exact coefficients and inverse prediction gains against 2281 spectra
  from Opus 1.6.1, plus integration with the NLSF decoding reference sequences.
- Preserve zero dependencies and allocation-free decoder primitives.

PCM synthesis and post-loss bandwidth expansion remain unimplemented.

## [0.1.0] - 2026-10-28

Initial development baseline, not yet an audio decoder.

- Parse Opus packet framing and expose borrowed frames and TOC metadata.
- Decode range-coded symbols, raw bits and bounded unsigned integers.
- Decode SILK VAD/LBRR headers, signal types and subframe gains.
- Decode and reconstruct SILK NLSF spectra for orders 10 and 16, including
  stabilization, interpolation and transactional history updates.
- Validate primitives against pinned Opus 1.6.1 reference fixtures, exhaustive
  short-input cases and targeted arithmetic/state mutations.
- Enforce pure Rust, zero Cargo dependencies, no unsafe code and Rust 1.87.
- Add offline tests, formatting, Clippy, rustdoc and dependency checks to CI.

PCM synthesis, full SILK/CELT decoding and encoding are not implemented.
