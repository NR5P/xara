//! Where xara's log lines go: stderr, unless a program embedding the modem has taken them.
//! Every crate that prints goes through here, so an app sees xara's lines and nothing else.

use std::fmt;
use std::sync::OnceLock;

type Sink = Box<dyn Fn(&str) + Send + Sync>;

static SINK: OnceLock<Sink> = OnceLock::new();

/// Sends every later log line to `f` instead of stderr. Only the first call has an effect.
pub fn set_sink(f: impl Fn(&str) + Send + Sync + 'static) {
    let _ = SINK.set(Box::new(f));
}

pub fn line(args: fmt::Arguments) {
    match SINK.get() {
        Some(f) => f(&args.to_string()),
        None => eprintln!("{args}"),
    }
}
