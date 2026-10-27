# Changelog

## unreleased

- macOS support: CoreAudio backend, `--list-devices`, CM108 PTT by USB id,
  launchd agent
- iOS app (`ios/`) embedding the modem through the new `xara-ffi` crate
- the host, KISS and web UI servers free their ports when dropped

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
