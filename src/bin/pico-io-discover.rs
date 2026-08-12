use pico_io_finder::{DiscoveryEvent, run_discovery};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let timeout = parse_timeout();
    let deadline = timeout.map(|duration| tokio::time::Instant::now() + duration);
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();
    let mut discovery = tokio::spawn(run_discovery(events_tx));

    println!("Browsing for Pico I/O HTTP services. Press Ctrl-C to stop.");

    loop {
        tokio::select! {
            result = &mut discovery => {
                match result {
                    Ok(Ok(())) => println!("DNS-SD browser stopped."),
                    Ok(Err(error)) => eprintln!("Discovery failed: {error}"),
                    Err(error) => eprintln!("Discovery task failed: {error}"),
                }
                break;
            }
            event = events_rx.recv() => match event {
                Some(DiscoveryEvent::Found(device)) => println!(
                    "+ {} [{}] {} [IP {}] ({})",
                    device.status.board,
                    device.status.serial,
                    device.open_url(),
                    device.numeric_open_url(),
                    device.service_name,
                ),
                Some(DiscoveryEvent::Removed(device)) => {
                    println!("- {}", device.service_name);
                }
                Some(DiscoveryEvent::Warning(message)) => eprintln!("! {message}"),
                None => break,
            },
            _ = tokio::signal::ctrl_c() => {
                println!("Stopping.");
                discovery.abort();
                break;
            }
            _ = async {
                match deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => core::future::pending().await,
                }
            } => {
                println!("Discovery timeout reached.");
                discovery.abort();
                break;
            }
        }
    }
}

fn parse_timeout() -> Option<std::time::Duration> {
    let mut args = std::env::args().skip(1);
    let argument = args.next()?;
    if argument != "--timeout-seconds" {
        eprintln!("unknown argument: {argument}");
        std::process::exit(2);
    }
    let seconds = args
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or_else(|| {
            eprintln!("--timeout-seconds requires a positive integer");
            std::process::exit(2);
        });
    if let Some(argument) = args.next() {
        eprintln!("unexpected argument: {argument}");
        std::process::exit(2);
    }
    Some(std::time::Duration::from_secs(seconds))
}
