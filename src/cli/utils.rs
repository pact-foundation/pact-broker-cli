use std::str::FromStr;

use console::Style;
use tracing_core::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn setup_loggers(level: &str) {
    let log_level = match level {
        "none" => LevelFilter::OFF,
        _ => LevelFilter::from_str(level).unwrap_or(LevelFilter::INFO),
    };

    tracing_subscriber::registry()
        .with({
            if log_level != LevelFilter::OFF {
                Some(
                    tracing_subscriber::fmt::layer()
                        .compact()
                        .with_thread_names(true)
                        .with_level(true),
                )
            } else {
                None
            }
        })
        .with({
            if log_level != LevelFilter::OFF {
                Some(tracing_subscriber::filter::LevelFilter::from_level(
                    log_level.into_level().unwrap(),
                ))
            } else {
                None
            }
        })
        .try_init()
        .unwrap_or_else(|err| eprintln!("ERROR: Failed to initialise loggers - {err}"));
}

pub fn glob_value(v: String) -> Result<String, String> {
    match glob::Pattern::new(&v) {
        Ok(res) => Ok(res.to_string()),
        Err(err) => Err(format!("'{}' is not a valid glob pattern - {}", v, err)),
    }
}

pub const RED: Style = Style::new().red();
pub const GREEN: Style = Style::new().green();
pub const YELLOW: Style = Style::new().yellow();
pub const CYAN: Style = Style::new().cyan();

/// A simple [`dbg!`](https://doc.rust-lang.org/std/macro.dbg.html)-like macro to help debugging `reqwest` calls.
/// Uses `tracing::debug!` instead of `eprintln!`.
#[macro_export]
macro_rules! dbg_as_curl {
    ($req:expr) => {
        match $req {
            tmp => {
                match tmp.try_clone().map(|b| b.build()) {
                    Some(Ok(req)) => tracing::debug!("{}", crate::cli::utils::AsCurl::new(&req)),
                    Some(Err(err)) => tracing::debug!("*Error*: {}", err),
                    None => tracing::debug!("*Error*: request not cloneable",),
                }
                tmp
            }
        }
    };
}

/// A wrapper around a request that displays as a cURL command.
pub struct AsCurl<'a> {
    req: &'a reqwest::Request,
}

impl<'a> AsCurl<'a> {
    /// Construct an instance of `AsCurl` with the given request.
    pub fn new(req: &'a reqwest::Request) -> AsCurl<'a> {
        Self { req }
    }
}

impl<'a> std::fmt::Debug for AsCurl<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        <Self as std::fmt::Display>::fmt(self, f)
    }
}

impl<'a> std::fmt::Display for AsCurl<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let AsCurl { req } = *self;

        write!(f, "curl ")?;

        let method = req.method();
        if method != "GET" {
            write!(f, "-X {} ", method)?;
        }

        for (name, value) in req.headers() {
            let value = value
                .to_str()
                .expect("Headers must contain only visible ASCII characters")
                .replace("'", r"'\''");

            write!(f, "--header '{}: {}' ", name, value)?;
        }

        // Body
        if let Some(body) = req.body() {
            // Try to get bytes if possible
            if let Some(bytes) = body.as_bytes() {
                let s = String::from_utf8_lossy(bytes).replace("'", r"'\''");
                write!(f, "--data-raw '{}' ", s)?;
            } else {
                write!(
                    f,
                    "# NOTE: Body present but not shown (stream or unknown type) "
                )?;
            }
        }

        // URL
        write!(f, "'{}'", req.url().to_string().replace("'", "%27"))?;

        Ok(())
    }
}

#[cfg(test)]
mod debug_as_curl_tests {

    use crate::dbg_as_curl;

    fn compare(req: reqwest::RequestBuilder, result: &str) {
        let req = dbg_as_curl!(req);

        let req = req.build().unwrap();
        assert_eq!(format!("{}", super::AsCurl::new(&req)), result);
    }

    #[test]
    fn basic() {
        let client = reqwest::Client::new();

        compare(
            client.get("http://example.org"),
            "curl 'http://example.org/'",
        );
        compare(
            client.get("https://example.org"),
            "curl 'https://example.org/'",
        );
    }

    #[test]
    fn escape_url() {
        let client = reqwest::Client::new();

        compare(
            client.get("https://example.org/search?q='"),
            "curl 'https://example.org/search?q=%27'",
        );
    }

    #[test]
    fn bearer() {
        let client = reqwest::Client::new();

        compare(
            client.get("https://example.org").bearer_auth("foo"),
            "curl --header 'authorization: Bearer foo' 'https://example.org/'",
        );
    }

    #[test]
    fn escape_headers() {
        let client = reqwest::Client::new();

        compare(
            client.get("https://example.org").bearer_auth("test's"),
            r"curl --header 'authorization: Bearer test'\''s' 'https://example.org/'",
        );
    }

    // The body cannot be included as there is not API to retrieve its content.
    #[test]
    fn body() {
        let client = reqwest::Client::new();

        compare(
            client.get("https://example.org").body("test's"),
            r"curl --data-raw 'test'\''s' 'https://example.org/'",
        );
    }
}
