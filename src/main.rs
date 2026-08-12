#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use dioxus::prelude::*;
use pico_io_finder::{Device, DiscoveryEvent, run_discovery};
use tokio::sync::mpsc;

const STYLE: &str = r#"
:root { color-scheme: dark; font-family: Inter, ui-sans-serif, system-ui, sans-serif; }
* { box-sizing: border-box; }
body { margin: 0; background: #0d171f; color: #edf4fa; }
button { font: inherit; }
.page { max-width: 920px; margin: 0 auto; padding: 40px 28px 64px; }
.eyebrow { color: #91a5b5; font-size: 12px; font-weight: 750; letter-spacing: .14em; text-transform: uppercase; }
h1 { margin: 6px 0; font-size: 38px; }
.intro, .status { color: #aebdca; }
.toolbar { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin: 28px 0 16px; }
.grid { display: grid; gap: 14px; }
.device { border: 1px solid #344a5b; border-radius: 16px; padding: 20px; background: #15222c; }
.deviceHead { display: flex; justify-content: space-between; gap: 16px; align-items: flex-start; }
.device h2 { margin: 0 0 4px; font-size: 21px; }
.meta { color: #aebdca; font-size: 14px; }
.facts { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; margin: 18px 0; }
.fact { border-top: 1px solid #304451; padding-top: 10px; }
.fact span { display: block; color: #91a5b5; font-size: 12px; }
.fact strong { overflow-wrap: anywhere; }
.actions { display: flex; gap: 8px; flex-wrap: wrap; justify-content: flex-end; }
.open { border: 0; border-radius: 10px; padding: 10px 16px; color: #07120d; background: #4bd095; font-weight: 750; cursor: pointer; }
.open:hover { background: #68dda9; }
.open.secondary { color: #d6e4ed; background: #253947; }
.open.secondary:hover { background: #304b5e; }
.empty { border: 1px dashed #344a5b; border-radius: 16px; padding: 34px 20px; color: #aebdca; text-align: center; }
.warning { color: #ffbd7a; font-size: 13px; overflow-wrap: anywhere; }
@media (max-width: 620px) { .facts { grid-template-columns: 1fr; } .deviceHead { flex-direction: column; } }
"#;

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            dioxus::desktop::Config::new().with_window(
                dioxus::desktop::WindowBuilder::new()
                    .with_title("Pico I/O Finder")
                    .with_inner_size(dioxus::desktop::LogicalSize::new(920.0, 680.0)),
            ),
        )
        .launch(App);
}

#[component]
fn App() -> Element {
    let mut devices = use_signal(Vec::<Device>::new);
    let mut discovery_state = use_signal(|| "Starting native DNS-SD…".to_owned());
    let mut warning = use_signal(|| None::<String>);

    use_future(move || async move {
        let (events_tx, mut events_rx) = mpsc::unbounded_channel();
        let mut task = tokio::spawn(run_discovery(events_tx));
        discovery_state.set("Browsing for Pico I/O devices…".to_owned());

        loop {
            tokio::select! {
                result = &mut task => {
                    discovery_state.set(match result {
                        Ok(Ok(())) => "DNS-SD browser stopped.".to_owned(),
                        Ok(Err(error)) => format!("Discovery failed: {error}"),
                        Err(error) => format!("Discovery task failed: {error}"),
                    });
                    break;
                }
                event = events_rx.recv() => match event {
                    Some(DiscoveryEvent::Found(device)) => {
                        devices.with_mut(|items| {
                            if let Some(current) = items.iter_mut().find(|item| item.key() == device.key()) {
                                *current = device;
                            } else {
                                items.push(device);
                                items.sort_by(|left, right| left.status.serial.cmp(&right.status.serial));
                            }
                        });
                        warning.set(None);
                    }
                    Some(DiscoveryEvent::Removed(removed)) => {
                        devices.with_mut(|items| items.retain(|item| {
                            item.service_name != removed.service_name
                                || item.interface_index != removed.interface_index
                        }));
                    }
                    Some(DiscoveryEvent::Warning(message)) => warning.set(Some(message)),
                    None => break,
                }
            }
        }
    });

    let current_devices = devices.read().clone();
    let count = current_devices.len();
    let count_label = match count {
        0 => "No devices found".to_owned(),
        1 => "1 device found".to_owned(),
        _ => format!("{count} devices found"),
    };

    rsx! {
        style { {STYLE} }
        main { class: "page",
            p { class: "eyebrow", "Pico I/O desktop utility" }
            h1 { "Pico I/O Finder" }
            p { class: "intro", "Find Pico I/O devices with native DNS-SD and open them in your default browser." }

            div { class: "toolbar",
                strong { "{count_label}" }
                span { class: "status", "{discovery_state}" }
            }

            if let Some(message) = warning.read().as_ref() {
                p { class: "warning", "{message}" }
            }

            if current_devices.is_empty() {
                div { class: "empty",
                    "Connect a Pico I/O device and keep this window open. Discovery updates automatically."
                }
            } else {
                div { class: "grid",
                    for device in current_devices {
                        DeviceCard { key: "{device.status.serial}", device }
                    }
                }
            }
        }
    }
}

#[component]
fn DeviceCard(device: Device) -> Element {
    let open_url = device.open_url();
    let button_url = open_url.clone();
    let numeric_url = device.numeric_open_url();
    let numeric_button_url = numeric_url.clone();
    let title = if device.status.board.is_empty() {
        device.status.device.clone()
    } else {
        device.status.board.clone()
    };

    rsx! {
        article { class: "device",
            div { class: "deviceHead",
                div {
                    h2 { "{title}" }
                    div { class: "meta", "{device.service_name}" }
                }
                div { class: "actions",
                    button {
                        class: "open secondary",
                        onclick: move |_| {
                            let _ = webbrowser::open(&numeric_button_url);
                        },
                        "Open IP"
                    }
                    button {
                        class: "open",
                        onclick: move |_| {
                            let _ = webbrowser::open(&button_url);
                        },
                        "Open"
                    }
                }
            }
            div { class: "facts",
                div { class: "fact", span { "Serial" } strong { "{device.status.serial}" } }
                div { class: "fact", span { "Firmware" } strong { "{device.status.firmware}" } }
                div { class: "fact", span { "Address" } strong { "{open_url}" } }
            }
            div { class: "meta", "IP fallback: {numeric_url} · network: {device.status.network}" }
        }
    }
}
