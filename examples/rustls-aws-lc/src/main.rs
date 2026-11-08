fn main() {
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let suites: Vec<String> = provider.cipher_suites.iter().map(|s| format!("{:?}", s.suite())).collect();
    let mut key = [0u8; 32];
    rustls::crypto::aws_lc_rs::default_provider().secure_random.fill(&mut key).unwrap();
    println!("rustls + aws-lc-rs on QNX: {} cipher suites, first {}, random byte {}", suites.len(), suites[0], key[0]);
}
