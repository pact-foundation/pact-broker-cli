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
    spawn_recording_broker_with_index("{}")
}

/// As [`spawn_recording_broker`], but `GET /` answers with `index_links` as the
/// `_links` object, with every `BASE` in it replaced by the server's base URL.
pub fn spawn_recording_broker_with_index(
    index_links: &str,
) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    spawn_recording_broker_with_responses(&[("/", &format!("{{\"_links\":{index_links}}}"))])
}

/// As [`spawn_recording_broker`], but a `GET` of a path in `responses` answers
/// with its body, with every `BASE` in it replaced by the server's base URL.
/// Paths are matched raw, without the query string.
pub fn spawn_recording_broker_with_responses(
    responses: &[(&str, &str)],
) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use axum::http::{Method, Uri};

    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = requests.clone();
    let responses: Vec<(String, String)> = responses
        .iter()
        .map(|(path, body)| (path.to_string(), body.to_string()))
        .collect();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let base = format!("http://{}", listener.local_addr().unwrap());
                let responses: std::collections::HashMap<String, String> = responses
                    .into_iter()
                    .map(|(path, body)| (path, body.replace("BASE", &base)))
                    .collect();
                tx.send(base).unwrap();
                let router = axum::Router::new().fallback(move |method: Method, uri: Uri| {
                    let recorded = recorded.clone();
                    let body = responses
                        .get(uri.path())
                        .filter(|_| method == Method::GET)
                        .cloned()
                        .unwrap_or_else(|| "{\"_links\":{}}".to_string());
                    async move {
                        let target = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("");
                        recorded.lock().unwrap().push(format!("{method} {target}"));
                        ([("content-type", "application/hal+json")], body)
                    }
                });
                axum::serve(listener, router).await.unwrap();
            });
    });
    (rx.recv().unwrap(), requests)
}
