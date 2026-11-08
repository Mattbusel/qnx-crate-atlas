#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let body = reqwest::get("https://example.com").await?.text().await?;
    println!("{} bytes", body.len());
    Ok(())
}
