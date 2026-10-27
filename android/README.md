# xara for Android

an Android app that runs the xara modem. it is the same Rust modem as the
`xara` command, built as a shared library (`crates/xara-ffi`, through JNI),
with a small Jetpack Compose app around it:

 - **Modem**: call sign, HF/FM mode, TX drive, audio in/out, PTT, start/stop
 - **Web UI**: xara's own web UI (waterfall, levels, status) in a web view
 - **Received**: what the modem receives, as text; turn on *Show other
   stations' traffic* to also decode VARA HF traffic between other stations,
   showing just the text or every packet heard
 - **Log**: everything xara logs

the usual VARA ports are open while it runs: command 8300, data 8301,
KISS 8100. turn on *Accept clients on the local network* to use the phone as
the modem for Winlink Express, Pat or similar running on another machine.

## building

needs the Android SDK with NDK 28.2 (Android Studio's SDK manager, or
`sdkmanager "ndk;28.2.13676358"`), a JDK 17 (Android Studio's own works) and
rust with the Android targets:

    rustup target add aarch64-linux-android x86_64-linux-android

then open `android/` in Android Studio and run, or:

    cd android && ./gradlew assembleDebug

gradle runs `scripts/build_android.sh` first, which builds `libxara_ffi.so`
for arm64-v8a (phones) and x86_64 (the emulator on Intel) into
`app/src/main/jniLibs`. set `sdk.dir` in `android/local.properties` if the
SDK is not found.

## radio hookup

plug in a USB audio interface (a Digirig, or any class-compliant USB sound
card) and pick it as *Audio in* and *Audio out*; a USB device is chosen by
default when one is connected. xara opens the device by id through AAudio at
48 kHz, using the unprocessed input preset where the phone has it, so there
is no voice processing or AGC. xara will not start on the phone's own
speaker or mic unless *Allow the phone's own speaker and mic* is on (for
testing only). unplugging the interface stops the modem.

PTT is VOX or hamlib `rigctld` reachable over the network. a foreground
service with a notification keeps the modem running in the background.

## not done yet

 - no app icon
 - USB serial (CDC) and CM108 PTT, which Android would allow through its USB
   host API
 - tested in the emulator; not yet on a phone with a radio attached
