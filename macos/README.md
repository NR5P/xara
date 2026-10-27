# xara for macOS (app)

a Mac app for the xara modem: the same Rust modem as the `xara` command,
linked in through `crates/xara-ffi`, with a window and a menu bar icon.

 - **Modem**: call sign, mode, TX drive, audio in/out (any CoreAudio device),
   radio mode and PTT (VOX, serial RTS/DTR, CM108/AIOC by USB id, rigctld),
   ports, LAN access, start at login
 - **Web UI**: xara's own web UI (waterfall, levels, status)
 - **Received**: what the modem receives, as text: your own links always, and
   with *Show other stations' traffic* on, VARA HF traffic between other
   stations too (like VARA's monitor), either just the text or every packet
   heard. Winlink's compressed data shows as a one-line summary rather than
   noise
 - **Log**: everything xara logs
 - **menu bar**: running state, start/stop, open the window or the web UI

closing the window leaves the modem running; quit from the menu bar icon or
with ⌘Q, which releases PTT first. the usual VARA ports are open while it
runs: command 8300, data 8301, KISS 8100, web UI 8080.

## other apps

other apps can start and stop the modem with URLs, so a Winlink client can
bring it up when it needs it:

    open xara://start
    open xara://stop

`xara://start` launches the app if needed and starts the modem with its
saved settings. the bundle id is `io.github.x0rro.xara.mac`.

## building

open `macos/XaraMac.xcodeproj` and run, or:

    xcodebuild -project macos/XaraMac.xcodeproj -scheme XaraMac -configuration Release build

the *Build Rust modem* phase runs `cargo build --release -p xara-ffi` first.
then copy `Xara.app` from the build products into `/Applications`. the first
start asks for microphone access; that is how xara hears the radio.

without radio mode xara only plays into a virtual audio device (e.g.
BlackHole), as the command does without `--radio`.
