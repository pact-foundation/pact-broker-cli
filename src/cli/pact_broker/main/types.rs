use std::str::FromStr;

use pact_models::http_utils::HttpAuth;

#[derive(Clone)]
pub struct BrokerDetails {
    pub(crate) auth: Option<HttpAuth>,
    pub(crate) url: String,
    pub(crate) ssl_options: SslOptions,
}
#[derive(Clone)]
pub enum OutputType {
    Json,
    Table,
    Text,
    Pretty,
}

impl FromStr for OutputType {
    type Err = ();

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.to_lowercase().as_str() {
            "json" => Ok(OutputType::Json),
            "table" => Ok(OutputType::Table),
            "text" => Ok(OutputType::Text),
            "pretty" => Ok(OutputType::Pretty),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SslOptions {
    pub skip_ssl: bool,
    pub ssl_cert_path: Option<String>,
    pub use_root_trust_store: bool,
}

impl Default for SslOptions {
    fn default() -> Self {
        SslOptions {
            skip_ssl: false,
            ssl_cert_path: None,
            use_root_trust_store: true,
        }
    }
}
