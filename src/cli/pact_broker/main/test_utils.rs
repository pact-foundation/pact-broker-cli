use pact_consumer::prelude::JsonPattern;

// Merge two serde_json::Value objects (both are objects)
pub fn merge_json_objects(a: &mut JsonPattern, b: &serde_json::Value) {
    if let (JsonPattern::Object(a_map), serde_json::Value::Object(b_map)) = (a, b) {
        for (k, v) in b_map {
            a_map.insert(
                k.clone(),
                pact_consumer::patterns::JsonPattern::Json(v.clone()),
            );
        }
    }
}

/// Starts an HTTP server that answers every request with an empty HAL document
/// and records `"{METHOD} {raw path and query}"` for each request.
///
/// Commands create their own Tokio runtime, so the server runs on its own thread.
pub fn spawn_recording_broker() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use axum::http::{Method, Uri};

    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = requests.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(listener.local_addr().unwrap()).unwrap();
                let router = axum::Router::new().fallback(move |method: Method, uri: Uri| {
                    let recorded = recorded.clone();
                    async move {
                        let target = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("");
                        recorded.lock().unwrap().push(format!("{method} {target}"));
                        (
                            [("content-type", "application/hal+json")],
                            "{\"_links\":{}}",
                        )
                    }
                });
                axum::serve(listener, router).await.unwrap();
            });
    });
    (format!("http://{}", rx.recv().unwrap()), requests)
}
