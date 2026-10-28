//! `cargo run -p sawit-core --example probe -- 5433` explains who listens on a port.

use sawit_core::ports::BindAddr;
use sawit_core::probe::check_port;

#[tokio::main]
async fn main() {
    let port = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(5432);
    let report = check_port(port, &[BindAddr::LoopbackV4, BindAddr::LoopbackV6], vec![])
        .await
        .expect("probe failed");
    println!("{}", report.explain());
}
