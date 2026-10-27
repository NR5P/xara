use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for round in 0..2 {
        let m = xara_ffi::start(args.clone()).expect("start");
        std::thread::sleep(Duration::from_secs(3));
        assert!(m.running(), "round {round}: modem stopped early");
        for p in [18300, 18301, 18100, 18080] {
            drop(std::net::TcpStream::connect(("127.0.0.1", p)));
        }
        std::thread::sleep(Duration::from_millis(500));
        let t = std::time::Instant::now();
        println!("round {round}: stop -> {} in {:?}", m.stop(), t.elapsed());
        std::thread::sleep(Duration::from_millis(500));
        for p in [18300, 18301, 18100, 18080] {
            let open =
                std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], p).into(), Duration::from_millis(200)).is_ok();
            println!("  port {p}: {}", if open { "STILL OPEN" } else { "closed" });
        }
    }
}
