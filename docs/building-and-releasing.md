# Building and releasing xara (NR5P fork)

How to build every part of this repository by hand, check it the way CI does, cut a release,
and keep up with the original project. Everything here is a plain command you can run in
Terminal; nothing needs an AI or an IDE except where Xcode or Android Studio is the tool.

- [1. What this repository is](#1-what-this-repository-is)
- [2. One-time setup](#2-one-time-setup)
- [3. What gets built, and how](#3-what-gets-built-and-how)
- [4. Building each part](#4-building-each-part)
- [5. Checking a change before you push](#5-checking-a-change-before-you-push)
- [6. Releasing xara](#6-releasing-xara)
- [7. Bringing in changes from upstream (x0rro/xara)](#7-bringing-in-changes-from-upstream-x0rroxara)
- [8. How OpenSlot uses this repository](#8-how-openslot-uses-this-repository)
- [9. Troubleshooting](#9-troubleshooting)

## 1. What this repository is

This is **NR5P/xara**, a fork of **x0rro/xara**. The fork adds the iOS, Android, macOS and
Windows apps, the (tr)uSDX support, the BUSY noise-shape fix and more; the original is the
modem and the Linux command.

The checkout at `~/Desktop/xara` has two git remotes:

| Remote | Repository | Used for |
|---|---|---|
| `fork` | `https://github.com/NR5P/xara` | your fork: you push here |
| `origin` | `https://github.com/x0rro/xara` | the original: you only fetch from it |

(This is the reverse of the usual naming, where `origin` is your own. It works, just be
careful: `git push origin` would try to push to x0rro's repository, and fail.)

Your `main` tracks `fork/main`, so a plain `git pull` or `git push` on `main` talks to your
fork, never to x0rro's. Check with:

    git remote -v
    git branch -vv

**OpenSlot builds its built-in modem from this folder**, so whatever branch is checked out
here when you build OpenSlot is the modem OpenSlot gets. See [section 8](#8-how-openslot-uses-this-repository).

### Layout

| Path | What it is |
|---|---|
| `crates/xara-phy` | the modem's signal processing (modulation, demodulation, BUSY) |
| `crates/xara-arq` | the link protocol (connect, ACKs, timing, retries) |
| `crates/xara-host` | the VARA-compatible TCP ports (command 8300, data 8301, KISS 8100) |
| `crates/xara-ui` | the built-in web UI (port 8080) and its settings |
| `crates/xara` | the `xara` and `xara-monitor` commands, audio backends, PTT |
| `crates/xara-ffi` | the modem as a library (C header `crates/xara-ffi/include/xara.h`, JNI) for the apps and OpenSlot |
| `crates/xara-testkit` | test helpers |
| `linux/`, `macos/`, `windows/`, `ios/`, `android/` | the apps; each has its own README |
| `scripts/` | build and release scripts |
| `dist/` | man pages, systemd and launchd files, udev rules, example config |
| `.github/workflows/` | CI (`ci.yml`) and releases (`release.yml`) |

## 2. One-time setup

### Every platform: Rust

Install Rust with rustup (<https://rustup.rs>):

    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

The minimum Rust version is 1.85 (`rust-version` in `Cargo.toml`); any newer stable works.
Update with `rustup update`.

### Per platform

| To build | Also install |
|---|---|
| Linux command or app | `sudo apt install build-essential pkg-config libpulse-dev libasound2-dev` (Fedora: `sudo dnf install pulseaudio-lib-devel alsa-lib-devel`) |
| macOS command | nothing else (audio is CoreAudio) |
| macOS app | Xcode |
| iOS app | Xcode, then `rustup target add aarch64-apple-ios aarch64-apple-ios-sim` |
| Android app | Android Studio (or the command-line SDK) with **NDK 28.2** (`sdkmanager "ndk;28.2.13676358"`), JDK 17 (Android Studio's own works), then `rustup target add aarch64-linux-android x86_64-linux-android` |
| Windows command or app | Visual Studio's "Desktop development with C++" build tools (the MSVC toolchain) |

## 3. What gets built, and how

Every app contains the same Rust modem. The apps don't copy it: their build compiles
`crates/xara-ffi` from this repository first, then links it in.

| Thing | Built by | Rust step it runs | Output |
|---|---|---|---|
| `xara`, `xara-monitor` commands | `cargo build --release` | (itself) | `target/release/xara`, `target/release/xara-monitor` |
| Linux app | `cargo build --release --manifest-path linux/Cargo.toml` | (itself; xara-ffi is a dependency) | `linux/target/release/xara-app` |
| Mac app | Xcode project `macos/XaraMac.xcodeproj` | *Build Rust modem* phase: `cargo build --locked --release -p xara-ffi --features registered` | `Xara.app` in Xcode's build products |
| iPhone/iPad app | Xcode project `ios/Xara.xcodeproj` | *Build Rust modem* phase: `scripts/build_ios.sh --target <triple>` | installed by Xcode, or an archive |
| Android app | Gradle in `android/` | `preBuild` runs `scripts/build_android.sh arm64-v8a x86_64` into `android/app/src/main/jniLibs` | `android/app/build/outputs/apk/...` |
| Windows app and commands | `windows\install.ps1` | `cargo build --release -p xara`, then `windows/Cargo.toml` | installed in `%LOCALAPPDATA%\Programs\xara` |

The `registered` feature (registered VARA speeds) is built into all of these by default. It
stays off at run time unless the user turns it on (`--registered`, or the app's
*Registered (VARA licence)* switch).

## 4. Building each part

All commands are run from the repository root (`cd ~/Desktop/xara`) unless they say
otherwise.

### The `xara` command (Linux, macOS, Windows)

    cargo build --release

The commands are `target/release/xara` and `target/release/xara-monitor`. Try one:

    target/release/xara --list-devices
    target/release/xara --help

To install it on your PATH from your fork, without a checkout:

    cargo install --locked --git https://github.com/NR5P/xara xara

(The README's line installs from x0rro's original instead, which has no apps.)

Linux system install (binaries, man pages, systemd units, udev rule) after building:

    sudo dist/install.sh

On Linux, `--no-default-features --features pulse` or `--features alsa` builds with just one
audio backend.

### Linux app

    cargo build --release --manifest-path linux/Cargo.toml

Copy `linux/target/release/xara-app` somewhere on your PATH (for example `~/.local/bin`) and
run it once; it adds itself to the applications menu. Details: [linux/README.md](../linux/README.md).

### Mac app

In Xcode: open `macos/XaraMac.xcodeproj` and press Run. From Terminal:

    xcodebuild -project macos/XaraMac.xcodeproj -scheme XaraMac -configuration Release build

Xcode prints where it put `Xara.app` (under `~/Library/Developer/Xcode/DerivedData/`); copy
it to `/Applications`. To put the build somewhere predictable, add
`-derivedDataPath macos/build` and find it at `macos/build/Build/Products/Release/Xara.app`.
Details: [macos/README.md](../macos/README.md).

### iPhone/iPad app

Open `ios/Xara.xcodeproj`, choose your iPhone or a simulator, set your team under
*Signing & Capabilities*, and press Run. The *Build Rust modem* phase compiles the modem
for the right target first.

Simulator build from Terminal (no signing needed):

    xcodebuild -project ios/Xara.xcodeproj -scheme Xara \
      -destination 'generic/platform=iOS Simulator' \
      -derivedDataPath ios/build/dd CODE_SIGNING_ALLOWED=NO build

`scripts/build_ios.sh` with no arguments builds the device and simulator libraries and
packages them as `ios/build/XaraFFI.xcframework`, for use in another Xcode project.
Details: [ios/README.md](../ios/README.md).

### Android app

    cd android
    ./gradlew assembleDebug

The APK is `android/app/build/outputs/apk/debug/app-debug.apk`. Install it on a phone with
USB debugging on:

    adb install -r app/build/outputs/apk/debug/app-debug.apk

Or open `android/` in Android Studio and press Run. If Gradle can't find the SDK, create
`android/local.properties` with `sdk.dir=/Users/<you>/Library/Android/sdk`. If the NDK is
somewhere unusual, set `ANDROID_NDK_HOME`. Details: [android/README.md](../android/README.md).

Note: the release build type is signed with the debug key, which is fine for sideloading
but not for the Play Store.

### Windows app and commands

In PowerShell, from the repository:

    powershell -ExecutionPolicy Bypass -File windows\install.ps1

That builds `xara`, `xara-monitor` and the app and installs them for your user with Start
menu and desktop shortcuts. `-NoBuild` installs what's already built; `-Uninstall` removes
it. Details: [windows/README.md](../windows/README.md).

## 5. Checking a change before you push

CI (`.github/workflows/ci.yml`) runs on every push to your fork. Run the same checks locally
first; on a Mac:

    cargo fmt --all --check
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo test --locked --release --workspace

The test suite takes a few minutes; every test should pass. `cargo fmt --all` (without
`--check`) fixes formatting.

If you changed the apps or `xara-ffi`, also check the phone builds:

    cargo clippy --locked -p xara-ffi --target aarch64-apple-ios -- -D warnings
    cargo clippy --locked -p xara-ffi --target aarch64-linux-android -- -D warnings
    xcodebuild -project macos/XaraMac.xcodeproj -scheme XaraMac CODE_SIGNING_ALLOWED=NO build

CI also builds for Linux (with PulseAudio and ALSA), ARM Linux under QEMU, Android and
Rust 1.85, which a Mac can't do by itself; check the run on GitHub after pushing:

    gh run list -R NR5P/xara -L 5
    gh run view <run-id> -R NR5P/xara --log-failed

**Known issue (November 2026):** CI on `main` has been failing on two tests that pass on a
Mac: `xara-ffi`'s `embed` test on Linux (CI has no sound card) and `xara-ui`'s
`a_closed_or_silent_control_page_is_reported_gone` on macOS (timing). Until those are fixed,
a red CI run needs reading, not just a glance.

### Work on a branch

Keep `main` the version you'd ship, because OpenSlot builds from it. For any change:

    git switch -c my-change
    # edit, build, run the checks above
    git commit -am "Say what changed and why"
    git push -u fork my-change

Then open a pull request on GitHub (`gh pr create -R NR5P/xara`) or merge it yourself:

    git switch main
    git merge my-change
    git push

## 6. Releasing xara

A release of xara is a version number, a changelog entry, and a git tag. Pushing the tag
makes GitHub build the Linux downloads.

1. **Pick the version** and set it in the root `Cargo.toml`, under `[workspace.package]`:

       version = "0.1.3"

   Then refresh the lock file so `--locked` builds still work:

       cargo build

2. **Write the changelog**: add a `## 0.1.3` section at the top of `CHANGELOG.md`. The
   release notes are taken from that section word for word.

3. **Check it** ([section 5](#5-checking-a-change-before-you-push)), then commit and push `main`:

       git commit -am "xara 0.1.3"
       git push

4. **Tag it and push the tag** to your fork:

       git tag v0.1.3
       git push fork v0.1.3

5. **Wait for the Release workflow** (`gh run watch -R NR5P/xara`). It:
   - checks that the tag (`v0.1.3`) matches the version in `Cargo.toml` (`0.1.3`), and stops
     if not
   - builds `xara` and `xara-monitor` for Linux x86_64, aarch64 and armv7
     (`scripts/release_bins.sh`), each as `xara-0.1.3-<target>.tar.xz` with the README,
     licence and `dist/`
   - makes a source tarball and a tarball of every dependency's source
     (`scripts/release_tarball.sh`), so it builds offline
   - writes `SHA256SUMS` and a build-provenance attestation
   - creates a **draft** release named "xara 0.1.3"

6. **Publish the draft**: on GitHub (Releases, Edit, Publish) or with

       gh release edit v0.1.3 -R NR5P/xara --draft=false

The apps aren't built by the release workflow. To attach them, build them as in
[section 4](#4-building-each-part) and add them to the release:

    gh release upload v0.1.3 -R NR5P/xara Xara-mac.zip

Zip a Mac app with `ditto -c -k --keepParent Xara.app Xara-mac.zip`, which keeps its
signature intact.

If a tag was wrong, delete it locally and on GitHub, fix things, and tag again:

    git tag -d v0.1.3
    git push fork :refs/tags/v0.1.3

## 7. Bringing in changes from upstream (x0rro/xara)

x0rro keeps working on the original. To see what's new without changing anything:

    git fetch origin
    git log --oneline main..origin/main

`git fetch` only downloads; it never changes your branches or your files.

To bring the changes in, do it on a branch, so `main` stays safe until it's tested:

    git switch -c upstream-sync main
    git merge origin/main

If git reports conflicts, the fork and upstream changed the same lines. For each conflicted
file, `git diff` shows both sides between `<<<<<<<` and `>>>>>>>` markers; edit the file to
the version you want, then `git add` it. When all are done, `git commit`. To give up and go
back to how it was: `git merge --abort`.

Merging all of upstream can conflict a lot where the fork has its own version of the same
feature (the Mac audio and (tr)uSDX code, for example). The alternative is to bring in only
the parts you want:

- **One whole upstream commit:** `git cherry-pick <commit>`
- **Only some folders of upstream's changes:** make a patch of just those paths between the
  last upstream commit you already have and upstream's newest one, and apply it:

      git diff <old-upstream-commit> origin/main -- crates/xara-arq crates/xara-phy > ../core.patch
      git apply -3 --index ../core.patch

  This is how the `upstream-0.1.3-fixes` branch brought in upstream 0.1.2 and 0.1.3's modem
  fixes without their audio rewrite.

Then run the full checks in [section 5](#5-checking-a-change-before-you-push), merge into
`main`, and push.

Note the fork's own improvements upstream doesn't have (the BUSY noise-shape fix, for
example) in the commit message, and consider offering them back to x0rro as a pull request.

## 8. How OpenSlot uses this repository

OpenSlot (`~/Desktop/OpenSlot`, NR5P/OpenSlot) has the xara modem built in. It doesn't keep a
copy: every OpenSlot app points at this folder with a Cargo path dependency
(`path = "../../xara/crates/xara-ffi"`), so the two folders must sit side by side on the
Desktop.

| OpenSlot build | Which xara it gets |
|---|---|
| Mac, iPhone, Android, and Linux/Windows built on your own computer | this folder, **on whatever branch is checked out** |
| Linux and Windows downloads built by OpenSlot's Release workflow on GitHub | a fresh clone of **NR5P/xara `main` on GitHub** |

So before an OpenSlot release:

1. `git switch main` here, and make sure `git status` is clean.
2. `git push` so GitHub's `main` matches yours.

Otherwise the Mac and phone builds (from this folder) and the Linux and Windows builds (from
GitHub) can end up with different modems. OpenSlot's own guide,
`OpenSlot/docs/building-and-releasing.md`, has its side of this.

## 9. Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `failed to run custom build command for alsa-sys` on a Mac | the ALSA backend is Linux-only; build normally (it's off on macOS), don't force `--features alsa` with a Linux target |
| `the lock file needs to be updated but --locked was passed` | a version or dependency changed; run `cargo build` once without `--locked` and commit `Cargo.lock` |
| Xcode: `cargo: command not found` | the build phases add `~/.cargo/bin` to PATH; if Rust is installed elsewhere, symlink cargo there |
| Xcode: `can't find crate for core` (iOS) | `rustup target add aarch64-apple-ios aarch64-apple-ios-sim` |
| Gradle: `no Android NDK found` | install NDK 28.2 with the SDK manager, or set `ANDROID_NDK_HOME` |
| Gradle: `SDK location not found` | create `android/local.properties` with `sdk.dir=...` |
| macOS keeps asking for microphone access | every rebuild with an ad hoc signature looks like a new app; sign with your Apple Development or Developer ID certificate |
| `git push origin` is refused | `origin` is x0rro's repository; push to `fork` |
