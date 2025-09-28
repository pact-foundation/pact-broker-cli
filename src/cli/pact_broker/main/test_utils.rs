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
