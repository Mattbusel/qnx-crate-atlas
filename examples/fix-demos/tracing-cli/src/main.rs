use clap::Parser;
use socket2::{Domain, Socket, TcpKeepalive, Type};
use std::time::Duration;
#[derive(Parser)]
struct A { #[arg(long, default_value = "127.0.0.1:8080")] addr: String }
fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();
    let a = A::parse();
    let s = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    s.set_tcp_keepalive(&TcpKeepalive::new().with_time(Duration::from_secs(30)))?;
    tracing::info!(addr = %a.addr, "keepalive socket ready");
    Ok(())
}
