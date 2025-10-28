//! List provider states for a provider

use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    types::{BrokerDetails, OutputType},
};
use clap::ArgMatches;
use comfy_table::{Table, presets::UTF8_FULL};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Provider state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderState {
    /// List of consumers that use this provider state
    pub consumers: Vec<String>,
    /// Name of the provider state
    pub name: String,
    /// Parameters for the provider state
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// Response structure for provider states endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatesResponse {
    /// List of provider states
    pub provider_states: Vec<ProviderState>,
}

/// List provider states for a given provider
pub fn list_provider_states(
    broker_details: &BrokerDetails,
    provider: &str,
    branch: Option<&str>,
    environment: Option<&str>,
    output_type: OutputType,
) -> Result<String, PactBrokerError> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| PactBrokerError::IoError(format!("Failed to create runtime: {}", e)))?;

    rt.block_on(async {
        let hal_client = HALClient::with_url(
            &broker_details.url,
            broker_details.auth.clone(),
            broker_details.ssl_options.clone(),
        );

        // Build the path based on provided parameters
        let path = build_provider_states_path(provider, branch, environment);

        // Fetch the provider states
        let response = hal_client.fetch(&path).await?;

        // Parse the response
        let provider_states: ProviderStatesResponse =
            serde_json::from_value(response).map_err(|e| {
                PactBrokerError::ContentError(format!("Failed to parse response: {}", e))
            })?;

        // Format output based on requested type
        match output_type {
            OutputType::Json => Ok(format_json_output(&provider_states)?),
            OutputType::Table => Ok(format_table_output(
                &provider_states,
                provider,
                branch,
                environment,
            )),
            OutputType::Text => Ok(format_table_output(
                &provider_states,
                provider,
                branch,
                environment,
            )),
            OutputType::Pretty => Ok(format_json_output(&provider_states)?),
        }
    })
}

/// Build the API path for provider states based on parameters
fn build_provider_states_path(
    provider: &str,
    branch: Option<&str>,
    environment: Option<&str>,
) -> String {
    let base_path = format!(
        "/pacts/provider/{}/provider-states",
        urlencoding::encode(provider)
    );

    match (branch, environment) {
        (Some(branch_name), None) => {
            format!("{}/branch/{}", base_path, urlencoding::encode(branch_name))
        }
        (None, Some(env_name)) => {
            format!(
                "{}/environment/{}",
                base_path,
                urlencoding::encode(env_name)
            )
        }
        (None, None) => base_path,
        (Some(_), Some(_)) => {
            // Both branch and environment specified - this is an error case
            // We'll default to main branch behavior
            base_path
        }
    }
}

/// Format provider states as JSON
fn format_json_output(provider_states: &ProviderStatesResponse) -> Result<String, PactBrokerError> {
    serde_json::to_string_pretty(provider_states)
        .map_err(|e| PactBrokerError::ContentError(format!("Failed to serialize JSON: {}", e)))
}

/// Format provider states as a table
fn format_table_output(
    provider_states: &ProviderStatesResponse,
    provider: &str,
    branch: Option<&str>,
    environment: Option<&str>,
) -> String {
    
    let mut output = String::new();

    // Add header information
    output.push_str(&format!("Provider States for Provider: {}\n", provider));

    if let Some(branch_name) = branch {
        output.push_str(&format!("Branch: {}\n", branch_name));
    } else if let Some(env_name) = environment {
        output.push_str(&format!("Environment: {}\n", env_name));
    } else {
        output.push_str("Scope: Main branch\n");
    }

    output.push('\n');

    if provider_states.provider_states.is_empty() {
        output.push_str("No provider states found.\n");
        return output;
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Provider State Name", "Consumers", "Parameters"]);

    for state in &provider_states.provider_states {
        let consumers_str = state.consumers.join(", ");
        let params_str = match &state.params {
            Some(params) => {
                serde_json::to_string(params).unwrap_or_else(|_| "{}".to_string())
            }
            None => "".to_string(),
        };

        table.add_row(vec![&state.name, &consumers_str, &params_str]);
    }

    output.push_str(&table.to_string());
    output
}

/// CLI handler for list provider states command
pub fn handle_list_provider_states_command(args: &ArgMatches) -> Result<String, PactBrokerError> {
    let provider = args.get_one::<String>("provider").ok_or_else(|| {
        PactBrokerError::ValidationError(vec!["Provider name is required".to_string()])
    })?;

    let branch = args.get_one::<String>("branch").map(|s| s.as_str());
    let environment = args.get_one::<String>("environment").map(|s| s.as_str());

    // Validate that both branch and environment are not specified
    if branch.is_some() && environment.is_some() {
        return Err(PactBrokerError::ValidationError(vec![
            "Cannot specify both branch and environment. Please specify only one.".to_string(),
        ]));
    }

    let output_type = if args.get_flag("json") {
        OutputType::Json
    } else {
        OutputType::Table
    };

    let broker_details = BrokerDetails::from_args(args)?;

    list_provider_states(&broker_details, provider, branch, environment, output_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
use comfy_table::{Table, presets::UTF8_FULL};
    #[test]
    fn test_build_provider_states_path() {
        // Test main branch path
        assert_eq!(
            build_provider_states_path("MyProvider", None, None),
            "/pacts/provider/MyProvider/provider-states"
        );

        // Test branch-specific path
        assert_eq!(
            build_provider_states_path("MyProvider", Some("feature-branch"), None),
            "/pacts/provider/MyProvider/provider-states/branch/feature-branch"
        );

        // Test environment-specific path
        assert_eq!(
            build_provider_states_path("MyProvider", None, Some("production")),
            "/pacts/provider/MyProvider/provider-states/environment/production"
        );

        // Test URL encoding
        assert_eq!(
            build_provider_states_path("My Provider", Some("feature/branch"), None),
            "/pacts/provider/My%20Provider/provider-states/branch/feature%2Fbranch"
        );
    }

    #[test]
    fn test_format_json_output() {
        let provider_states = ProviderStatesResponse {
            provider_states: vec![ProviderState {
                consumers: vec!["Consumer1".to_string(), "Consumer2".to_string()],
                name: "test state".to_string(),
                params: Some(json!({"id": "123"})),
            }],
        };

        let result = format_json_output(&provider_states);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("test state"));
    }

    #[test]
    fn test_format_table_output() {
        let provider_states = ProviderStatesResponse {
            provider_states: vec![ProviderState {
                consumers: vec!["Consumer1".to_string()],
                name: "test state".to_string(),
                params: None,
            }],
        };

        let result = format_table_output(&provider_states, "TestProvider", None, None);
        assert!(result.contains("TestProvider"));
        assert!(result.contains("test state"));
        assert!(result.contains("Consumer1"));
    }
}
