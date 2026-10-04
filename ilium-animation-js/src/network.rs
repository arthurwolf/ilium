//! Pure origin/address checks. The broker must pin the validated DNS answer
//! into its connector and recheck every redirect; validation alone is not I/O.
use crate::error::{AnimationError, Result};
use std::net::{IpAddr, Ipv4Addr};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkAddressClass {
    Public,
    Local,
    Forbidden,
}

/// Canonical exact HTTPS origin, suitable for permission scope comparison.
pub fn validate_https_origin(input: &str) -> Result<String> {
    if input.len() > 2048
        || input
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
    {
        return Err(AnimationError::PermissionDenied("invalid origin".into()));
    }
    let url = Url::parse(input)
        .map_err(|_| AnimationError::PermissionDenied("invalid origin URL".into()))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || url
            .host_str()
            .is_none_or(|host| host.contains('*') || host.ends_with('.'))
    {
        return Err(AnimationError::PermissionDenied(
            "scope must be an exact HTTPS origin".into(),
        ));
    }
    Ok(url.origin().ascii_serialization())
}

/// Classify actual resolved addresses, including IPv4-mapped IPv6.
/// Local access requires network.local in addition to the HTTPS origin grant.
pub fn classify_address(address: IpAddr) -> NetworkAddressClass {
    match address {
        IpAddr::V4(address) => classify_v4(address),
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return classify_v4(mapped);
            }
            let segments = address.segments();
            if address.is_loopback()
                || address.is_unspecified()
                || segments[0] & 0xfe00 == 0xfc00
                || segments[0] & 0xffc0 == 0xfe80
            {
                return NetworkAddressClass::Local;
            }
            // Only ordinary global unicast. Transition and translation ranges
            // are denied so embedded private IPv4 addresses cannot bypass grants.
            if segments[0] & 0xe000 != 0x2000
                || segments[0] == 0x2002
                || (segments[0] == 0x2001
                    && (segments[1] == 0
                        || segments[1] == 0x0db8
                        || segments[1] & 0xfff0 == 0x0010))
            {
                return NetworkAddressClass::Forbidden;
            }
            NetworkAddressClass::Public
        }
    }
}

fn classify_v4(address: Ipv4Addr) -> NetworkAddressClass {
    let value = u32::from(address);
    let contains = |network: [u8; 4], bits: u32| {
        let mask = u32::MAX << (32 - bits);
        value & mask == u32::from(Ipv4Addr::from(network)) & mask
    };
    for (network, bits) in [
        ([10, 0, 0, 0], 8),
        ([100, 64, 0, 0], 10),
        ([127, 0, 0, 0], 8),
        ([169, 254, 0, 0], 16),
        ([172, 16, 0, 0], 12),
        ([192, 168, 0, 0], 16),
    ] {
        if contains(network, bits) {
            return NetworkAddressClass::Local;
        }
    }
    for (network, bits) in [
        ([0, 0, 0, 0], 8),
        ([192, 0, 0, 0], 24),
        ([192, 0, 2, 0], 24),
        ([192, 88, 99, 0], 24),
        ([198, 18, 0, 0], 15),
        ([198, 51, 100, 0], 24),
        ([203, 0, 113, 0], 24),
        ([224, 0, 0, 0], 4),
        ([240, 0, 0, 0], 4),
    ] {
        if contains(network, bits) {
            return NetworkAddressClass::Forbidden;
        }
    }
    NetworkAddressClass::Public
}
