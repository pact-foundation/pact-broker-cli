use base64::Engine;
use base64::engine::general_purpose::STANDARD as Base64;
use clap::ArgMatches;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;

use crate::cli::{
    pact_broker::main::{
        HALClient, Notice, PactBrokerError, process_notices,
        utils::{
            get_auth, get_broker_relation, get_broker_url, get_custom_headers, get_retries,
            get_ssl_options,
        },
    },
    utils::git_info,
};

/// One entry from a `--contract name=X,file=Y,...` flag.
#[derive(Debug, Clone, PartialEq)]
pub struct ContractSpec {
    pub name: String,
    pub file: String,
    pub specification: String,
    pub content_type: String,
    pub verification_results: Option<String>,
    pub verification_success: Option<bool>,
    pub verifier: Option<String>,
    pub verifier_version: Option<String>,
    pub verification_results_content_type: Option<String>,
    pub verification_results_format: Option<String>,
}

impl ContractSpec {
    /// Parse `"name=payments-api,file=./pay.yaml,specification=oas,content-type=application/yaml"`
    /// into a `ContractSpec`. Required keys: `name`, `file`.
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for pair in input.split(',') {
            if let Some(eq_pos) = pair.find('=') {
                let key = pair[..eq_pos].trim().to_string();
                let value = pair[eq_pos + 1..].trim().to_string();
                map.insert(key, value);
            }
        }

        let name = map
            .remove("name")
            .ok_or_else(|| "--contract requires 'name' key".to_string())?;
        let file = map
            .remove("file")
            .ok_or_else(|| "--contract requires 'file' key".to_string())?;
        let specification = map
            .remove("specification")
            .unwrap_or_else(|| "oas".to_string());
        let content_type = map
            .remove("content-type")
            .unwrap_or_else(|| "application/yaml".to_string());
        let verification_results = map.remove("verification-results");
        let verification_success = map
            .remove("verification-success")
            .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1"));
        let verifier = map.remove("verifier");
        let verifier_version = map.remove("verifier-version");
        let verification_results_content_type = map.remove("verification-results-content-type");
        let verification_results_format = map.remove("verification-results-format");

        Ok(ContractSpec {
            name,
            file,
            specification,
            content_type,
            verification_results,
            verification_success,
            verifier,
            verifier_version,
            verification_results_content_type,
            verification_results_format,
        })
    }
}

#[derive(Debug, Deserialize)]
struct PublishContractsResponse {
    notices: Vec<Notice>,
    contracts: Vec<String>,
}

pub fn publish_multiple(args: &ArgMatches) -> Result<Value, PactBrokerError> {
    // Parse --contract flags
    let raw_contracts: Vec<String> = args
        .get_many::<String>("contract")
        .unwrap_or_default()
        .cloned()
        .collect();

    let specs: Vec<ContractSpec> = raw_contracts
        .iter()
        .map(|s| ContractSpec::parse(s))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| PactBrokerError::ValidationError(vec![e]))?;

    // Validate: no duplicate contract names
    let mut seen: HashSet<&str> = HashSet::new();
    for spec in &specs {
        if !seen.insert(spec.name.as_str()) {
            return Err(PactBrokerError::ValidationError(vec![format!(
                "Duplicate contract name: '{}'",
                spec.name
            )]));
        }
    }

    // Read all files eagerly — fail before any HTTP call
    let contract_data: Vec<(ContractSpec, String, Option<String>)> = specs
        .into_iter()
        .map(|spec| {
            let content = std::fs::read_to_string(&spec.file).map_err(|e| {
                eprintln!("❌ Failed to read contract file '{}': {}", spec.file, e);
                PactBrokerError::IoError(e.to_string())
            })?;
            let verif_content = if let Some(ref path) = spec.verification_results {
                Some(std::fs::read_to_string(path).map_err(|e| {
                    eprintln!(
                        "❌ Failed to read verification results file '{}': {}",
                        path, e
                    );
                    PactBrokerError::IoError(e.to_string())
                })?)
            } else {
                None
            };
            Ok::<_, PactBrokerError>((spec, content, verif_content))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let hal_client: HALClient = HALClient::with_url(
        &broker_url,
        Some(get_auth(args)),
        get_ssl_options(args),
        get_custom_headers(args),
    )
    .with_retry_count(get_retries(args));

    let publish_href_result = tokio::runtime::Runtime::new().unwrap().block_on(async {
        get_broker_relation(
            hal_client.clone(),
            "pf:publish-provider-contracts".to_string(),
            broker_url.to_string(),
        )
        .await
    });

    match publish_href_result {
        Ok(publish_href) => {
            let provider_name = args
                .get_one::<String>("provider")
                .expect("PROVIDER is required");
            let mut provider_app_version = args.get_one::<String>("provider-app-version");
            let mut branch = args.get_one::<String>("branch");
            let build_url = args.get_one::<String>("build-url");
            let tag_with_git_branch = args.get_flag("tag-with-git-branch");
            let auto_detect = args.get_flag("auto-detect-version-properties");

            let (git_commit, git_branch);
            if auto_detect {
                git_commit = git_info::commit(false);
                git_branch = git_info::branch(false);
                if provider_app_version.is_none() {
                    provider_app_version = git_commit.as_ref();
                    if let Some(v) = provider_app_version {
                        println!("🔍 Auto detected git commit: {}", v);
                    }
                }
                if branch.is_none() {
                    branch = git_branch.as_ref();
                    if let Some(b) = branch {
                        println!("🔍 Auto detected git branch: {}", b);
                    }
                }
            }

            let publish_href = publish_href.replace("{provider}", provider_name);

            // Build the contracts array
            let contracts_array: Vec<Value> = contract_data
                .iter()
                .map(|(spec, content, verif_content)| {
                    let mut entry = json!({
                        "name": spec.name,
                        "content": Base64.encode(content),
                        "contentType": spec.content_type,
                        "specification": spec.specification,
                    });

                    if verif_content.is_some()
                        || spec.verifier.is_some()
                        || spec.verifier_version.is_some()
                    {
                        let mut svr = serde_json::Map::new();
                        svr.insert(
                            "success".to_string(),
                            Value::Bool(spec.verification_success.unwrap_or(false)),
                        );
                        if let Some(vc) = verif_content {
                            svr.insert("content".to_string(), Value::String(Base64.encode(vc)));
                        }
                        if let Some(ct) = &spec.verification_results_content_type {
                            svr.insert("contentType".to_string(), Value::String(ct.clone()));
                        }
                        if let Some(fmt) = &spec.verification_results_format {
                            svr.insert("format".to_string(), Value::String(fmt.clone()));
                        }
                        if let Some(v) = &spec.verifier {
                            svr.insert("verifier".to_string(), Value::String(v.clone()));
                        }
                        if let Some(vv) = &spec.verifier_version {
                            svr.insert("verifierVersion".to_string(), Value::String(vv.clone()));
                        }
                        entry["selfVerificationResults"] = Value::Object(svr);
                    }

                    entry
                })
                .collect();

            let mut payload = json!({
                "pacticipantVersionNumber": provider_app_version,
                "contracts": contracts_array,
            });

            if let Some(tags) = args.get_many::<String>("tag") {
                payload["tags"] = serde_json::Value::Array(vec![]);
                for tag in tags {
                    payload["tags"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::Value::String(tag.to_string()));
                }
            }
            if tag_with_git_branch {
                if !payload.get("tags").is_some_and(|v| v.is_array()) {
                    payload["tags"] = serde_json::Value::Array(vec![]);
                }
                payload["tags"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::Value::String(
                        git_info::branch(false).unwrap_or_default(),
                    ));
            }
            if let Some(b) = branch {
                payload["branch"] = Value::String(b.to_string());
            }
            if let Some(u) = build_url {
                payload["buildUrl"] = Value::String(u.to_string());
            }

            let output: Result<Option<&String>, clap::parser::MatchesError> =
                args.try_get_one::<String>("output");

            let n = payload["contracts"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);
            println!(
                "📨 Attempting to publish {} provider contracts for provider: {} version: {}",
                n,
                provider_name,
                provider_app_version.map_or("unknown", |v| v)
            );

            let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
                hal_client
                    .clone()
                    .post_json(
                        &publish_href,
                        &payload.to_string(),
                        Some({
                            let mut headers = std::collections::HashMap::new();
                            headers.insert(
                                "Accept".to_string(),
                                "application/problem+json".to_string(),
                            );
                            headers
                        }),
                    )
                    .await
            });

            match res {
                Ok(res) => match output {
                    Ok(Some(output)) => {
                        if output == "pretty" {
                            let json = serde_json::to_string_pretty(&res).unwrap();
                            println!("{}", json);
                        } else if output == "json" {
                            return Ok(res.clone());
                        } else {
                            match serde_json::from_value::<PublishContractsResponse>(res) {
                                Ok(parsed) => {
                                    print!("✅ ");
                                    process_notices(&parsed.notices);
                                    println!(
                                        "Published contracts: {}",
                                        parsed.contracts.join(", ")
                                    );
                                }
                                Err(err) => {
                                    println!(
                                        "✅ Provider contracts published successfully for: {} version: {}",
                                        provider_name,
                                        provider_app_version.map_or("unknown", |v| v)
                                    );
                                    println!(
                                        "⚠️ Warning: Failed to process response - Error: {:?}",
                                        err
                                    );
                                    return Err(PactBrokerError::ContentError(err.to_string()));
                                }
                            }
                        }
                    }
                    _ => {
                        println!("{:?}", res.clone());
                    }
                },
                Err(err) => {
                    match &err {
                        PactBrokerError::ValidationErrorWithNotices(messages, notices) => {
                            println!("❌ Provider contract publication failed:");
                            for message in messages {
                                println!("   {}", message);
                            }
                            if !notices.is_empty() {
                                println!("\nDetails:");
                                process_notices(notices);
                            }
                        }
                        _ => {
                            println!("❌ {}", err);
                        }
                    }
                    return Err(err);
                }
            }
            Ok(json!({}))
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod publish_multiple_provider_contracts_tests {
    use super::*;
    use crate::cli::pactflow::main::subcommands::add_publish_provider_contracts_subcommand;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    const PROVIDER_NAME: &str = "my-provider";
    const PROVIDER_VERSION: &str = "1.4.2";
    const BRANCH: &str = "main";
    const TAG: &str = "dev";
    const BUILD_URL: &str = "http://ci/build/42";
    const PAYMENTS_FIXTURE: &str = "tests/fixtures/payments-api.yaml";
    const FRAUD_FIXTURE: &str = "tests/fixtures/fraud-events.yaml";
    const PROTO_FIXTURE: &str = "tests/fixtures/service.proto";
    const VERIF_RESULTS: &str = "tests/fixtures/verification-results.txt";

    fn mock_server_config() -> MockServerConfig {
        MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        }
    }

    // Test 1: happy path — two contracts (OAS + AsyncAPI), no verification results
    #[test]
    fn publish_two_contracts_succeeds() {
        let payments_content = std::fs::read_to_string(PAYMENTS_FIXTURE).unwrap();
        let fraud_content = std::fs::read_to_string(FRAUD_FIXTURE).unwrap();
        let payments_b64 = Base64.encode(&payments_content);
        let fraud_b64 = Base64.encode(&fraud_content);

        let request_body = json!({
            "pacticipantVersionNumber": PROVIDER_VERSION,
            "branch": BRANCH,
            "tags": [TAG],
            "buildUrl": BUILD_URL,
            "contracts": [
                {
                    "name": "payments-api",
                    "content": payments_b64,
                    "contentType": "application/yaml",
                    "specification": "oas"
                },
                {
                    "name": "fraud-events",
                    "content": fraud_b64,
                    "contentType": "application/yaml",
                    "specification": "asyncapi"
                }
            ]
        });

        let response_body = json!({
            "notices": [{ "text": "Contracts published successfully", "type": "success" }],
            "contracts": ["payments-api", "fraud-events"]
        });

        let pactflow_service = PactBuilder::new("pact-broker-cli", "PactFlow")
            .interaction("GET / returns HAL index with pf:publish-provider-contracts", "", |mut i| {
                i.given("pf:publish-provider-contracts relation exists in index");
                i.request
                    .get()
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pf:publish-provider-contracts": {
                                "href": term!(
                                    format!(".*\\/provider-contracts\\/provider\\/{}\\/publish-contracts", PROVIDER_NAME),
                                    format!("http://localhost:1234/provider-contracts/provider/{}/publish-contracts", PROVIDER_NAME)
                                )
                            }
                        }
                    }));
                i
            })
            .interaction("POST publish-contracts publishes two contracts", "", |mut i| {
                i.request
                    .post()
                    .path(format!(
                        "/provider-contracts/provider/{}/publish-contracts",
                        PROVIDER_NAME
                    ))
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/hal+json,application/problem+json")
                    .json_body(request_body.clone());
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(response_body.clone());
                i
            })
            .start_mock_server(None, Some(mock_server_config()));

        let url = pactflow_service.url();

        let matches = add_publish_provider_contracts_subcommand().get_matches_from(vec![
            "publish-provider-contracts",
            "-b",
            url.as_str(),
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--branch",
            BRANCH,
            "--tag",
            TAG,
            "--build-url",
            BUILD_URL,
            "--contract",
            &format!(
                "name=payments-api,file={},specification=oas,content-type=application/yaml",
                PAYMENTS_FIXTURE
            ),
            "--contract",
            &format!(
                "name=fraud-events,file={},specification=asyncapi,content-type=application/yaml",
                FRAUD_FIXTURE
            ),
            "--output",
            "json",
        ]);

        let result = publish_multiple(&matches);

        assert!(result.is_ok());
        let val = result.unwrap();
        let notices = val.get("notices").unwrap().as_array().unwrap();
        assert!(!notices.is_empty());
        let contracts = val.get("contracts").unwrap().as_array().unwrap();
        assert_eq!(contracts.len(), 2);
        assert!(contracts.iter().any(|c| c.as_str() == Some("payments-api")));
        assert!(contracts.iter().any(|c| c.as_str() == Some("fraud-events")));
    }

    // Test 2: verification results on one contract (OAS), not the other (AsyncAPI)
    #[test]
    fn publish_contracts_with_per_contract_verification_results() {
        let payments_content = std::fs::read_to_string(PAYMENTS_FIXTURE).unwrap();
        let fraud_content = std::fs::read_to_string(FRAUD_FIXTURE).unwrap();
        let verif_content = std::fs::read_to_string(VERIF_RESULTS).unwrap();
        let payments_b64 = Base64.encode(&payments_content);
        let fraud_b64 = Base64.encode(&fraud_content);
        let verif_b64 = Base64.encode(&verif_content);

        let request_body = json!({
            "pacticipantVersionNumber": PROVIDER_VERSION,
            "contracts": [
                {
                    "name": "payments-api",
                    "content": payments_b64,
                    "contentType": "application/yaml",
                    "specification": "oas",
                    "selfVerificationResults": {
                        "success": true,
                        "content": verif_b64,
                        "contentType": "text/plain",
                        "format": "text",
                        "verifier": "spectral",
                        "verifierVersion": "1.0.0"
                    }
                },
                {
                    "name": "fraud-events",
                    "content": fraud_b64,
                    "contentType": "application/yaml",
                    "specification": "asyncapi"
                }
            ]
        });

        let response_body = json!({
            "notices": [{ "text": "Contracts published", "type": "success" }],
            "contracts": ["payments-api", "fraud-events"]
        });

        let pactflow_service = PactBuilder::new("pact-broker-cli", "PactFlow")
            .interaction(
                "GET / returns HAL index with pf:publish-provider-contracts (verif test)",
                "",
                |mut i| {
                    i.given("pf:publish-provider-contracts relation exists in index");
                    i.request
                        .get()
                        .path("/")
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_links": {
                                "pf:publish-provider-contracts": {
                                    "href": term!(
                                        format!(".*\\/provider-contracts\\/provider\\/{}\\/publish-contracts", PROVIDER_NAME),
                                        format!("http://localhost:1234/provider-contracts/provider/{}/publish-contracts", PROVIDER_NAME)
                                    )
                                }
                            }
                        }));
                    i
                },
            )
            .interaction(
                "POST publish-contracts with per-contract verification results",
                "",
                |mut i| {
                    i.request
                        .post()
                        .path(format!(
                            "/provider-contracts/provider/{}/publish-contracts",
                            PROVIDER_NAME
                        ))
                        .header("Content-Type", "application/json")
                        .header("Accept", "application/hal+json,application/problem+json")
                        .json_body(request_body.clone());
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(response_body.clone());
                    i
                },
            )
            .start_mock_server(None, Some(mock_server_config()));

        let url = pactflow_service.url();

        let matches = add_publish_provider_contracts_subcommand().get_matches_from(vec![
            "publish-provider-contracts",
            "-b",
            url.as_str(),
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--contract",
            &format!(
                "name=payments-api,file={},specification=oas,content-type=application/yaml,\
                 verification-results={},verification-success=true,verifier=spectral,\
                 verifier-version=1.0.0,verification-results-content-type=text/plain,\
                 verification-results-format=text",
                PAYMENTS_FIXTURE, VERIF_RESULTS
            ),
            "--contract",
            &format!(
                "name=fraud-events,file={},specification=asyncapi,content-type=application/yaml",
                FRAUD_FIXTURE
            ),
            "--output",
            "json",
        ]);

        let result = publish_multiple(&matches);

        assert!(result.is_ok());
        let val = result.unwrap();
        let contracts = val.get("contracts").unwrap().as_array().unwrap();
        assert_eq!(contracts.len(), 2);
    }

    // Test 3: missing contract file returns IoError before any HTTP call
    #[test]
    fn publish_contracts_missing_file_returns_io_error() {
        let matches = add_publish_provider_contracts_subcommand().get_matches_from(vec![
            "publish-provider-contracts",
            "-b",
            "http://localhost:9999",
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--contract",
            "name=payments-api,file=tests/fixtures/nonexistent.yaml,specification=oas,content-type=application/yaml",
        ]);

        let result = publish_multiple(&matches);

        assert!(result.is_err());
        match result.unwrap_err() {
            PactBrokerError::IoError(_) => {}
            other => panic!("Expected IoError, got: {:?}", other),
        }
    }

    // Test 4: arbitrary specification type (protobuf) passes through unchanged — server decides validity
    #[test]
    fn publish_contracts_supports_arbitrary_specification_types() {
        let payments_content = std::fs::read_to_string(PAYMENTS_FIXTURE).unwrap();
        let fraud_content = std::fs::read_to_string(FRAUD_FIXTURE).unwrap();
        let proto_content = std::fs::read_to_string(PROTO_FIXTURE).unwrap();
        let payments_b64 = Base64.encode(&payments_content);
        let fraud_b64 = Base64.encode(&fraud_content);
        let proto_b64 = Base64.encode(&proto_content);

        let request_body = json!({
            "pacticipantVersionNumber": PROVIDER_VERSION,
            "contracts": [
                {
                    "name": "payments-api",
                    "content": payments_b64,
                    "contentType": "application/yaml",
                    "specification": "oas"
                },
                {
                    "name": "fraud-events",
                    "content": fraud_b64,
                    "contentType": "application/yaml",
                    "specification": "asyncapi"
                },
                {
                    "name": "payments-grpc",
                    "content": proto_b64,
                    "contentType": "application/x-protobuf",
                    "specification": "protobuf"
                }
            ]
        });

        let response_body = json!({
            "notices": [{ "text": "Contracts published successfully", "type": "success" }],
            "contracts": ["payments-api", "fraud-events", "payments-grpc"]
        });

        let pactflow_service = PactBuilder::new("pact-broker-cli", "PactFlow")
            .interaction(
                "GET / returns HAL index with pf:publish-provider-contracts (multi-spec test)",
                "",
                |mut i| {
                    i.given("pf:publish-provider-contracts relation exists in index");
                    i.request
                        .get()
                        .path("/")
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_links": {
                                "pf:publish-provider-contracts": {
                                    "href": term!(
                                        format!(".*\\/provider-contracts\\/provider\\/{}\\/publish-contracts", PROVIDER_NAME),
                                        format!("http://localhost:1234/provider-contracts/provider/{}/publish-contracts", PROVIDER_NAME)
                                    )
                                }
                            }
                        }));
                    i
                },
            )
            .interaction(
                "POST publish-contracts with oas, asyncapi, and protobuf specifications",
                "",
                |mut i| {
                    i.request
                        .post()
                        .path(format!(
                            "/provider-contracts/provider/{}/publish-contracts",
                            PROVIDER_NAME
                        ))
                        .header("Content-Type", "application/json")
                        .header("Accept", "application/hal+json,application/problem+json")
                        .json_body(request_body.clone());
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(response_body.clone());
                    i
                },
            )
            .start_mock_server(None, Some(mock_server_config()));

        let url = pactflow_service.url();

        let matches = add_publish_provider_contracts_subcommand().get_matches_from(vec![
            "publish-provider-contracts",
            "-b",
            url.as_str(),
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--contract",
            &format!(
                "name=payments-api,file={},specification=oas,content-type=application/yaml",
                PAYMENTS_FIXTURE
            ),
            "--contract",
            &format!(
                "name=fraud-events,file={},specification=asyncapi,content-type=application/yaml",
                FRAUD_FIXTURE
            ),
            "--contract",
            &format!(
                "name=payments-grpc,file={},specification=protobuf,content-type=application/x-protobuf",
                PROTO_FIXTURE
            ),
            "--output",
            "json",
        ]);

        let result = publish_multiple(&matches);

        assert!(result.is_ok());
        let val = result.unwrap();
        let contracts = val.get("contracts").unwrap().as_array().unwrap();
        assert_eq!(contracts.len(), 3);
        assert!(contracts.iter().any(|c| c.as_str() == Some("payments-api")));
        assert!(contracts.iter().any(|c| c.as_str() == Some("fraud-events")));
        assert!(
            contracts
                .iter()
                .any(|c| c.as_str() == Some("payments-grpc"))
        );
    }

    // Test 5: duplicate contract names are rejected before any HTTP call
    #[test]
    fn publish_contracts_duplicate_names_rejected() {
        let matches = add_publish_provider_contracts_subcommand().get_matches_from(vec![
            "publish-provider-contracts",
            "-b",
            "http://localhost:9999",
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--contract",
            &format!(
                "name=payments-api,file={},specification=oas,content-type=application/yaml",
                PAYMENTS_FIXTURE
            ),
            "--contract",
            &format!(
                "name=payments-api,file={},specification=asyncapi,content-type=application/yaml",
                FRAUD_FIXTURE
            ),
        ]);

        let result = publish_multiple(&matches);

        assert!(result.is_err());
        match result.unwrap_err() {
            PactBrokerError::ValidationError(msgs) => {
                assert!(msgs.iter().any(|m| m.contains("payments-api")));
            }
            other => panic!("Expected ValidationError, got: {:?}", other),
        }
    }
}
