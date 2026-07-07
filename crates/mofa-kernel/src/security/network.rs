/// Network security utilities (SSRF prevention).

use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use url::Url;

#[derive(Debug, Clone, Copy)]
pub struct NetworkSecurity;

impl NetworkSecurity {
    /// This performs DNS resolution using
    /// `ToSocketAddrs`. If resolution fails or yields no addresses, the URL is denied.
    pub fn is_url_allowed(url: &str) -> bool {
        let parsed = match Url::parse(url) {
            Ok(u) => u,
            Err(_) => return false,
        };

        match parsed.scheme() {
            "http" | "https" => {}
            _ => return false,
        }

        let host = match parsed.host_str() {
            Some(h) => h,
            None => return false,
        };

        if host.eq_ignore_ascii_case("metadata.google.internal") {
            return false;
        }

        // IP-literal URLs don't need DNS resolution.
        if let Ok(ip) = host.parse::<IpAddr>() {
            return !Self::is_blocked_ip(&ip);
        }

        let port = parsed
            .port()
            .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });

        let addrs = match (host, port).to_socket_addrs() {
            Ok(it) => it.collect::<Vec<SocketAddr>>(),
            Err(_) => return false,
        };

        if addrs.is_empty() {
            return false;
        }

        for addr in addrs {
            if Self::is_blocked_ip(&addr.ip()) {
                return false;
            }
        }

        true
    }

    /// Returns `true` if the IP is considered unsafe for outbound requests.
    pub fn is_blocked_ip(ip: &IpAddr) -> bool {
        match ip {
            IpAddr::V4(ipv4) => {
                ipv4.is_private()
                    || ipv4.is_loopback()
                    || ipv4.is_unspecified()
                    || ipv4.is_multicast()
                    || ipv4.is_link_local()
                    || ipv4.is_broadcast()
                    || ipv4.is_documentation()
                    || Self::is_cgnat_ipv4(*ipv4)
            }
            IpAddr::V6(ipv6) => {
                // Handle IPv4-mapped IPv6 addresses, e.g. ::ffff:127.0.0.1.
                if let Some(mapped) = ipv6.to_ipv4_mapped() {
                    return Self::is_blocked_ip(&IpAddr::V4(mapped));
                }

                ipv6.is_loopback()
                    || ipv6.is_unspecified()
                    || ipv6.is_multicast()
                    || ipv6.is_unique_local()
                    || ipv6.is_unicast_link_local()
                    || Self::is_documentation_ipv6(*ipv6)
            }
        }
    }

    fn is_cgnat_ipv4(ipv4: Ipv4Addr) -> bool {
        // 100.64.0.0/10
        let [a, b, ..] = ipv4.octets();
        a == 100 && (64..=127).contains(&b)
    }

    fn is_documentation_ipv6(ipv6: std::net::Ipv6Addr) -> bool {
        // 2001:db8::/32
        let seg = ipv6.segments();
        seg[0] == 0x2001 && seg[1] == 0x0db8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_loopback_ipv4_url() {
        assert!(!NetworkSecurity::is_url_allowed("http://127.0.0.1:8080/"));
    }

    #[test]
    fn blocks_loopback_ipv6_url() {
        assert!(!NetworkSecurity::is_url_allowed("http://[::1]:8080/"));
    }

    #[test]
    fn blocks_ipv4_mapped_loopback_ip() {
        let ip: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(NetworkSecurity::is_blocked_ip(&ip));
    }

    #[test]
    fn blocks_link_local_metadata_ip() {
        assert!(!NetworkSecurity::is_url_allowed("http://169.254.169.254/latest/meta-data/"));
    }

    #[test]
    fn blocks_metadata_hostname_case_insensitively() {
        assert!(!NetworkSecurity::is_url_allowed(
            "http://METADATA.GOOGLE.INTERNAL/"
        ));
    }

    #[test]
    fn blocks_non_http_scheme() {
        assert!(!NetworkSecurity::is_url_allowed("file:///etc/passwd"));
        assert!(!NetworkSecurity::is_url_allowed("ftp://example.com/"));
    }
}

