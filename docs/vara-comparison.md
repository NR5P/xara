# xara compared with VARA HF 4.9.0 (black-box, Windows)

October 31, 2026. VARA HF 4.9.0 (unregistered) and xara (this branch) on one Windows 11 PC,
linked over two VB-Audio virtual cables (CABLE-A one way, CABLE-B the other), driven on
their TCP host ports by a test harness. Nothing was disassembled or decompiled: VARA's
license forbids it, and everything here is observable behaviour (host-port bytes, the audio
on the cables, timings).

Four stations ran at once so every receiver could be compared on every transmitter:
VARA #1 and xara-A sending on CABLE-A, xara-B and a second VARA (its own folder and port,
as VARA's quick guide describes) receiving on it and replying on CABLE-B. Every transfer
is random data (compression off), timed from CONNECTED until the receiver's data port has
all of it.

Limits: VARA was unregistered, so only speed levels 1-4 were reachable (175 bps at 2300
and 2750 Hz, 88 bps at 500 Hz). One test PC, virtual cables, no fading or multipath;
most points are single runs.

## Summary

| Area | Result |
|---|---|
| Host command set (65 probes, incl. errors, case, junk) | identical replies (one notice arrives at a different moment) |
| Spectrum / occupied band (500, 2300, 2750) | same within ~10 Hz |
| Frame lengths, levels (data, call, answer, ACK) | identical |
| Turnaround (end of frame heard to reply) | 160-180 ms, as VARA (after the WASAPI fix below) |
| Clean-channel throughput, every pair, 3 bandwidths | xara matches VARA (after the fix) |
| SN reported for the same signal | within 0.3 dB of VARA |
| Weak signal, down to about -8 SN | xara receives as well as or better than VARA |
| Weak signal, about -11 SN | same within run-to-run variation (4 runs each); xara steps level 4 down a little sooner |
| Weak signal, about -14 SN | all pairs within 10% |
| Link setup | 8.2-8.3 s for every pair, as VARA (after the virtual-cable allowance below) |
| `DRIVELEVEL` | xara applies it; VARA replies OK and ignores it (kept: xara's is the useful one) |

## Fixed on this branch: delay in the Windows audio backend

The WASAPI backend kept the whole device buffer (40 ms) and a 20 ms queue in front of every
frame it sent, about 90 ms in all with the stream latency, and xara did not know about it.
Effects, all on Windows only:

- xara as receiver NACKed frames it could decode (decoding the same audio offline with
  xara-monitor: 88/88 frames good), so VARA stepped down and never reached `L4 x2` frames:
  VARA to xara ran at 106 bps (2300 Hz) and 90 bps (2750 Hz) against 138 for VARA to VARA.
- two xaras did not link (each one's answer arrived after the other's reply window).

The fix: playback keeps two device periods queued instead of the whole buffer; the backend
reports what remains (`Audio::latency`, default 0, so the other backends are unchanged) and
the daemon adds it to the latency it allows for, as for `--audio-latency-ms`; the audio
threads run with MMCSS "Pro Audio" priority.

Clean channel, net bits per second (link time in seconds), 600 B at 500 Hz, 2000 B otherwise:

| Pair (sender to receiver) | 500 Hz | 2300 Hz before / after | 2750 Hz before / after |
|---|---|---|---|
| VARA to VARA | 66.2 (8.3) | 138.1 / 137.8 (8.3) | 138.7 / 138.1 (8.3) |
| VARA to xara | 66.5 (8.3) | 106.2 / **138.4** (8.3) | 89.7 / **138.7** (8.3) |
| xara to VARA | 66.2 (10.7) | 136.6 / 137.8 (10.7) | 136.9 / 129.4 (10.7) |
| xara to xara | 66.2 (40.5) | no link / no link* | 94.2 / no link* |

\* links reliably (8.2 s) once the cable's own delay is allowed for; see the next section.

## Fixed: delay a virtual cable adds

A virtual cable buffers audio between its two ends, and WASAPI's stream latency doesn't
include it. xara's call and reply windows are VARA's to within 10-20 ms (calling a station
that isn't there: 1.28 s wait in Winlink mode, 1.83 s with CHAT ON, both modems), so it's
not a narrower window: xara measures its windows from where it believes its transmission
ended, and an unreported delay comes out of the margin. Two xaras over the cables, link
setup in seconds over four tries, with extra `--audio-latency-ms` on each:

| Extra allowed for | Link times |
|---|---|
| 0 ms | 27.1, 17.0, 8.2, 8.2 |
| 25 ms | 8.2, 8.2, 14.2, 8.2 |
| 50 ms | 8.2, 8.2, 8.2, 8.2 |

So the WASAPI backend now adds 25 ms for each end that is a virtual cable (VB-Audio,
VoiceMeeter, Virtual Audio Cable, BlackHole: the names it already checks before allowing
playback without `--radio`); a radio's sound card gets none. With no setting, every pair
now links in 8.2-8.3 s: xara to xara 4 of 4, xara to VARA and VARA to xara 3 of 3 each
(before: xara to VARA 10.7 s, xara to xara 8-27 s or no link).

A setup with delay that isn't reported (an SDR, network audio) still wants
`--audio-latency-ms` ("Audio delay" in the apps).

## Weak signal near -11 SN: no change made

Noise on the data path only (calibrated band-limited noise, 200-3200 Hz), 800 B at 2300 Hz:

| SN (VARA's scale) | VARA to VARA | VARA to xara | xara to VARA | xara to xara |
|---|---|---|---|---|
| about 0 | 110 | 125 | 126 | 127 |
| about -2.6 | 110 | 121 | 126 | 127 |
| about -5.5 | 110 | 126 | 126 | 127 |
| about -8.4 | 113 | 126 | 111 | 127 |
| about -11.3 (VV, VX: 4 runs; others 2) | 75 (89, 75, 73, 64) | 71 (74, 44, 84, 84) | 71 | 73 |
| about -14 | 43.5 | 42.2 | 41.3 | 38.6 |

Every transfer completed at every level. Decoding the ACKs each receiver sent (cable
recording, xara-monitor) against the SN of the frame it answered: after a good level 4
frame xara steps down at -11.3 to -11.9 SN and stays at -10.6 to -11.4 (its margin grows
after a lost frame), where VARA stayed at the two level 4 frames it got (-11.0, -9.8); both
stay at level 3 down to -12.1; xara climbs back to 4 more readily (5 times to VARA's 3).
xara's decode is fine (it decoded the frames it answered DOWN to), and the net throughput is
the same within the run-to-run spread, so the adaptation (`crates/xara-arq/src/hf/adapt.rs`,
fitted and tested as it is) is left alone. More runs at -10 to -12 SN would show whether
VARA really holds level 4 lower down.

## Kept: `DRIVELEVEL`

VARA 4.9.0 answers `OK` and does not change its output; xara applies it. Kept as it is.

## Matching (no gap found)

- All 65 host-command probes: same `OK`/`WRONG`, same PENDING/CANCELPENDING/BUSY sequence
  around CONNECT, CQFRAME and TUNE.
- Signal: occupied band at -20 dB 656-2309 Hz (VARA) vs 668-2320 Hz (xara) at 2300,
  504-2473 vs 504-2484 at 2750, 738-1840 at 500 for both; data frames 4.35 s (4.29 at 500);
  rms levels -11.4 dBFS data and call/answer frames, -13.9 dBFS ACK frames, for both.
- Link setup, header (`start level 4`, unregistered, compression off), ACK / ACK+1 / NACK /
  GO AHEAD codes, `L4 x2` double frames after six good frames: the same exchange.
- Calls in noise: xara answered within one or two calls; VARA often needed several (about
  20 s to link at every noise level, against 8-14 s for the others).
