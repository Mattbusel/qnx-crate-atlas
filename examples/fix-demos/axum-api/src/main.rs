use axum::{routing::get, Json, Router};
#[tokio::main]
async fn main() {
    let app = Router::new().route("/health", get(|| async { Json(serde_json::json!({"ok": true})) }));
    let l = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(l, app).await.unwrap();
}
