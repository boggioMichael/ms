//! This PC's address on the local network, for the link the phone opens.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};

/// The IPv4 address this PC would use to reach the internet, which is the
/// one other devices on the same Wi-Fi reach it at. Connecting a UDP socket
/// sends nothing; it only asks the system which interface it would use.
pub fn lan_ipv4() -> Option<Ipv4Addr> {
    for target in ["192.0.2.1:9", "8.8.8.8:53", "10.255.255.255:9"] {
        let Ok(socket) = UdpSocket::bind(("0.0.0.0", 0)) else {
            continue;
        };
        if socket.connect(target).is_err() {
            continue;
        }
        if let Ok(addr) = socket.local_addr()
            && let IpAddr::V4(ip) = addr.ip()
            && !ip.is_loopback()
            && !ip.is_unspecified()
        {
            return Some(ip);
        }
    }
    None
}

/// Whether `ip` is an address only the local network can reach.
pub fn is_private(ip: Ipv4Addr) -> bool {
    ip.is_private()
        || ip.is_link_local()
        || ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses_are_recognised() {
        assert!(is_private(Ipv4Addr::new(192, 168, 1, 20)));
        assert!(is_private(Ipv4Addr::new(10, 0, 0, 5)));
        assert!(is_private(Ipv4Addr::new(100, 80, 0, 1)));
        assert!(!is_private(Ipv4Addr::new(8, 8, 8, 8)));
    }

    #[test]
    fn the_lan_address_is_not_loopback() {
        if let Some(ip) = lan_ipv4() {
            assert!(!ip.is_loopback());
        }
    }
}
