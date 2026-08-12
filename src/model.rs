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

    /// The numeric origin used to avoid browser `.local` resolver problems.
    pub fn origin(&self) -> String {
        if self.port == 80 {
            format!("http://{}", self.address)
        } else {
            format!("http://{}:{}", self.address, self.port)
        }
    }

    /// The device's built-in start page.
    pub fn open_url(&self) -> String {
        format!("{}/", self.origin())
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
}
