//! The address the phone server binds to (specs/remote.md §3.1): one private
//! IPv4 of an up interface, `en0` first — never `0.0.0.0`, so a VPN or a second
//! interface stays unexposed.

use std::ffi::CStr;
use std::net::Ipv4Addr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
    pub addr: Ipv4Addr,
    pub is_up: bool,
}

const PREFERRED: &str = "en0";

pub fn lan_address(interfaces: &[Interface]) -> Option<Ipv4Addr> {
    let candidates = || {
        interfaces
            .iter()
            .filter(|i| i.is_up && i.addr.is_private() && !i.addr.is_loopback())
    };
    candidates()
        .find(|i| i.name == PREFERRED)
        .or_else(|| candidates().next())
        .map(|i| i.addr)
}

/// The machine's IPv4 interfaces (`getifaddrs`).
pub fn interfaces() -> Vec<Interface> {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut cursor = head;
    while let Some(entry) = unsafe { cursor.as_ref() } {
        if let Some(addr) = ipv4_of(entry) {
            found.push(Interface {
                name: unsafe { CStr::from_ptr(entry.ifa_name) }
                    .to_string_lossy()
                    .into_owned(),
                addr,
                is_up: entry.ifa_flags & UP_AND_RUNNING == UP_AND_RUNNING,
            });
        }
        cursor = entry.ifa_next;
    }
    unsafe { libc::freeifaddrs(head) };
    found
}

const UP_AND_RUNNING: u32 = (libc::IFF_UP | libc::IFF_RUNNING) as u32;

fn ipv4_of(entry: &libc::ifaddrs) -> Option<Ipv4Addr> {
    let sockaddr = unsafe { entry.ifa_addr.as_ref() }?;
    if i32::from(sockaddr.sa_family) != libc::AF_INET {
        return None;
    }
    let inet = unsafe { &*entry.ifa_addr.cast::<libc::sockaddr_in>() };
    Some(Ipv4Addr::from(u32::from_be(inet.sin_addr.s_addr)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(name: &str, addr: &str) -> Interface {
        Interface {
            name: name.to_owned(),
            addr: addr.parse().unwrap(),
            is_up: true,
        }
    }

    #[test]
    fn en0_wins_over_another_private_interface() {
        let interfaces = [up("bridge100", "192.168.64.1"), up("en0", "192.168.1.20")];

        assert_eq!(
            lan_address(&interfaces),
            Some("192.168.1.20".parse().unwrap())
        );
    }

    #[test]
    fn without_en0_the_first_private_up_interface_is_taken() {
        let mut down = up("en1", "10.0.0.5");
        down.is_up = false;
        let interfaces = [
            up("lo0", "127.0.0.1"),
            up("utun3", "100.101.102.103"),
            down,
            up("en7", "172.16.4.2"),
        ];

        assert_eq!(
            lan_address(&interfaces),
            Some("172.16.4.2".parse().unwrap())
        );
    }

    #[test]
    fn no_private_address_means_no_local_network() {
        assert_eq!(
            lan_address(&[up("lo0", "127.0.0.1"), up("en0", "8.8.8.8")]),
            None
        );
    }

    #[test]
    fn the_loopback_interface_is_listed() {
        assert!(
            interfaces().iter().any(|i| i.addr.is_loopback()),
            "getifaddrs is read and decoded"
        );
    }
}
