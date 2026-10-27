use std::time::{Duration, Instant};

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

fn logged(want: &str, within: Duration) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < within {
        match xara_ffi::next_log() {
            Some(l) if l.contains(want) => return true,
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    false
}

fn main() {
    let m = xara_ffi::start(args("--call N0CALL --capture c --playback p")).unwrap();
    assert!(logged("N0CALL is not a callsign", Duration::from_secs(5)));
    let t0 = Instant::now();
    while m.running() {
        assert!(t0.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(m.stop(), 2);

    let m = xara_ffi::start(args("--call N1CALL --capture c --playback p --list-devices")).unwrap();
    assert!(xara_ffi::start(args("--version")).is_none() || !m.running());
    assert_eq!(m.stop(), 0);
}
