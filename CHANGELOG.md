# Changelog

## Unreleased

- Windows and Mac/iPhone audio: a loop turn that runs long (decoding a big
  frame) no longer cuts the audio still coming in. Playback runs dry for it,
  and that used to be made up by dropping captured audio, which mostly lost
  the second frame of a double frame; it is now made up from the silence
  xara writes, and from capture only while transmitting. Against VARA FM
  4.4.1 (registered, over virtual cables), with VARA calling xara: Narrow
  6,139 / 871 bps each way before, 8,491 / 8,175 after (VARA to VARA 9,275 /
  8,192); Wide 8,549 / 513 before, 15,925 / 11,593 after (VARA to VARA
  16,781 / 12,940). VARA HF timings are unchanged

## 0.1.2

- BUSY no longer trips on plain noise when the radio's passband slopes: the
  tone detector learns the receiver's noise shape while the channel is clear
  and divides it out, so a strong bin on the loud side of the passband stops
  looking like a carrier. On an FTDX3000 on 14.105 MHz this took false BUSYs
  from every few seconds to almost none, and real beacons still trip it
- Windows audio: less delay through WASAPI, and the rest, including a virtual
  cable's own, allowed for so PTT holds until the audio has played

- (tr)uSDX audio over CAT: `--audio trusdx` and `--ptt trusdx` carry the
  radio's audio and PTT on its USB serial port, on the command line and in
  the Android app (*Connection: (tr)uSDX*), with no sound card. ported from
  Shane Curless VA3SHC's xara-android (x0rro/xara#12)

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
- registered speeds are built in by default (still off until `--registered`),
  and the apps and the web UI's settings have a *Registered (VARA licence)*
  switch
- the web UI's settings also set the audio delay, monitor mode (other
  stations' traffic, every packet, calls to name), recording and a debug log,
  the last two into the state directory

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
