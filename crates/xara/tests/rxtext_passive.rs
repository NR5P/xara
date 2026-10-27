mod common;

use common::air::{self, BLOCK};
use std::sync::{Arc, Mutex};
use xara::rxtext::Passive;
use xara::station::hf::Station;
use xara::station::{Modem, Options, Report};
use xara_arq::hf::session::Config;
use xara_arq::host::HostEvent;
use xara_phy::hf::level::Bandwidth;

#[test]
fn other_stations_traffic_is_shown_when_monitoring_at_its_own_bandwidth_or_another() {
    // One test, as the received-text sink is global to the process.
    other_stations_traffic_at(Bandwidth::Hz2300, Bandwidth::Hz2300);
    other_stations_traffic_at(Bandwidth::Hz500, Bandwidth::Hz2300);
}

fn other_stations_traffic_at(link_bw: Bandwidth, modem_bw: Bandwidth) {
    let got = Arc::new(Mutex::new(String::new()));
    let g = got.clone();
    xara::rxtext::set_sink(move |s| g.lock().unwrap().push_str(s));
    let bw = link_bw;
    let st = [
        Station::new(Config { seed: 3, ..Config::new(bw, "N0CALL") }, Options::default()),
        Station::new(Config { seed: 5, ..Config::new(bw, "N1CALL") }, Options::default()),
    ];
    let mut link = air::Air::pair(st, [2400; 2], 0.0, |r| !matches!(r, Report::Host(HostEvent::Connected { .. })));
    let passive = Passive::spawn(modem_bw, &["N1CALL".to_string()]).unwrap();
    let step = |link: &mut air::Air<Station, 2>| {
        link.step();
        let w = &link.wire;
        let tail = |i: usize| w[i].range(w[i].len() - BLOCK..);
        let heard: Vec<f64> = tail(0).zip(tail(1)).map(|(a, b)| a + b).collect();
        // The simulation runs faster than real time, so wait rather than drop audio.
        passive.feed_wait(&heard);
    };
    let linked = |l: &air::Air<Station, 2>| {
        l.events.iter().all(|e| e.iter().any(|(_, r)| matches!(r, Report::Host(HostEvent::Connected { .. }))))
    };
    let run = |link: &mut air::Air<Station, 2>, secs: f64, done: &dyn Fn(&air::Air<Station, 2>) -> bool| {
        let limit = link.secs() + secs;
        while link.secs() < limit {
            if done(link) {
                return true;
            }
            step(link);
        }
        done(link)
    };
    link.st[0].connect("N0CALL", "N1CALL", &[]);
    assert!(run(&mut link, 40.0, &linked), "no link");
    let msg = b"QSL on the 20m net, 73\r";
    link.st[0].send(msg);
    assert!(run(&mut link, 60.0, &|l| l.got[1].len() >= msg.len()), "transfer stalled");
    link.st[0].disconnect();
    assert!(run(&mut link, 40.0, &|l| l.st.iter().all(Station::idle)), "no disconnect");
    run(&mut link, 10.0, &|_| false);
    drop(passive);
    let text = got.lock().unwrap().clone();
    assert!(text.contains("— showing other stations' traffic (500, 2300 Hz) —"), "{text}");
    assert!(text.contains(&format!("— heard N0CALL → N1CALL ({} Hz) —", link_bw.hz())), "{text}");
    assert!(text.contains("— N0CALL → N1CALL ended"), "{text}");
    assert!(text.contains("QSL on the 20m net, 73"), "{text}");
}
