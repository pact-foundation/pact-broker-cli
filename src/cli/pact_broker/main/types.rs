use std::str::FromStr;

use pact_models::http_utils::HttpAuth;

#[derive(Clone)]
pub struct BrokerDetails {
    pub(crate) auth: Option<HttpAuth>,
    pub(crate) url: String,
    pub(crate) ssl_options: SslOptions,
}

impl BrokerDetails {
    pub fn from_args(
        args: &clap::ArgMatches,
    ) -> Result<Self, crate::cli::pact_broker::main::PactBrokerError> {
        use crate::cli::pact_broker::main::utils::{get_auth, get_broker_url, get_ssl_options};

        let url = get_broker_url(args).trim_end_matches('/').to_string();
        let auth = get_auth(args);
        let ssl_options = get_ssl_options(args);

        Ok(BrokerDetails {
            auth: Some(auth),
            url,
            ssl_options,
        })
    }
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
