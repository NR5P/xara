mod common;
use common::{Client, Lab};
use std::sync::{Arc, Mutex};

fn link(lab: &mut Lab, a: &mut Client, b: &mut Client) {
    lab.settle();
    for c in ["MYCALL N0CALL", "BW2300", "CHAT OFF", "COMPRESSION OFF"] {
        a.send(c);
    }
    for c in ["MYCALL N1CALL", "BW2300", "CHAT OFF", "LISTEN ON"] {
        b.send(c);
    }
    lab.settle();
    a.send("CONNECT N0CALL N1CALL");
    let bl = b.lines.clone();
    assert!(lab.run_until(60.0, |_| bl.lock().unwrap().iter().any(|x| x.starts_with("CONNECTED"))));
}

#[test]
fn the_readout_shows_the_link_and_its_text_as_it_arrives() {
    let got = Arc::new(Mutex::new(String::new()));
    let g = got.clone();
    xara::rxtext::set_sink(move |s| g.lock().unwrap().push_str(s));
    let mut lab = Lab::new(2);
    let (mut a, mut b) = (lab.attach(0), lab.attach(1));
    link(&mut lab, &mut a, &mut b);
    a.write(b"hello from N0CALL\rhow copy?\r");
    let bb = b.bytes.clone();
    assert!(lab.run_until(120.0, |_| bb.lock().unwrap().len() >= 28));
    assert_eq!(&b.bytes.lock().unwrap()[..], b"hello from N0CALL\rhow copy?\r", "the client still gets the bytes");
    a.send("DISCONNECT");
    let g = got.clone();
    assert!(lab.run_until(60.0, |_| g.lock().unwrap().contains("— disconnected —")));
    let text = got.lock().unwrap().clone();
    // Both stations feed the one sink here; each says who called whom and the answerer shows the text.
    assert!(text.contains("— connected N0CALL → N1CALL —\n"), "{text}");
    assert!(text.contains("hello from N0CALL\nhow copy?\n"), "{text}");
}
