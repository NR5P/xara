use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Ends a blocking accept loop when dropped, so its port is free again: it sets the flag the
/// loop checks after each accept and connects once to wake it. A process that exits never
/// needs this; a program that embeds the modem and starts it again does.
pub struct Closer {
    closed: Arc<AtomicBool>,
    wake: SocketAddr,
}

impl Closer {
    pub fn new(l: &TcpListener) -> io::Result<(Self, Arc<AtomicBool>)> {
        let mut wake = l.local_addr()?;
        if wake.ip().is_unspecified() {
            wake.set_ip(if wake.is_ipv4() { Ipv4Addr::LOCALHOST.into() } else { Ipv6Addr::LOCALHOST.into() });
        }
        let closed = Arc::new(AtomicBool::new(false));
        Ok((Self { closed: closed.clone(), wake }, closed))
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub fn flag(&self) -> Arc<AtomicBool> {
        self.closed.clone()
    }
}

impl Drop for Closer {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.wake, Duration::from_millis(200));
    }
}
