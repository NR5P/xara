# xara for Linux (app)

a Linux app for the xara modem: the same Rust modem as the `xara` command,
linked in through `crates/xara-ffi` as the Windows, Mac, iOS and Android apps
do, with a window.

 - **Modem**: the link's live status at the top while it runs (state, the
   station you're linked with, speed level, bitrate, S/N, AFC, bandwidth,
   modulation, attempts, buffer, PTT / BUSY / CLIP and the RX/TX levels) and
   **TUNE**: a steady carrier for at most 20 s with the TX drive adjustable
   live, to set the radio's ALC (use a clear frequency or a dummy load), then
   the settings: call sign, mode, TX drive, *Registered (VARA licence)*, audio
   in/out (any PulseAudio / PipeWire device), radio mode and PTT (VOX, serial
   RTS/DTR, CM108/CM119 GPIO, rigctld), PTT lead and tail, audio delay, ports,
   LAN access, open at login
 - **Web UI**: opens xara's own web UI (waterfall, levels, status) in your
   browser
 - **Received**: what the modem receives, as text: your own links always, and
   with *Show other stations' traffic* on, VARA HF traffic between other
   stations too (like VARA's monitor), either just the text or every packet
 - **Log**: everything xara logs

closing the window stops the modem (releasing PTT first) and quits. the usual
VARA ports are open while it runs: command 8300, data 8301, KISS 8100, web UI
8080. VarAC (under Wine), Openslot, Pat, Winlink Express and other VARA clients
use it as they would VARA (don't run VARA on the same ports at the same time).

the first start puts *xara* in your applications menu, with its icon.
settings are kept in `~/.config/xara/app.json`; the web UI's settings, the
debug log and recordings go to `~/.local/state/xara`.

## other programs

other programs can start the app with the modem running:

    xara-app --start

## building

needs rust and the PulseAudio and ALSA development files (on Debian / Ubuntu:
`sudo apt install libpulse-dev libasound2-dev`). from the repository:

    cargo build --release --manifest-path linux/Cargo.toml

the app is `linux/target/release/xara-app`; copy it anywhere (for example
`~/.local/bin`) and run it once to add it to the menu.

## audio

pick your radio's USB audio interface for audio in and out, and turn on radio
mode. without radio mode xara only plays into a virtual device, as the command
does without `--radio`: a PipeWire null sink, so nothing reaches a transmitter.
to run xara next to VARA (under Wine) or another xara on one PC with no radio,
make two null sinks, one for each direction:

    pactl load-module module-null-sink sink_name=xara_a2b
    pactl load-module module-null-sink sink_name=xara_b2a

| | plays into | listens to |
|---|---|---|
| VARA / the other modem | xara_a2b | xara_b2a.monitor |
| xara | xara_b2a | xara_a2b.monitor |

## PTT

 - **VOX**: the radio keys itself on the audio
 - **Serial RTS / DTR**: a serial port's RTS or DTR line (the other is held
   low). many radios with a USB interface key on RTS of their second serial
   port (on a Yaesu FTDX3000, the *Standard* port of its CP2105; the
   *Enhanced* one is CAT)
 - **CM108 / CM119 GPIO**: USB sound cards with a PTT line (DigiRig Lite,
   AIOC, many interfaces), by their `/dev/hidraw*` device. the device must be
   readable and writable by you (a udev rule, see `dist/udev`)
 - **rigctld**: hamlib's rigctld keys the radio over CAT (default
   127.0.0.1:4532)

your user needs access to serial ports (the `dialout` group on most
distributions).
