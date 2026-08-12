use std::net::Ipv4Addr;

use serde::Deserialize;

/// The stable identity fields returned by `/api/status`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiStatus {
    pub device: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub board: String,
    pub serial: String,
    #[serde(default)]
    pub firmware: String,
    #[serde(default)]
    pub network: String,
}

impl ApiStatus {
    pub(crate) fn is_pico_io(&self) -> bool {
        matches!(self.device.as_str(), "pico-io-fruit-jam" | "pico-io-bridge")
            && !self.board.is_empty()
            && !self.serial.is_empty()
            && !self.firmware.is_empty()
    }
}

/// A verified Pico I/O HTTP endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub service_name: String,
    pub host_name: String,
    pub address: Ipv4Addr,
    pub port: u16,
    pub interface_index: Option<u32>,
    pub status: ApiStatus,
}

impl Device {
    /// A stable key suitable for deduplicating discovery updates.
    pub fn key(&self) -> &str {
        &self.status.serial
    }

    /// The numeric origin retained as a resolver-independent fallback.
    pub fn numeric_origin(&self) -> String {
        if self.port == 80 {
            format!("http://{}", self.address)
        } else {
            format!("http://{}:{}", self.address, self.port)
        }
    }

    /// The advertised mDNS origin, if the service supplied a hostname.
    pub fn mdns_origin(&self) -> Option<String> {
        let host_name = self.host_name.trim().trim_end_matches('.');
        if host_name.is_empty() {
            None
        } else if self.port == 80 {
            Some(format!("http://{host_name}"))
        } else {
            Some(format!("http://{host_name}:{}", self.port))
        }
    }

    /// The preferred origin for browser navigation.
    pub fn origin(&self) -> String {
        self.mdns_origin().unwrap_or_else(|| self.numeric_origin())
    }

    /// The device's built-in start page, preferably addressed by mDNS name.
    pub fn open_url(&self) -> String {
        format!("{}/", self.origin())
    }

    /// The device's built-in start page addressed by numeric IPv4 address.
    pub fn numeric_open_url(&self) -> String {
        format!("{}/", self.numeric_origin())
    }
}

/// DNS-SD identity available when a service disappears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedDevice {
    pub service_name: String,
    pub interface_index: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(host_name: &str, port: u16) -> Device {
        Device {
            service_name: "Pico I/O Test".to_owned(),
            host_name: host_name.to_owned(),
            address: Ipv4Addr::new(10, 17, 31, 1),
            port,
            interface_index: Some(7),
            status: ApiStatus {
                device: "pico-io-bridge".to_owned(),
                manufacturer: "Pico I/O".to_owned(),
                board: "Test board".to_owned(),
                serial: "ABC123".to_owned(),
                firmware: "0.1.0".to_owned(),
                network: "cdc-ncm".to_owned(),
            },
        }
    }

    #[test]
    fn accepts_current_product_identity() {
        let status: ApiStatus = serde_json::from_str(
            r#"{"device":"pico-io-fruit-jam","board":"Adafruit Fruit Jam","serial":"E6AF","firmware":"0.1.0","network":"cdc-ncm"}"#,
        )
        .unwrap();

        assert!(status.is_pico_io());
    }

    #[test]
    fn rejects_unrelated_http_service() {
        let status: ApiStatus =
            serde_json::from_str(r#"{"device":"printer","serial":"123"}"#).unwrap();

        assert!(!status.is_pico_io());
    }

    #[test]
    fn rejects_incomplete_identity() {
        let status: ApiStatus = serde_json::from_str(
            r#"{"device":"pico-io-fruit-jam","serial":"E6AF","firmware":"0.1.0"}"#,
        )
        .unwrap();

        assert!(!status.is_pico_io());
    }

    #[test]
    fn prefers_mdns_name_for_browser_navigation() {
        let device = device("pico-io-test.local.", 80);

        assert_eq!(device.open_url(), "http://pico-io-test.local/");
        assert_eq!(device.numeric_open_url(), "http://10.17.31.1/");
    }

    #[test]
    fn preserves_nonstandard_port_in_both_urls() {
        let device = device("pico-io-test.local", 8080);

        assert_eq!(device.open_url(), "http://pico-io-test.local:8080/");
        assert_eq!(device.numeric_open_url(), "http://10.17.31.1:8080/");
    }

    #[test]
    fn falls_back_to_numeric_url_without_hostname() {
        let device = device("  ", 80);

        assert_eq!(device.open_url(), "http://10.17.31.1/");
    }
}
