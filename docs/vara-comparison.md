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
| Weak signal, about -11 SN | xara as receiver ~15-30% slower (open) |
| Weak signal, about -14 SN | all pairs within 10% |
| First answer after a call with unaccounted audio delay | VARA tolerates ~100 ms more than xara (open) |
| `DRIVELEVEL` | xara applies it; VARA replies OK and ignores it |

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

\* links reliably (8.3 s) with `--audio-latency-ms 50`; see the first open gap.

## Open gaps

1. **First answer after a call: less tolerance of unaccounted delay than VARA.**
   With ~50 ms of delay that Windows does not report (the virtual cable's own buffering),
   xara as caller lets VARA's first answer go and calls again (it adapts and takes the
   second), so it links in 10.7 s where VARA takes 8.3; two xaras often do not link.
   VARA as caller took xara's answers even when they were ~90 ms late. With the delay set
   (`--audio-latency-ms 50`), both cases link in 8.3 s. Any setup whose delay isn't known
   (SDRs, network audio) would see this; a wider initial reply window when calling, as VARA
   appears to use, would close it. The reply window is `Timing::reply_window` in
   `crates/xara-arq/src/hf/timing.rs` (initial control delay 548 ticks + 150).

2. **Throughput as receiver around -11 SN.**
   Noise on the data path only (calibrated band-limited noise, 200-3200 Hz), 800 B at 2300 Hz:

   | SN (VARA's scale) | VARA to VARA | VARA to xara | xara to VARA | xara to xara |
   |---|---|---|---|---|
   | about 0 | 110 | 125 | 126 | 127 |
   | about -2.6 | 110 | 121 | 126 | 127 |
   | about -5.5 | 110 | 126 | 126 | 127 |
   | about -8.4 | 113 | 126 | 111 | 127 |
   | about -11.3 (2 runs, mean) | 82 | 59 | 71 | 73 |
   | about -14 | 43.5 | 42.2 | 41.3 | 38.6 |

   Every transfer completed at every level. At about -11 SN VARA sends xara mostly levels
   2-3 where VARA receiving gets mostly 3-4; whether xara's level 4 decode needs a dB or two
   more in noise or its ACK/NACK policy steps down sooner needs more runs to tell apart.

3. **`DRIVELEVEL`**: VARA 4.9.0 answers `OK` and does not change its output; xara applies it.
   Harmless, but a client that sends it gets different levels from the two modems.

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
