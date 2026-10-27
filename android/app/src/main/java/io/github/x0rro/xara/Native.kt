package io.github.x0rro.xara

/** The Rust modem (crates/xara-ffi), which runs exactly as the `xara` command does. */
object Native {
    init {
        System.loadLibrary("xara_ffi")
    }

    external fun version(): String

    /** Starts xara with command-line options; returns a handle, or 0 if one is already running. */
    external fun start(args: Array<String>): Long

    /** False once the modem has stopped by itself (bad options, audio failure). */
    external fun running(handle: Long): Boolean

    /** Stops the modem, releasing PTT first, and frees the handle. Returns the exit status. */
    external fun stop(handle: Long): Int

    /** The next line xara logged, or null. */
    external fun nextLog(): String?

    /** The next piece of what the modem received, as readable text, or null. */
    external fun nextReceived(): String?
}
