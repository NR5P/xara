# xara for Windows (app)

a Windows app for the xara modem: the same Rust modem as the `xara` command,
linked in through `crates/xara-ffi` as the Mac, iOS and Android apps do, with
a window and a tray icon.

 - **Modem**: call sign, mode, TX drive, audio in/out (any WASAPI device),
   radio mode and PTT (VOX, serial RTS/DTR on a COM port, rigctld), PTT lead
   and tail, audio delay, ports, LAN access, start with Windows
 - **Web UI**: opens xara's own web UI (waterfall, levels, status) in your
   browser
 - **Received**: what the modem receives, as text: your own links always, and
   with *Show other stations' traffic* on, VARA HF traffic between other
   stations too (like VARA's monitor), either just the text or every packet
 - **Log**: everything xara logs
 - **tray icon**: running state (blue when running), start/stop, show the
   window, open the web UI, quit

closing the window leaves the modem running in the tray; quit from the tray
menu, which releases PTT first. the usual VARA ports are open while it runs:
command 8300, data 8301, KISS 8100, web UI 8080. VarAC, Openslot, Winlink
Express and other VARA clients use it as they would VARA (don't run VARA on
the same ports at the same time).

settings are kept in `%APPDATA%\xara\app.json`; the debug log and recordings
go to `%LOCALAPPDATA%\xara\logs`.

## other programs

other programs can start the app with the modem running:

    xara-app.exe --start

## building and installing

needs rust with the MSVC toolchain (Visual Studio's C++ build tools). from the
repository:

    powershell -ExecutionPolicy Bypass -File windows\install.ps1

builds `xara`, `xara-monitor` and the app, and installs them for your user in
`%LOCALAPPDATA%\Programs\xara` with a Start menu and a desktop shortcut (no
administrator needed). `-NoBuild` installs what is already built,
`-Uninstall` removes it again.

for development: `cargo run --release --manifest-path windows/Cargo.toml`.

## audio

without radio mode xara only plays into a virtual audio device, as the command
does without `--radio`. to run xara next to VARA (or VarAC with VARA) on one
PC with no radio, install two virtual cables, one for each direction, e.g.
[VB-CABLE](https://vb-audio.com/Cable/) and VB-Audio's Hi-Fi Cable:

| | plays into | listens to |
|---|---|---|
| VARA | CABLE Input | Hi-Fi Cable Output |
| xara | Hi-Fi Cable Input | CABLE Output |
