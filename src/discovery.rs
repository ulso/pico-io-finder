use std::{collections::HashMap, net::IpAddr, time::Duration};

use mdns_sd_discovery::{
    BrowseEvent, DiscoveredService, ServiceBrowseError, ServiceBrowserBuilder,
};
use reqwest::{Client, redirect::Policy};
use thiserror::Error;
use tokio::sync::mpsc::UnboundedSender;

use crate::{ApiStatus, Device, RemovedDevice};

const HTTP_SERVICE_TYPE: &str = "_http._tcp";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const PROBE_RETRIES: usize = 3;
const PROBE_RETRY_DELAY: Duration = Duration::from_millis(200);
const PENDING_RETRY_INTERVAL: Duration = Duration::from_secs(2);
const LIVENESS_FAILURES: u8 = 3;
const MAX_STATUS_BYTES: usize = 64 * 1024;

struct Candidate {
    service: DiscoveredService,
    last_devices: Vec<Device>,
    failures: u8,
}

/// Updates emitted by the long-running DNS-SD browser.
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    Found(Device),
    Removed(RemovedDevice),
    Warning(String),
}

/// A fatal discovery setup error.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("failed to start native DNS-SD browsing: {0}")]
    Browse(#[from] mdns_sd_discovery::ServiceBrowseError),
    #[error("failed to construct HTTP client: {0}")]
    HttpClient(#[source] reqwest::Error),
}

/// Browse native DNS-SD continuously and verify matching HTTP services.
///
/// The function returns when DNS-SD stops or the receiver is dropped.
pub async fn run_discovery(events: UnboundedSender<DiscoveryEvent>) -> Result<(), DiscoveryError> {
    let client = Client::builder()
        .no_proxy()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(Policy::none())
        .build()
        .map_err(DiscoveryError::HttpClient)?;

    let mut builder = ServiceBrowserBuilder::new();
    builder.service_type(HTTP_SERVICE_TYPE);
    let mut browser = builder.browse().await?;
    let mut candidates = HashMap::<(String, Option<u32>), Candidate>::new();
    let mut retry = tokio::time::interval(PENDING_RETRY_INTERVAL);
    retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            event = browser.recv() => match event {
                Some(Ok(BrowseEvent::Found(service))) if is_pico_io_candidate(&service) => {
                    let key = service_key(&service);
                    candidates
                        .entry(key)
                        .and_modify(|candidate| candidate.service = service.clone())
                        .or_insert(Candidate {
                            service,
                            last_devices: Vec::new(),
                            failures: 0,
                        });
                }
                Some(Ok(BrowseEvent::Found(_))) => {}
                Some(Ok(BrowseEvent::Removed(service))) => {
                    let key = (service.name.clone(), service.interface_index.map(Into::into));
                    if candidates.remove(&key).is_some() {
                        let removed = RemovedDevice {
                            service_name: service.name,
                            interface_index: service.interface_index.map(Into::into),
                        };
                        if events.send(DiscoveryEvent::Removed(removed)).is_err() {
                            return Ok(());
                        }
                    }
                }
                Some(Err(ServiceBrowseError::ResolveFailed(name, _)))
                    if !is_pico_io_service_name(&name) => {}
                Some(Err(error)) => {
                    if events
                        .send(DiscoveryEvent::Warning(error.to_string()))
                        .is_err()
                    {
                        return Ok(());
                    }
                }
                None => return Ok(()),
            },
            _ = retry.tick(), if !candidates.is_empty() => {
                let services: Vec<_> = candidates.values().map(|candidate| candidate.service.clone()).collect();
                for service in services {
                    let key = service_key(&service);
                    match probe_service(&client, &service).await {
                        Ok(devices) => {
                            let Some(candidate) = candidates.get_mut(&key) else {
                                continue;
                            };
                            candidate.failures = 0;
                            if candidate.last_devices != devices {
                                candidate.last_devices = devices.clone();
                                for device in devices {
                                    if events.send(DiscoveryEvent::Found(device)).is_err() {
                                        return Ok(());
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            let Some(candidate) = candidates.get_mut(&key) else {
                                continue;
                            };
                            candidate.failures = candidate.failures.saturating_add(1);
                            if candidate.failures == 1 && candidate.last_devices.is_empty() {
                                let warning = format!(
                                    "{} was found, but its status endpoint has not answered yet. Retrying…",
                                    service.name
                                );
                                if events.send(DiscoveryEvent::Warning(warning)).is_err() {
                                    return Ok(());
                                }
                            }
                            if candidate.failures == LIVENESS_FAILURES
                                && !candidate.last_devices.is_empty()
                            {
                                candidate.last_devices.clear();
                                let removed = RemovedDevice {
                                    service_name: service.name.clone(),
                                    interface_index: service.interface_index.map(Into::into),
                                };
                                if events.send(DiscoveryEvent::Removed(removed)).is_err() {
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn service_key(service: &DiscoveredService) -> (String, Option<u32>) {
    (
        service.name.clone(),
        service.interface_index.map(Into::into),
    )
}

fn is_pico_io_candidate(service: &DiscoveredService) -> bool {
    let host = service.host_name.to_ascii_lowercase();
    is_pico_io_service_name(&service.name)
        || host.starts_with("pico-io-")
        || service
            .txt("device")
            .is_some_and(|value| value.starts_with(b"pico-io-"))
}

fn is_pico_io_service_name(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    name.starts_with("pico i/o ") || name.starts_with("pico-io")
}

async fn probe_service(
    client: &Client,
    service: &DiscoveredService,
) -> Result<Vec<Device>, String> {
    let mut devices = Vec::new();
    let mut last_error = None;

    if service.port == 0 {
        return Err(format!("{} advertised TCP port 0", service.name));
    }

    for address in &service.addresses {
        let IpAddr::V4(address) = address else {
            continue;
        };
        if !is_usable_address(*address) {
            continue;
        }
        let origin = if service.port == 80 {
            format!("http://{address}")
        } else {
            format!("http://{address}:{}", service.port)
        };
        let status_url = format!("{origin}/api/status");

        let mut result = None;
        for attempt in 0..PROBE_RETRIES {
            let response = request_status(client, &status_url).await;
            let succeeded = response.is_ok();
            result = Some(response);
            if succeeded || attempt + 1 == PROBE_RETRIES {
                break;
            }
            tokio::time::sleep(PROBE_RETRY_DELAY).await;
        }

        match result.expect("the positive retry count always produces a result") {
            Ok(status) if status.is_pico_io() => devices.push(Device {
                service_name: service.name.clone(),
                host_name: service.host_name.clone(),
                address: *address,
                port: service.port,
                interface_index: service.interface_index.map(Into::into),
                status,
            }),
            Ok(_) => last_error = Some(format!("{status_url} is not a Pico I/O device")),
            Err(error) => last_error = Some(format!("{status_url}: {error}")),
        }
    }

    if devices.is_empty() {
        Err(last_error.unwrap_or_else(|| {
            format!(
                "{} was discovered without a usable IPv4 address",
                service.name
            )
        }))
    } else {
        Ok(devices)
    }
}

fn is_usable_address(address: std::net::Ipv4Addr) -> bool {
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_multicast()
        && address != std::net::Ipv4Addr::BROADCAST
}

async fn request_status(client: &Client, status_url: &str) -> Result<ApiStatus, String> {
    let mut response = client
        .get(status_url)
        .header("Cache-Control", "no-cache")
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;

    if response
        .content_length()
        .is_some_and(|length| length > MAX_STATUS_BYTES as u64)
    {
        return Err(format!("response exceeds {MAX_STATUS_BYTES} bytes"));
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if body.len().saturating_add(chunk.len()) > MAX_STATUS_BYTES {
            return Err(format!("response exceeds {MAX_STATUS_BYTES} bytes"));
        }
        body.extend_from_slice(&chunk);
    }

    serde_json::from_slice(&body).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    fn service(name: &str, host: &str) -> DiscoveredService {
        DiscoveredService {
            name: name.to_owned(),
            service_type: HTTP_SERVICE_TYPE.to_owned(),
            domain: "local".to_owned(),
            host_name: host.to_owned(),
            port: 80,
            addresses: Vec::new(),
            txt_records: Vec::new(),
            interface_index: NonZeroU32::new(4),
        }
    }

    #[test]
    fn recognises_current_dns_sd_identity() {
        assert!(is_pico_io_candidate(&service(
            "Pico I/O Fruit Jam-CC6D4F",
            "pico-io-fruit-jam-cc6d4f.local"
        )));
    }

    #[test]
    fn ignores_unrelated_http_services() {
        assert!(!is_pico_io_candidate(&service(
            "Office printer",
            "printer.local"
        )));
    }

    #[test]
    fn recognises_only_pico_io_service_name_prefixes() {
        assert!(is_pico_io_service_name("Pico I/O Fruit Jam-CC6D4F"));
        assert!(is_pico_io_service_name("pico-io-bridge-244c29"));
        assert!(!is_pico_io_service_name("M33 - 5B51"));
        assert!(!is_pico_io_service_name("My pico-io test"));
    }

    #[test]
    fn rejects_addresses_that_should_not_be_probed() {
        assert!(!is_usable_address(std::net::Ipv4Addr::UNSPECIFIED));
        assert!(!is_usable_address(std::net::Ipv4Addr::LOCALHOST));
        assert!(!is_usable_address(std::net::Ipv4Addr::BROADCAST));
        assert!(!is_usable_address(std::net::Ipv4Addr::new(224, 0, 0, 1)));
        assert!(is_usable_address(std::net::Ipv4Addr::new(10, 97, 225, 1)));
    }
}
