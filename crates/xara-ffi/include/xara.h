// C interface to the xara modem, for embedding it in an app.
//
// xara runs exactly as the command does, on its own thread: the same options
// (see `xara --help`), TCP command/data/KISS ports and web UI. Audio uses the
// system's current route, so on iOS configure AVAudioSession first and pass
// "--capture route --playback route --radio --ptt ...".

#ifndef XARA_H
#define XARA_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct XaraModem XaraModem;

// The library version, e.g. "0.1.1". Static; do not free.
const char *xara_version(void);

// Starts the modem with command-line options (argv[0] is the first option,
// not a program name). Returns NULL if a modem is already running.
XaraModem *xara_start(const char *const *argv, size_t argc);

// True while the modem thread runs. It stops by itself on a fatal error
// (bad options, audio failure); xara_stop then returns its exit status.
bool xara_running(const XaraModem *modem);

// Stops the modem, waits for it (PTT is released first), frees it and
// returns the exit status: 0 for a clean stop, 2 for bad options, else 1.
uint8_t xara_stop(XaraModem *modem);

// Copies the next line xara logged into buf (NUL-terminated, cut to fit).
// Returns false when no line is waiting. Lines are also kept on stderr.
bool xara_next_log(char *buf, size_t cap);

#ifdef __cplusplus
}
#endif

#endif
