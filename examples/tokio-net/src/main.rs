// Uses tokio's TCP listener so the net feature is linked, not just compiled.
#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = l.local_addr()?;
    let c = tokio::spawn(async move { tokio::net::TcpStream::connect(addr).await });
    let (_s, peer) = l.accept().await?;
    c.await.unwrap()?;
    println!("tokio net on QNX: accepted {peer}");
    Ok(())
}
