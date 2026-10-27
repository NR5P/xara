# xara for iOS

an iPhone/iPad app that runs the xara modem. it is the same Rust modem as the
`xara` command, linked in as a static library (`crates/xara-ffi`), with a small
SwiftUI app around it:

 - **Modem**: call sign, HF/FM mode, TX drive, PTT, start/stop
 - **Web UI**: xara's own web UI (waterfall, levels, status) in a web view
 - **Log**: everything xara logs

the usual VARA ports are open while it runs: command 8300, data 8301,
KISS 8100. turn on *Accept clients on the local network* to use the iPhone as
the modem for Winlink Express, Pat or similar running on another machine.

## building

needs Xcode and rust with the iOS targets:

    rustup target add aarch64-apple-ios aarch64-apple-ios-sim

then open `ios/Xara.xcodeproj` and run. the *Build Rust modem* phase builds
the library (`scripts/build_ios.sh --target ...`) before Swift compiles, so a
Rust change is picked up on the next build. set your own team under Signing.

`scripts/build_ios.sh` with no arguments builds both libraries and packages
them as `ios/build/XaraFFI.xcframework`, for use in another project.

## radio hookup

audio goes wherever iOS routes it, so plug in a USB audio interface (a Digirig,
or any class-compliant USB sound card) before starting. the session runs at
48 kHz in measurement mode, so there is no voice processing or AGC. xara will
not start, and stops, if the route is the phone's own speaker or mic, unless
*Allow the iPhone speaker and mic* is on (for testing only).

iOS has no serial port or HID access, so PTT is VOX or hamlib `rigctld`
reachable over the network (`rigctld` on a Raspberry Pi next to the radio, for
example). the app keeps running in the background while it is receiving.

a phone call or other audio interruption stops the modem and starts it again
afterwards; a route change to the phone's speaker stops it.

## not done yet

 - no app icon
 - tested in the simulator; not yet on a device with a radio attached
