# Packet-local PCM output

The0.2.2 decoder writes normal frames and PLC/FEC recovery directly into bounded
private views of its staged packet result. The public `DecodedPcm` type, fixed
maximum120ms capacity, signed16 output, geometry, range and owner layout remain
unchanged. No unsafe code, heap allocation or Cargo dependency is introduced.

Previously the private frame, normal and loss paths returned another complete
public PCM container before copying its active samples into the packet. The new
private view carries a mutable slice and its written length. Each packet frame
gets a fresh tail view, so offsets used by Hybrid accumulation, redundant tails,
mode crossfades and gain remain local to that frame. PLC subframes similarly
write successive tails and apply gain only to the new subframe. FEC first writes
its PLC prefix and then a fresh view for the recovered frame.

An explicit5ms signed16 stereo scratch holds transition PLC (at most480samples
at48kHz). The earlier fractional-SILK concealment behavior is preserved: state
may advance10ms while returning the required prefix. Gain still applies before
transition mixing, including gain on recursive transition PLC, as required by
the native path. Applying it once to the completed packet would change audio.

The enclosing public decoder retains its staged owner clone. A private render
error may have written into the local unpublished PCM object; it never exposes
partial PCM or commits decoder histories. Appends use checked length arithmetic
and checked slices; successful child lengths are bounded by their parent tail.
The final public length is published only after all frames succeed.

## Native coverage

The captured corpus contains70 independent rate/channel/gain profiles and1680
operations:1330normal,210PLC and140FEC.420normal operations are long multi-frame
packets;70resets retain gain. Five output rates, mono/stereo and signed Q8 gains
0,1536,-1536,-1,1,32767,-32768 are independently enumerated in the Rust test.
Normal packets include40/60/120ms sequences, both directions of SILK/Hybrid/CELT
changes, and prefixed120ms FEC. The encoder packets carry actual native LBRR.

The unchanged original Opus1.6.1 reference at
`22244de5a79bd1d6d623c32e72bf1954b56235be` generates compressed input and expected
decoded PCM/ranges through its public APIs. There are no native Q24 patches in
this oracle. The prior0.2.1 and current candidate both pass the exact same
captured corpus. Tests check sample counts, ranges, PCM fingerprints, durations,
reset gain, the full case set and operation totals.

```sh
python3 tools/decoder_pcm_fixtures.py /path/to/opus /new/path/pcm.txt
cargo test --offline --locked --test decoder_pcm --test opus_decoder
```

Only successful new oracle output is accepted. Ordinary tests use captured data
and never require native code. Existing native transition/loss/FEC snapshots,
multistream signed gain, malformed-packet boundedness, whole-owner late failure
and file-gain regressions also pass. Four deliberate source mutations are
caught by running tests: wrong packet-frame offset, gain applied to earlier PLC
subframes again, undersized transition scratch, and history commit before a
later frame fails. Restored tests pass.

Final-source [correctness evidence](results/decoder-pcm-0.2.2-correctness.json)
records all seven passing offline gates (172 library/41 Python tests), detected
source mutations and exact fixture/source hashes. The complete
[120-case conformance report](results/decoder-pcm-0.2.2-conformance.json) passes
all encoded final ranges and retains every0.2.1 PCM SHA256. Reports explicitly
record their pre-commit provenance. Final public before/after timing and release
finalization remain pending. The historical0.2.0 sparse timer profile supplied a lead about
copying; it does not measure the current copy fraction, peak stack or memory.
A7-round48kHz stereo pilot suggests lower codec time but is not retained final
performance evidence. No native-competitor, encoder-speed or SOTA claim follows.
