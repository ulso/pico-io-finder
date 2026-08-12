# Pico I/O Finder

Native desktop discovery for Pico I/O devices.

The application browses `_http._tcp.local` through the operating system's
DNS-SD implementation, verifies candidates using `/api/status`, and opens the
device's numeric IP address in the default browser. The browser therefore does
not need to resolve a `.local` hostname itself.

## Development

Requirements:

- Rust 1.96 or newer
- Dioxus CLI 0.7.3 for `dx` commands
- macOS 10.12+, Windows 10+, or Linux with a running Avahi daemon

Run the discovery-only CLI first:

```sh
cargo run --no-default-features --bin pico-io-discover
```

For a bounded smoke test:

```sh
cargo run --no-default-features --bin pico-io-discover -- --timeout-seconds 10
```

Run the desktop application:

```sh
cargo run --bin pico-io-finder
```

Or with Dioxus hot reload:

```sh
dx serve --desktop
```

## Current scope

- Native DNS-SD discovery on macOS, Windows, and Linux
- HTTP identity verification through `/api/status`
- Deduplication by device serial number
- Opening the numeric device address in the system browser

The first milestone intentionally does not scan network ranges or enumerate USB
devices. A direct CDC-NCM probe can be added later if native DNS-SD proves
unreliable on a supported platform.

Before distributing a macOS bundle, add `NSLocalNetworkUsageDescription` and
`NSBonjourServices` (`_http._tcp`) to its `Info.plist`, then sign and notarize
the application. Development builds may prompt separately through the local
network privacy dialog and a third-party firewall such as LuLu.
