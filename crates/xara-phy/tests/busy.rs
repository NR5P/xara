mod common;
use common::{Rng, bw_of, load_doc};
use xara_phy::busy::{
    Busy, HANG, Signature, detect_signature, detect_tone, signature_threshold, signatures, tone_band,
};
use xara_phy::hf::level::{Bandwidth, Record};
use xara_phy::hf::tx::Modulator;
use xara_testkit::json::Value;

const BLOCK48: usize = 128 * 16;

fn spectrum(v: &Value, n: usize) -> Vec<f64> {
    let mut p = vec![0.0; n];
    for (k, x) in v.as_object().unwrap() {
        p[k.parse::<usize>().unwrap()] = x.as_f64().unwrap();
    }
    p
}

fn bins<const N: usize>(v: &Value) -> [usize; N] {
    std::array::from_fn(|i| v[i].as_u64().unwrap() as usize)
}

fn noise(r: &mut Rng, n: usize, rms: f64) -> Vec<f64> {
    (0..n).map(|_| rms * r.gauss()).collect()
}

fn db(x: f64) -> f64 {
    10f64.powf(x / 20.0)
}

fn tone(n: usize, hz: f64, amp: f64) -> Vec<f64> {
    (0..n).map(|i| amp * (std::f64::consts::TAU * hz * i as f64 / 48000.0).sin()).collect()
}

fn run(b: &mut Busy, x: &[f64]) -> Vec<(usize, bool)> {
    let mut edges = Vec::new();
    for (i, c) in x.chunks(480).enumerate() {
        edges.extend(b.feed(c, false).into_iter().map(|e| (i * 480 + c.len(), e)));
    }
    edges
}

#[test]
fn tone_detector_matches_vector() {
    let v = load_doc("hf/busy");
    for c in v["detector1"]["cases"].as_array().unwrap() {
        let bw = bw_of(&c["bw"]);
        let p = spectrum(&c["spectrum"], 128);
        assert_eq!(tone_band(bw).0, c["bins"][0].as_u64().unwrap() as usize);
        assert_eq!(detect_tone(&p, bw), c["busy"].as_bool().unwrap(), "{c}");
    }
}

#[test]
fn signature_detector_matches_vector() {
    let v = load_doc("hf/busy");
    let d2 = &v["detector2"];
    for bw in Bandwidth::ALL {
        let want: Vec<Signature> = d2["signatures"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| bw_of(&s["bw"]) == bw)
            .map(|s| Signature {
                record: Record::new(s["record"].as_u64().unwrap() as usize).unwrap(),
                carriers: bins(&s["carrier_bins"]),
                gaps: bins(&s["gap_bins"]),
            })
            .collect();
        assert_eq!(signatures(bw), want, "{bw:?}");
    }
    for c in d2["cases"].as_array().unwrap() {
        let bw = bw_of(&c["bw"]);
        let sig = *signatures(bw).iter().find(|s| s.record.get() as u64 == c["record"].as_u64().unwrap()).unwrap();
        let s = spectrum(&c["S"], 64);
        assert_eq!(signature_threshold(bw), c["threshold"].as_f64().unwrap());
        assert_eq!(detect_signature(&s, &sig, signature_threshold(bw)), c["busy"].as_bool().unwrap(), "{c}");
    }
}

#[test]
fn silence_and_noise_rarely_trip() {
    for bw in Bandwidth::ALL {
        let mut b = Busy::new(bw);
        assert!(run(&mut b, &vec![0.0; 48000 * 3]).is_empty());
        let mut b = Busy::new(bw);
        let x = noise(&mut Rng::new(7), 48000 * 20, db(-15.0));
        run(&mut b, &x);
        assert!(b.hits[0] == 0 && b.hits[1] <= 3, "{bw:?} {:?}", b.hits);
    }
}

/// Gaussian noise whose level climbs `db_per_100hz` above 1000 Hz (flat below), as a radio's
/// passband or a noisy band edge gives it: shaped in the frequency domain, n samples (a power
/// of two).
fn sloped_noise(r: &mut Rng, n: usize, rms: f64, db_per_100hz: f64) -> Vec<f64> {
    use num_complex::Complex64;
    let mut spec: Vec<Complex64> = (0..n)
        .map(|k| {
            let hz = 48000.0 * k.min(n - k) as f64 / n as f64;
            let gain = db(db_per_100hz * ((hz - 1000.0).max(0.0) / 100.0).min(30.0));
            Complex64::new(r.gauss(), r.gauss()) * gain
        })
        .collect();
    rustfft::FftPlanner::new().plan_fft_inverse(n).process(&mut spec);
    let x: Vec<f64> = spec.iter().map(|c| c.re).collect();
    let now = (x.iter().map(|v| v * v).sum::<f64>() / n as f64).sqrt();
    x.iter().map(|v| v * rms / now).collect()
}

#[test]
fn sloped_noise_is_not_a_carrier() {
    // A radio whose noise climbs ~11 dB across the 500 Hz passband (measured on an FTDX3000's
    // USB audio, 2.3 dB per 100 Hz): comparing the strongest bin with the quietest made plain
    // noise look like a tone every few seconds. Once the noise shape is learned it doesn't, and
    // a real carrier on top of it still trips.
    let mut r = Rng::new(21);
    let mut b = Busy::new(Bandwidth::Hz500);
    let warm = sloped_noise(&mut r, 1 << 18, db(-35.0), 2.3);
    run(&mut b, &warm);
    let before = b.hits[0];
    let quiet = sloped_noise(&mut r, 1 << 19, db(-35.0), 2.3);
    run(&mut b, &quiet);
    assert_eq!(b.hits[0], before, "noise alone tripped the tone detector {:?}", b.hits);
    let carrier: Vec<f64> = sloped_noise(&mut r, 1 << 16, db(-35.0), 2.3)
        .iter()
        .zip(tone(1 << 16, 1400.0, db(-25.0)))
        .map(|(a, b)| a + b)
        .collect();
    run(&mut b, &carrier);
    assert!(b.hits[0] > before, "the carrier went unseen {:?}", b.hits);
}

#[test]
fn tone_in_noise_hangs_34_blocks() {
    let mut b = Busy::new(Bandwidth::Hz2300);
    let mut n = Rng::new(3);
    let mut x = noise(&mut n, 48000, db(-30.0));
    let burst: Vec<f64> =
        tone(9600, 1500.0, db(-20.0)).iter().zip(noise(&mut n, 9600, db(-30.0))).map(|(a, b)| a + b).collect();
    x.extend(burst);
    x.extend(noise(&mut n, 48000 * 3, db(-30.0)));
    let edges = run(&mut b, &x);
    assert_eq!(edges.iter().map(|e| e.1).collect::<Vec<_>>(), [true, false], "{edges:?}");
    let off = (edges[1].0 as f64 - 57600.0) / 48000.0;
    let hang = f64::from(HANG) * BLOCK48 as f64 / 48000.0;
    assert!((hang - 0.09..=hang + 0.05).contains(&off), "{off}");
    assert!(edges[0].0 < 48000 + 2 * BLOCK48 + 480);
}

#[test]
fn tone_threshold_is_24_db_over_the_floor() {
    let mut n = Rng::new(11);
    let sigma = db(-30.0);
    for (bin_db, want) in [(8.0, false), (30.0, true)] {
        let mut b = Busy::new(Bandwidth::Hz2300);
        let noise = noise(&mut n, 48000 * 2, sigma);
        let t = tone(noise.len(), 1500.0, sigma * (db(bin_db) * db(bin_db) / 1024.0).sqrt());
        let x: Vec<f64> = noise.iter().zip(t).map(|(a, b)| a + b).collect();
        run(&mut b, &x);
        assert_eq!(b.hits[0] > 0, want, "{bin_db} dB {:?}", b.hits);
    }
}

#[test]
fn quiet_tone_is_gated() {
    let mut b = Busy::new(Bandwidth::Hz2300);
    let mut n = Rng::new(13);
    let floor = |x: Vec<f64>, n: &mut Rng| -> Vec<f64> {
        x.iter().zip(noise(n, x.len(), db(-75.0))).map(|(a, b)| a + b).collect()
    };
    let quiet = floor(tone(48000 * 3, 1500.0, db(-52.0)), &mut n);
    assert!(run(&mut b, &quiet).is_empty(), "{:?}", b.hits);
    let edges = run(&mut b, &floor(tone(48000, 1500.0, db(-40.0)), &mut n));
    assert_eq!(edges.first().map(|e| e.1), Some(true));
}

#[test]
fn xara_psk_trips_signature_detector() {
    let mut rng = Rng::new(1);
    let mut rand = |n: u32| rng.below(n);
    for (bw, record) in [(Bandwidth::Hz2300, 5), (Bandwidth::Hz2300, 8), (Bandwidth::Hz2750, 7), (Bandwidth::Hz500, 4)]
        .map(|(bw, r)| (bw, Record::new(r).unwrap()))
    {
        let m = Modulator::new(bw);
        let len = bw.level(record).frame_bytes() as usize;
        let frame: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
        let x = m.transmission(record, &[&frame], &mut rand);
        let rms = (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
        let mut n = Rng::new(5);
        let sig: Vec<f64> =
            x.iter().map(|v| v / rms * db(-25.0)).zip(noise(&mut n, x.len(), db(-25.0))).map(|(a, b)| a + b).collect();
        let mut b = Busy::new(bw);
        run(&mut b, &noise(&mut n, 48000 * 3, db(-25.0)));
        assert!(!b.is_on() && b.hits == [0, 0]);
        run(&mut b, &sig);
        assert!(b.hits[1] > 0, "{bw:?} L{record} {:?}", b.hits);
        assert!(b.is_on());
    }
}

#[test]
fn forced_holds_and_releases_after_hang() {
    let mut b = Busy::new(Bandwidth::Hz2300);
    assert_eq!(b.feed(&[0.0; 480], true), [true]);
    for _ in 0..100 {
        assert!(b.feed(&[0.0; 480], true).is_empty());
    }
    let edges = run(&mut b, &vec![0.0; 48000 * 2]);
    assert_eq!(edges.len(), 1);
    assert!(!edges[0].1);
    let t = edges[0].0 as f64 / 48000.0;
    assert!((1.40..1.50).contains(&t), "{t}");
}
