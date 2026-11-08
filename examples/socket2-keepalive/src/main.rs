fn main() {
    let s = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).unwrap();
    s.set_keepalive(true).unwrap();
    let ka = socket2::TcpKeepalive::new().with_time(std::time::Duration::from_secs(30));
    s.set_tcp_keepalive(&ka).unwrap();
    println!("socket2 keepalive on QNX: {:?}, pid {}", s.keepalive().unwrap(), unsafe { libc::getpid() });
}
