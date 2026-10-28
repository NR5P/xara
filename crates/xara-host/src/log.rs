//! Where xara's log lines go: stderr, unless a program embedding the modem has taken them.
//! Every crate that prints goes through here, so an app sees xara's lines and nothing else.
//! A debug log file, when one is open, gets every line as well, stamped with the time.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

type Sink = Box<dyn Fn(&str) + Send + Sync>;

static SINK: OnceLock<Sink> = OnceLock::new();
static FILE: Mutex<Option<File>> = Mutex::new(None);

/// Sends every later log line to `f` instead of stderr. Only the first call has an effect.
pub fn set_sink(f: impl Fn(&str) + Send + Sync + 'static) {
    let _ = SINK.set(Box::new(f));
}

/// Also appends every later log line to `path`, or stops when `path` is None. An embedded modem
/// starts many times in one process, so each start says again whether it wants the file.
pub fn set_file(path: Option<&Path>) -> io::Result<()> {
    let f = match path {
        Some(p) => Some(OpenOptions::new().create(true).append(true).open(p)?),
        None => None,
    };
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = f;
    Ok(())
}

/// Appends a line to the debug log file only, for lines too frequent for an app's log view.
pub fn file_only(args: fmt::Arguments) {
    if let Some(f) = FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = writeln!(f, "{} {args}", stamp());
    }
}

pub fn line(args: fmt::Arguments) {
    file_only(args);
    match SINK.get() {
        Some(f) => f(&args.to_string()),
        None => eprintln!("{args}"),
    }
}

/// Wall-clock time of day in UTC with milliseconds, to line the log up with the other station's.
fn stamp() -> String {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis()) % 86_400_000;
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_gets_stamped_lines_until_closed() {
        let p = std::env::temp_dir().join(format!("xara-log-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&p);
        set_file(Some(&p)).unwrap();
        line(format_args!("one"));
        file_only(format_args!("two"));
        set_file(None).unwrap();
        line(format_args!("three"));
        let text = std::fs::read_to_string(&p).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].ends_with(" one") && lines[1].ends_with(" two"), "{text}");
        assert_eq!(lines[0].as_bytes()[2], b':');
        std::fs::remove_file(&p).unwrap();
    }
}
