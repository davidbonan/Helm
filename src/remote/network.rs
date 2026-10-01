//! Which network the Mac is on (specs/remote.md §3.4): the MAC of its default
//! gateway, read from `route` and `arp` — the SSID would need the location
//! permission.

use std::net::Ipv4Addr;
use std::process::Command;

/// The default gateway's MAC, lowercase with two digits per octet; `None` off any
/// network or when the gateway is not in the ARP cache.
pub fn current_gateway_mac() -> Option<String> {
    let route = run("/sbin/route", &["-n", "get", "default"])?;
    let gateway = gateway_ip(&route)?;
    mac_in_arp(&run("/usr/sbin/arp", &["-n", &gateway.to_string()])?)
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The `gateway:` line of `route -n get default`.
pub fn gateway_ip(route_output: &str) -> Option<Ipv4Addr> {
    route_output
        .lines()
        .find_map(|line| line.trim().strip_prefix("gateway:"))?
        .trim()
        .parse()
        .ok()
}

/// `? (192.168.1.1) at 38:6:e6:45:5e:10 on en0 …`: macOS drops the leading zero
/// of an octet, so the MAC is normalized.
pub fn mac_in_arp(arp_output: &str) -> Option<String> {
    let mac = arp_output.split(" at ").nth(1)?.split_whitespace().next()?;
    let octets: Vec<u8> = mac
        .split(':')
        .map(|octet| u8::from_str_radix(octet, 16).ok())
        .collect::<Option<_>>()?;
    (octets.len() == 6).then(|| {
        octets
            .iter()
            .map(|octet| format!("{octet:02x}"))
            .collect::<Vec<_>>()
            .join(":")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTE: &str = "   route to: default\ndestination: default\n       mask: default\n    gateway: 192.168.1.1\n  interface: en0\n";

    #[test]
    fn the_gateway_is_read_from_route() {
        assert_eq!(gateway_ip(ROUTE), Some(Ipv4Addr::new(192, 168, 1, 1)));
        assert_eq!(
            gateway_ip("route: writing to routing socket: not in table"),
            None
        );
    }

    #[test]
    fn the_mac_is_read_from_arp_with_two_digits_per_octet() {
        assert_eq!(
            mac_in_arp("? (192.168.1.1) at 38:6:e6:45:5e:10 on en0 ifscope [ethernet]\n")
                .as_deref(),
            Some("38:06:e6:45:5e:10")
        );
    }

    #[test]
    fn an_unresolved_gateway_has_no_mac() {
        assert_eq!(
            mac_in_arp("? (192.168.1.1) at (incomplete) on en0 ifscope [ethernet]\n"),
            None
        );
        assert_eq!(mac_in_arp("192.168.1.1 (192.168.1.1) -- no entry\n"), None);
    }
}
