# CPU profiles of the stable v0.2.5 decoder

On the AMD Ryzen AI 9 HX 370, the largest flat CPU sample count for Opura in
both runs belonged to the system copy routine. The following table counts
samples with that routine at the top of the stack. Its immediate callers
include codec state cloning, CELT pulse-vector reconstruction and the replay
harness. The native reference spent most of its sampled CPU time elsewhere.
[All flat symbol counts and immediate copy callers](results/decoder-cpu-0.2.5.json)

| Output | Implementation | CPU samples | Copy samples | Copy share | Highest-ranked codec function (flat samples) |
| --- | --- | ---: | ---: | ---: | --- |
| 8 kHz mono | Opura | 3,469 | 679 | 19.6% | CELT partition theta (223) |
| 8 kHz mono | libopus | 1,967 | 12 | 0.6% | SILK decode_pulses (185) |
| 48 kHz stereo | Opura | 4,267 | 741 | 17.4% | CELT inverse MDCT synthesis (384) |
| 48 kHz stereo | libopus | 2,823 | 16 | 0.6% | CELT inverse MDCT (286) |

Of the Opura copy samples, the immediate caller was the full decoder clone in
128 mono and 136 stereo samples, and CELT `decode_partition_band_into` in 115
mono and 132 stereo samples. The latter is a candidate for an isolated
experiment that writes decoded pulses into caller-owned storage. Inspect the
copy site before changing the implementation. An earlier experiment changing
the full-owner rollback path missed its acceptance target.
[Rollback experiment](decoder-rollback-0.2.6.md)

## Method

We reused the exact two generic x86-64 binaries from the
[paired throughput comparison](decoder-native-0.2.5.md). Each process decoded
all 12 official streams for one warmup and 50 retained passes, on logical CPU
0. Both adapters reproduced the expected PCM fingerprint in both configurations.
The profiler samples the entire process, including input handling, warmup, replay
and output. These are flat on-CPU observations. They do not give the number of
bytes copied or the reduction in wall time that a code change would achieve.

We used the gperftools 2.18.1 CPU timer at 97 Hz and the gperftools 2.10 Perl
symbolizer with local debug symbols. System CPU counters were unavailable
under the host's perf settings. For each run, flat counts summed to the
profiler's reported total, and samples divided by 97 agreed within 5% with
process user plus system CPU time. Each configuration was profiled once per
implementation. Timing comparisons use the separate 15-round benchmark.

With the verified extracted vectors and binaries from the paired comparison,
run the following for each binary and output configuration. Set
`EXPECTED_FINGERPRINT` from that configuration's calibration record and
`LIBPROFILER` to the installed libprofiler shared library.

```sh
/usr/bin/time -f '%U %S %e' -o cpu.time taskset -c 0 env \
  LD_PRELOAD="$LIBPROFILER" CPUPROFILE_FREQUENCY=97 CPUPROFILE="$PWD/cpu.prof" \
  "$BINARY" "$VECTORS" "$RATE" "$CHANNELS" 50 1 0 "$EXPECTED_FINGERPRINT"
perl pprof --text "$BINARY" cpu.prof
perl pprof --collapsed "$BINARY" cpu.prof
```

The sanitized record includes binary hashes, fingerprints, counts and profiler
identities. Use new profile and time paths for each invocation. The raw profile
contains the host's process mappings. [Perl symbolizer source](https://github.com/gperftools/gperftools/blob/gperftools-2.10/src/pprof)
