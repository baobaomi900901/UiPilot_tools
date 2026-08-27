use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const IPV4_DENIED_CIDRS: [([u8; 4], u8); 18] = [
    ([0, 0, 0, 0], 8),
    ([10, 0, 0, 0], 8),
    ([100, 64, 0, 0], 10),
    ([127, 0, 0, 0], 8),
    ([169, 254, 0, 0], 16),
    ([172, 16, 0, 0], 12),
    ([192, 0, 0, 0], 24),
    ([192, 0, 2, 0], 24),
    ([192, 31, 196, 0], 24),
    ([192, 52, 193, 0], 24),
    ([192, 88, 99, 0], 24),
    ([192, 168, 0, 0], 16),
    ([192, 175, 48, 0], 24),
    ([198, 18, 0, 0], 15),
    ([198, 51, 100, 0], 24),
    ([203, 0, 113, 0], 24),
    ([224, 0, 0, 0], 4),
    ([240, 0, 0, 0], 4),
];

const IPV6_DENIED_CIDRS: [(u128, u8); 15] = [
    (0x00000000000000000000000000000000, 96),
    (0x0064ff9b000000000000000000000000, 96),
    (0x0064ff9b000100000000000000000000, 48),
    (0x01000000000000000000000000000000, 64),
    (0x01000000000000010000000000000000, 64),
    (0x20010000000000000000000000000000, 23),
    (0x20010db8000000000000000000000000, 32),
    (0x20020000000000000000000000000000, 16),
    (0x2620004f800000000000000000000000, 48),
    (0x3fff0000000000000000000000000000, 20),
    (0x5f000000000000000000000000000000, 16),
    (0xfc000000000000000000000000000000, 7),
    (0xfe800000000000000000000000000000, 10),
    (0xfec00000000000000000000000000000, 10),
    (0xff000000000000000000000000000000, 8),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AddressPolicyError {
    Empty,
    Denied,
}

fn ipv4_in_cidr(address: Ipv4Addr, base: [u8; 4], prefix: u8) -> bool {
    let address = u32::from(address);
    let base = u32::from(Ipv4Addr::from(base));
    let mask = u32::MAX << (32 - prefix);
    address & mask == base & mask
}

fn ipv6_in_cidr(address: Ipv6Addr, base: u128, prefix: u8) -> bool {
    let mask = u128::MAX << (128 - prefix);
    u128::from(address) & mask == base & mask
}

fn normalize_address(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(address)),
        address => address,
    }
}

pub(super) fn is_denied_address(address: IpAddr) -> bool {
    match normalize_address(address) {
        IpAddr::V4(address) => IPV4_DENIED_CIDRS
            .iter()
            .any(|(base, prefix)| ipv4_in_cidr(address, *base, *prefix)),
        IpAddr::V6(address) => IPV6_DENIED_CIDRS
            .iter()
            .any(|(base, prefix)| ipv6_in_cidr(address, *base, *prefix)),
    }
}

pub(super) fn validate_resolved_addresses(
    addresses: Vec<IpAddr>,
) -> Result<Vec<IpAddr>, AddressPolicyError> {
    if addresses.is_empty() {
        return Err(AddressPolicyError::Empty);
    }
    let normalized = addresses
        .into_iter()
        .map(normalize_address)
        .collect::<Vec<_>>();
    if normalized.iter().copied().any(is_denied_address) {
        return Err(AddressPolicyError::Denied);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::{is_denied_address, validate_resolved_addresses, AddressPolicyError};

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }

    #[test]
    fn network_address_policy_rejects_every_fixed_ipv4_cidr() {
        for value in [
            "0.0.0.0",
            "0.255.255.255",
            "10.0.0.0",
            "10.255.255.255",
            "100.64.0.0",
            "100.127.255.255",
            "127.0.0.0",
            "127.255.255.255",
            "169.254.0.0",
            "169.254.255.255",
            "172.16.0.0",
            "172.31.255.255",
            "192.0.0.0",
            "192.0.0.255",
            "192.0.2.0",
            "192.0.2.255",
            "192.31.196.0",
            "192.31.196.255",
            "192.52.193.0",
            "192.52.193.255",
            "192.88.99.0",
            "192.88.99.255",
            "192.168.0.0",
            "192.168.255.255",
            "192.175.48.0",
            "192.175.48.255",
            "198.18.0.0",
            "198.19.255.255",
            "198.51.100.0",
            "198.51.100.255",
            "203.0.113.0",
            "203.0.113.255",
            "224.0.0.0",
            "239.255.255.255",
            "240.0.0.0",
            "255.255.255.255",
        ] {
            assert!(is_denied_address(ip(value)), "{value}");
        }
        for value in ["1.1.1.1", "8.8.8.8", "93.184.216.34"] {
            assert!(!is_denied_address(ip(value)), "{value}");
        }
    }

    #[test]
    fn network_address_policy_rejects_every_fixed_ipv6_cidr() {
        for value in [
            "::",
            "::ffff:ffff",
            "64:ff9b::",
            "64:ff9b::ffff:ffff",
            "64:ff9b:1::",
            "64:ff9b:1:ffff:ffff:ffff:ffff:ffff",
            "100::",
            "100::ffff:ffff:ffff:ffff",
            "100:0:0:1::",
            "100:0:0:1:ffff:ffff:ffff:ffff",
            "2001::",
            "2001:1ff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:db8::",
            "2001:db8:ffff:ffff:ffff:ffff:ffff:ffff",
            "2002::",
            "2002:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            "2620:4f:8000::",
            "2620:4f:8000:ffff:ffff:ffff:ffff:ffff",
            "3fff::",
            "3fff:fff:ffff:ffff:ffff:ffff:ffff:ffff",
            "5f00::",
            "5f00:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            "fc00::",
            "fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            "fe80::",
            "febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            "fec0::",
            "feff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            "ff00::",
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
        ] {
            assert!(is_denied_address(ip(value)), "{value}");
        }
        for value in ["2001:4860:4860::8888", "2606:4700:4700::1111"] {
            assert!(!is_denied_address(ip(value)), "{value}");
        }
    }

    #[test]
    fn network_address_policy_normalizes_mapped_ipv4_and_rejects_mixed_answers() {
        assert!(is_denied_address(ip("::ffff:127.0.0.1")));
        assert!(!is_denied_address(ip("::ffff:8.8.8.8")));
        assert_eq!(
            validate_resolved_addresses(vec![ip("::ffff:8.8.8.8")]),
            Ok(vec![ip("8.8.8.8")])
        );
        assert_eq!(
            validate_resolved_addresses(Vec::<IpAddr>::new()),
            Err(AddressPolicyError::Empty)
        );
        assert_eq!(
            validate_resolved_addresses(vec![ip("8.8.8.8"), ip("10.0.0.1")]),
            Err(AddressPolicyError::Denied)
        );
        assert_eq!(
            validate_resolved_addresses(vec![ip("8.8.8.8"), ip("1.1.1.1")]),
            Ok(vec![ip("8.8.8.8"), ip("1.1.1.1")])
        );
    }
}
