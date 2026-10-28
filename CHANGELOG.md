# Changelog

## unreleased

- macOS support: CoreAudio backend, `--list-devices`, CM108 PTT by USB id,
  launchd agent
- iOS app (`ios/`) embedding the modem through the new `xara-ffi` crate
- the host, KISS and web UI servers free their ports when dropped
- Android app (`android/`): AAudio backend and JNI interface in `xara-ffi`
- log lines go through one hook, so an embedding app gets xara's alone
- macOS app (`macos/`): window, menu bar icon, settings, and `xara://start`
  and `xara://stop` for other apps
- received-text readout in the iOS and macOS apps, and `--monitor` to also
  decode other stations' VARA HF traffic (`--monitor-call` names them) at
  500 and 2300 Hz at once; `--monitor-frames` lists every frame heard, as the
  apps' *Every packet* setting; the Android app gets the Received tab too
- the web UI (in every app) names the link's modulation, e.g. "OFDM 49
  carriers · 16QAM", and monitor mode's DATA lines do too

## 0.1.1

recording feature and bugfixes

- `--record` flag captures rx, tx, and PTT in FLAC
- bugfix: long squelch tails no longer trigger false receive
- bugfix: phantom frames no longer send inappropriate compression

## 0.1.0

first release

- modem is fully compatible with VARA HF 4.9.0 and VARA FM 4.4.0
- web UI for configuration and management
- all the usual TCP ports (works with your favorite software)
- pipewire, pulse, and alsa support
- `xara-monitor`: passive VARA HF monitor (with web UI)
