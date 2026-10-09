//! Builds Pact Broker URL paths from runtime values.

use crate::cli::pact_broker::main::PactBrokerError;

/// Extensions the Pact Broker strips from the end of a request path, converting
/// them into an `Accept` header. Its lookup is case-sensitive.
const STRIPPED_EXTENSIONS: [&str; 4] = [".json", ".csv", ".svg", ".yaml"];

/// Validates `value` as a single URL path segment and percent-encodes it.
///
/// `arg` names the value in the error message. `is_last` marks the final path
/// segment, where the broker would strip a file extension.
pub fn encode_path_value(arg: &str, value: &str, is_last: bool) -> Result<String, PactBrokerError> {
    let reason = if value.is_empty() {
        Some("empty values produce an empty URL path segment".to_string())
    } else if value == "." || value == ".." {
        Some("'.' and '..' are URL dot-segments".to_string())
    } else if is_last {
        STRIPPED_EXTENSIONS
            .iter()
            .find(|ext| value.ends_with(*ext))
            .map(|ext| {
                format!("the Pact Broker strips a trailing '{ext}' from the final URL path segment")
            })
    } else {
        None
    };
    match reason {
        Some(reason) => Err(PactBrokerError::InvalidPathValue(format!(
            "{arg} value '{value}' cannot be sent: {reason}"
        ))),
        None => Ok(urlencoding::encode(value).into_owned()),
    }
}

/// A broker URL path assembled from fixed segments and runtime values.
///
/// The base URL and `&'static str` literals are inserted raw; every value goes
/// through [`encode_path_value`].
#[derive(Debug)]
pub struct BrokerPath {
    base: String,
    segments: Vec<Segment>,
}

#[derive(Debug)]
enum Segment {
    Literal(&'static str),
    Value { arg: &'static str, value: String },
}

impl BrokerPath {
    /// `base` is the broker URL, or `""` for a path relative to the broker root.
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            segments: Vec::new(),
        }
    }

    pub fn literal(mut self, segment: &'static str) -> Self {
        self.segments.push(Segment::Literal(segment));
        self
    }

    pub fn value(mut self, arg: &'static str, value: &str) -> Self {
        self.segments.push(Segment::Value {
            arg,
            value: value.to_string(),
        });
        self
    }

    pub fn build(self) -> Result<String, PactBrokerError> {
        let last = self.segments.len().saturating_sub(1);
        let mut path = self.base;
        for (index, segment) in self.segments.iter().enumerate() {
            path.push('/');
            match segment {
                Segment::Literal(literal) => path.push_str(literal),
                Segment::Value { arg, value } => {
                    path.push_str(&encode_path_value(arg, value, index == last)?)
                }
            }
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err_message(result: Result<String, PactBrokerError>) -> String {
        match result {
            Err(PactBrokerError::InvalidPathValue(msg)) => msg,
            other => panic!("expected InvalidPathValue, got {other:?}"),
        }
    }

    #[test]
    fn encodes_reserved_and_non_ascii_characters() {
        let cases = [
            ("fix/foo", "fix%2Ffoo"),
            ("a?b", "a%3Fb"),
            ("a#b", "a%23b"),
            ("100%", "100%25"),
            ("fix%2Ffoo", "fix%252Ffoo"),
            ("a\\b", "a%5Cb"),
            ("my consumer", "my%20consumer"),
            ("a+b", "a%2Bb"),
            ("{x}", "%7Bx%7D"),
            ("café", "caf%C3%A9"),
            ("🦀", "%F0%9F%A6%80"),
            ("1.0.0", "1.0.0"),
            ("a-b_c~d", "a-b_c~d"),
        ];
        for (value, expected) in cases {
            assert_eq!(
                encode_path_value("--x", value, false).unwrap(),
                expected,
                "{value}"
            );
            assert_eq!(
                encode_path_value("--x", value, true).unwrap(),
                expected,
                "{value}"
            );
        }
    }

    #[test]
    fn rejects_empty_and_dot_segments_in_any_position() {
        for is_last in [false, true] {
            assert_eq!(
                err_message(encode_path_value("--tag", "", is_last)),
                "--tag value '' cannot be sent: empty values produce an empty URL path segment"
            );
            assert_eq!(
                err_message(encode_path_value("--pacticipant", "..", is_last)),
                "--pacticipant value '..' cannot be sent: '.' and '..' are URL dot-segments"
            );
            assert!(encode_path_value("--pacticipant", ".", is_last).is_err());
        }
        assert!(encode_path_value("--x", "...", true).is_ok());
    }

    #[test]
    fn rejects_stripped_extensions_only_in_final_segment() {
        for ext in [".json", ".csv", ".svg", ".yaml"] {
            let value = format!("release{ext}");
            assert_eq!(
                err_message(encode_path_value("--branch", &value, true)),
                format!(
                    "--branch value '{value}' cannot be sent: the Pact Broker strips a trailing '{ext}' from the final URL path segment"
                )
            );
            assert_eq!(encode_path_value("--branch", &value, false).unwrap(), value);
        }
        assert!(encode_path_value("--branch", "release.JSON", true).is_ok());
        assert!(encode_path_value("--branch", "release.yml", true).is_ok());
    }

    #[test]
    fn builds_paths_from_literals_and_values() {
        let path = BrokerPath::new("http://broker/ctx/")
            .literal("pacticipants")
            .value("--pacticipant", "my consumer")
            .literal("branches")
            .value("--branch", "fix/foo")
            .literal("versions")
            .value("--version", "1.0.0")
            .build()
            .unwrap();
        assert_eq!(
            path,
            "http://broker/ctx/pacticipants/my%20consumer/branches/fix%2Ffoo/versions/1.0.0"
        );
        assert_eq!(
            BrokerPath::new("")
                .literal("pacts")
                .value("--provider", "p")
                .build()
                .unwrap(),
            "/pacts/p"
        );
    }

    #[test]
    fn build_applies_final_segment_rule_to_last_segment_only() {
        assert!(
            BrokerPath::new("")
                .literal("branches")
                .value("--branch", "release.json")
                .literal("versions")
                .build()
                .is_ok()
        );
        assert!(
            BrokerPath::new("")
                .literal("tags")
                .value("--tag", "release.json")
                .build()
                .is_err()
        );
    }
}
