use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as Base64;
use clap::ArgMatches;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::verification::{SelfVerificationInputs, missing_self_verification_keys};
use crate::cli::{
    pact_broker::main::{
        HALClient, Notice, PactBrokerError,
        broker_path::expand_path_template,
        process_notices,
        utils::{
            get_auth, get_broker_relation, get_broker_url, get_custom_headers, get_retries,
            get_ssl_options,
        },
    },
    utils::git_info,
};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderContractPublishRoot {
    #[serde(rename = "_embedded")]
    embedded: Embedded,
    #[serde(rename = "_links")]
    links: Links,
    notices: Vec<Notice>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Embedded {
    version: Version,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    number: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Links {
    #[serde(rename = "pb:pacticipant-version-tags")]
    pb_pacticipant_version_tags: Vec<Value>,
}

/// Read a per-contract option in single-contract mode.
///
/// These options are declared `ArgAction::Append` so the `--contract` form can bind each
/// occurrence to the contract it follows. With a single contract only one occurrence makes sense,
/// so a repeat is reported rather than left to `get_one`, which would panic on multiple values.
fn single_value<'a>(
    args: &'a ArgMatches,
    option: &str,
) -> Result<Option<&'a String>, PactBrokerError> {
    let mut values = args.get_many::<String>(option).into_iter().flatten();
    let first = values.next();
    if values.next().is_some() {
        return Err(PactBrokerError::ValidationError(vec![format!(
            "--{option} was given more than once, but only one contract is being published. \
             Use --contract FILE --name NAME to publish several."
        )]));
    }
    Ok(first)
}

/// The self-verification outcome in single-contract mode, or `None` when neither flag was given.
fn single_outcome(args: &ArgMatches) -> Result<Option<bool>, PactBrokerError> {
    let read = |option: &str, positive: bool| -> Result<Option<bool>, PactBrokerError> {
        let mut values = args.get_many::<bool>(option).into_iter().flatten();
        let first = values.next().copied();
        if values.next().is_some() {
            return Err(PactBrokerError::ValidationError(vec![format!(
                "--{option} was given more than once, but only one contract is being published."
            )]));
        }
        // --no-verification-success is the negation, so bare use means a failed outcome.
        Ok(first.map(|value| if positive { value } else { !value }))
    };
    match (
        read("verification-success", true)?,
        read("no-verification-success", false)?,
    ) {
        (Some(_), Some(_)) => Err(PactBrokerError::ValidationError(vec![
            "--verification-success and --no-verification-success cannot both be given."
                .to_string(),
        ])),
        (Some(outcome), None) | (None, Some(outcome)) => Ok(Some(outcome)),
        (None, None) => Ok(None),
    }
}

pub fn publish(args: &ArgMatches) -> Result<Value, PactBrokerError> {
    // --contract selects the batch endpoint, which takes a named array rather than the single
    // scalar contract this function builds. clap guarantees the two modes are mutually exclusive.
    if args
        .get_many::<String>("contract")
        .into_iter()
        .flatten()
        .next()
        .is_some()
    {
        return super::publish_multiple::publish_multiple(args);
    }

    // Load contract file
    let contract_file = args
        .get_one::<String>("contract-file")
        .expect("CONTRACT_FILE is required");
    let contract_content = std::fs::read_to_string(contract_file).map_err(|e| {
        println!("❌ Failed to read contract file: {}", e);
        PactBrokerError::IoError(e.to_string())
    })?;

    // Checked before any request is made, so an incomplete set of flags fails without a broker.
    let missing = missing_self_verification_keys(&SelfVerificationInputs {
        results: single_value(args, "verification-results")?.is_some(),
        results_content_type: single_value(args, "verification-results-content-type")?.is_some(),
        verifier: single_value(args, "verifier")?.is_some(),
        verifier_version: single_value(args, "verifier-version")?.is_some(),
    });
    if !missing.is_empty() {
        return Err(PactBrokerError::ValidationError(vec![format!(
            "Self-verification needs --verification-results, --verification-results-content-type \
             and --verifier together; missing: {}",
            missing
                .iter()
                .map(|key| format!("--{key}"))
                .collect::<Vec<_>>()
                .join(", ")
        )]));
    }

    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let custom_headers = get_custom_headers(args);
    let ssl_options = get_ssl_options(args);
    let hal_client: HALClient = HALClient::with_url(
        &broker_url,
        Some(auth.clone()),
        ssl_options.clone(),
        custom_headers.clone(),
    )
    .with_retry_count(get_retries(args));

    // Use pf:publish-provider-contract relation
    let publish_contract_href_path = tokio::runtime::Runtime::new().unwrap().block_on(async {
        get_broker_relation(
            hal_client.clone(),
            "pf:publish-provider-contract".to_string(),
            broker_url.to_string(),
        )
        .await
    });
    match publish_contract_href_path {
        Ok(publish_contract_href) => {
            let provider_name = args
                .get_one::<String>("provider")
                .expect("PROVIDER is required");
            let mut provider_app_version = args.get_one::<String>("provider-app-version");
            let mut branch = args.get_one::<String>("branch");
            let tag_with_git_branch = args.get_flag("tag-with-git-branch");
            let build_url = args.get_one::<String>("build-url");
            let default_specification = "oas".to_string();
            let specification =
                single_value(args, "specification")?.unwrap_or(&default_specification);
            let default_content_type = "application/yaml".to_string();
            let content_type = single_value(args, "content-type")?.unwrap_or(&default_content_type);
            let auto_detect_version_properties: bool =
                args.get_flag("auto-detect-version-properties");
            let (git_commit, git_branch);
            if auto_detect_version_properties {
                git_commit = git_info::commit(false);
                git_branch = git_info::branch(false);
            } else {
                git_commit = Some("".to_string());
                git_branch = Some("".to_string());
            }
            if auto_detect_version_properties {
                if provider_app_version.is_none() {
                    provider_app_version = git_commit.as_ref();
                    if let Some(v) = provider_app_version {
                        println!("🔍 Auto detected git commit: {}", v);
                    }
                } else if let Some(v) = provider_app_version {
                    println!(
                        "🔍 auto_detect_version_properties set to {}, but provider_app_version provided {}",
                        auto_detect_version_properties, v
                    );
                }
                if branch.is_none() {
                    branch = git_branch.as_ref();
                    if let Some(b) = branch {
                        println!("🔍 Auto detected git branch: {}", b);
                    }
                } else if let Some(b) = branch {
                    println!(
                        "🔍 auto_detect_version_properties set to {}, but branch provided {}",
                        auto_detect_version_properties, b
                    );
                }
            }
            let publish_contract_href = expand_path_template(
                &publish_contract_href,
                &HashMap::from([("provider".to_string(), provider_name.to_string())]),
            )?;

            // Verification results
            let verification_exit_code = single_value(args, "verification-exit-code")?;
            let verification_success = match (single_outcome(args)?, verification_exit_code) {
                (Some(_), Some(_)) => {
                    return Err(PactBrokerError::ValidationError(vec![
                        "--verification-success/--no-verification-success and \
                         --verification-exit-code cannot both be given."
                            .to_string(),
                    ]));
                }
                (Some(outcome), None) => outcome,
                (None, Some(exit_code_str)) => {
                    matches!(exit_code_str.parse::<i32>(), Ok(0))
                }
                (None, None) => false,
            };

            let verification_results_path = single_value(args, "verification-results")?;
            let verification_results_content = if let Some(file_path) = verification_results_path {
                Some(std::fs::read_to_string(file_path).map_err(|e| {
                    eprintln!(
                        "❌ Failed to read verification results file '{}': {}",
                        file_path, e
                    );
                    PactBrokerError::IoError(e.to_string())
                })?)
            } else {
                None
            };
            let verification_results_content_type =
                single_value(args, "verification-results-content-type")?;
            let verification_results_format = single_value(args, "verification-results-format")?;
            let verifier = single_value(args, "verifier")?;
            let verifier_version = single_value(args, "verifier-version")?;

            // Build contract params
            let mut contract_params = json!({
                "content": Base64.encode(contract_content),
                "specification": specification,
                "contentType": content_type,
            });

            // Add selfVerificationResults if provided
            if verification_results_content.is_some()
                || verifier.is_some()
                || verifier_version.is_some()
            {
                let mut verification_results_params = serde_json::Map::new();
                verification_results_params
                    .insert("success".to_string(), Value::Bool(verification_success));
                if let Some(content) = verification_results_content {
                    verification_results_params
                        .insert("content".to_string(), Value::String(Base64.encode(content)));
                }
                if let Some(content_type) = verification_results_content_type {
                    verification_results_params.insert(
                        "contentType".to_string(),
                        Value::String(content_type.to_string()),
                    );
                }
                if let Some(format) = verification_results_format {
                    verification_results_params
                        .insert("format".to_string(), Value::String(format.to_string()));
                }
                if let Some(verifier) = verifier {
                    verification_results_params
                        .insert("verifier".to_string(), Value::String(verifier.to_string()));
                }
                if let Some(verifier_version) = verifier_version {
                    verification_results_params.insert(
                        "verifierVersion".to_string(),
                        Value::String(verifier_version.to_string()),
                    );
                }
                contract_params["selfVerificationResults"] =
                    Value::Object(verification_results_params);
            }

            // Build payload
            let mut payload = json!({
                "pacticipantVersionNumber": provider_app_version,
                "contract": contract_params,
            });

            if let Some(tags) = args.get_many::<String>("tag") {
                payload["tags"] = serde_json::Value::Array(vec![]);
                for tag in tags {
                    payload["tags"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::Value::String(tag.to_string()));
                }
            };
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

            if let Some(branch) = branch {
                payload["branch"] = Value::String(branch.to_string());
            }
            if let Some(build_url) = build_url {
                payload["buildUrl"] = Value::String(build_url.to_string());
            }

            // Output option
            let output: Result<Option<&String>, clap::parser::MatchesError> =
                args.try_get_one::<String>("output");

            println!(
                "📨 Attempting to publish provider contract for provider: {} version: {}",
                provider_name,
                provider_app_version.unwrap()
            );
            let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
                hal_client
                    .clone()
                    .post_json(
                        &(publish_contract_href),
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
                            let parsed_res =
                                serde_json::from_value::<ProviderContractPublishRoot>(res);
                            match parsed_res {
                                Ok(parsed_res) => {
                                    print!("✅ ");
                                    process_notices(&parsed_res.notices);
                                }
                                Err(err) => {
                                    println!(
                                        "✅ Provider contract published successfully for provider: {} version: {}",
                                        provider_name,
                                        provider_app_version.unwrap()
                                    );
                                    println!(
                                        "⚠️ Warning: Failed to process response notices - Error: {:?}",
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
                        crate::cli::pact_broker::main::PactBrokerError::ValidationErrorWithNotices(messages, notices) => {
                            println!("❌ Provider contract publication failed:");
                            for message in messages {
                                println!("   {}", message);
                            }
                            if !notices.is_empty() {
                                println!("\nDetails:");
                                process_notices(notices);
                            }
                        },
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
mod publish_provider_contract_tests {
    use crate::cli::pactflow::main::provider_contracts::publish::publish;
    use crate::cli::pactflow::main::subcommands::add_publish_provider_contract_subcommand;
    use base64::{Engine, engine::general_purpose::STANDARD as Base64};
    use pact_consumer::prelude::*;
    use pact_models::prelude::Generator;
    use pact_models::{PactSpecification, generators};
    use serde_json::json;

    // --- mode boundary: CONTRACT_FILE and --contract are mutually exclusive ---

    fn try_parse(argv: Vec<&str>) -> Result<clap::ArgMatches, clap::Error> {
        add_publish_provider_contract_subcommand().try_get_matches_from(argv)
    }

    const SPEC: &str = "tests/fixtures/payments-api.yaml";

    #[test]
    fn rejects_a_contract_file_and_contract_flag_together() {
        let err = try_parse(vec![
            "publish-provider-contract",
            "-b",
            "http://localhost:9999",
            "--provider",
            "p",
            "-a",
            "1.0",
            "tests/fixtures/payments-api.yaml",
            "--contract",
            SPEC,
        ])
        .expect_err("the two modes must not be combinable");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn rejects_neither_a_contract_file_nor_a_contract_flag() {
        let err = try_parse(vec![
            "publish-provider-contract",
            "-b",
            "http://localhost:9999",
            "--provider",
            "p",
            "-a",
            "1.0",
        ])
        .expect_err("one of the two modes is required");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    /// The per-contract options describe a `--contract` in the ordered form, so unlike the earlier
    /// design they must be *accepted* alongside it rather than rejected.
    #[test]
    fn accepts_per_contract_options_after_a_contract_flag() {
        for (flag, value) in [
            ("--specification", Some("oas")),
            ("--content-type", Some("application/yaml")),
            ("--verifier", Some("spectral")),
            ("--verifier-version", Some("1.0.0")),
            ("--verification-results", Some("r.txt")),
            ("--verification-results-content-type", Some("text/plain")),
            ("--verification-results-format", Some("junit")),
            ("--verification-exit-code", Some("0")),
            ("--verification-success", None),
            ("--no-verification-success", None),
        ] {
            let mut argv = vec![
                "publish-provider-contract",
                "-b",
                "http://localhost:9999",
                "--provider",
                "p",
                "-a",
                "1.0",
                "--contract",
                SPEC,
                "--name",
                "a",
                flag,
            ];
            if let Some(v) = value {
                argv.push(v);
            }
            try_parse(argv)
                .unwrap_or_else(|e| panic!("{flag} should be accepted after --contract: {e}"));
        }
    }

    #[test]
    fn rejects_a_name_alongside_the_positional_contract_file() {
        let err = try_parse(vec![
            "publish-provider-contract",
            "-b",
            "http://localhost:9999",
            "--provider",
            "p",
            "-a",
            "1.0",
            SPEC,
            "--name",
            "a",
        ])
        .expect_err("--name only describes a --contract");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn accepts_repeated_contract_flags_without_a_positional() {
        try_parse(vec![
            "publish-provider-contract",
            "-b",
            "http://localhost:9999",
            "--provider",
            "p",
            "-a",
            "1.0",
            "--contract",
            SPEC,
            "--name",
            "a",
            "--contract",
            "tests/fixtures/fraud-events.yaml",
            "--name",
            "b",
        ])
        .expect("repeating --contract is the multi-contract happy path");
    }

    #[test]
    fn publish_provider_contract_success() {
        // Arrange
        let provider_name = "Bar";
        let provider_version_number = "1";
        let branch_name = "main";
        let tag = "dev";
        let build_url = "http://build";
        let contract_content_yaml = "some:\n  contract";
        let contract_content_base64 = Base64.encode(contract_content_yaml);
        let verification_results_content = "some results";
        let verification_results_content_base64 = Base64.encode(verification_results_content);

        let request_body = json!({
            "pacticipantVersionNumber": provider_version_number,
            "tags": [tag],
            "branch": branch_name,
            "buildUrl": build_url,
            "contract": {
                "content": contract_content_base64,
                "contentType": "application/yaml",
                "specification": "oas",
                "selfVerificationResults": {
                    "success": true,
                    "content": verification_results_content_base64,
                    "contentType": "text/plain",
                    "format": "text",
                    "verifier": "my custom tool",
                    "verifierVersion": "1.0"
                }
            }
        });

        let publish_provider_contract_path_generator = generators! {
            "BODY" => {
            "$._links.pb:pf:publish-provider-contract.href" => Generator::MockServerURL(
                           format!("/contracts/publish/{}", provider_name),
                            ".*\\/contracts\\/publish\\/.*".to_string()
            )
            }
        };

        let index_response_body = json_pattern!({
            "_links": {
                "pf:publish-provider-contract": {
                    "href": term!(format!(".*\\/contracts\\/publish\\/{}",provider_name), format!("http://localhost:1234/contracts/publish/{}", provider_name)),
                }
            }
        });

        let success_response_body = json!({
            "notices": [
                { "text": "some notice", "type": "info" }
            ],
            "_embedded": {
                "version": {
                    "number": provider_version_number
                }
            },
            "_links": {
                "pb:pacticipant-version-tags": [json!({})],
                "pb:branch-version": json!({})
            }
        });

        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "PactFlow")
            .interaction("a request for the index resource", "", |mut i| {
                i.given("the pb:publish-provider-contract relation exists in the index resource");
                i.request
                    .get()
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(index_response_body)
                    .generators()
                    .add_generators(publish_provider_contract_path_generator);
                i
            })
            .interaction("a request to publish a provider contract", "", |mut i| {
                i.request
                    .post()
                    .path(format!("/contracts/publish/{}", provider_name))
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/hal+json,application/problem+json")
                    .json_body(request_body.clone());
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(success_response_body.clone());
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange - set up the command line arguments
        let matches = add_publish_provider_contract_subcommand().get_matches_from(vec![
            "publish-provider-contract",
            "tests/fixtures/provider-contract.yaml",
            "-b",
            mock_server_url.as_str(),
            "--provider",
            provider_name,
            "--provider-app-version",
            provider_version_number,
            "--branch",
            branch_name,
            "--tag",
            tag,
            "--build-url",
            build_url,
            // "--specification",
            // "oas",
            "--content-type",
            "application/yaml",
            "--verification-success",
            "--verification-results",
            "tests/fixtures/verification-results.txt",
            "--verification-results-content-type",
            "text/plain",
            "--verification-results-format",
            "text",
            "--verifier",
            "my custom tool",
            "--verifier-version",
            "1.0",
            "--output",
            "json",
        ]);

        // Act
        let result = publish(&matches);

        // Assert
        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value.is_object());
        let notices = value.get("notices").unwrap();
        assert!(notices.is_array());
        assert!(notices[0]["text"].as_str().unwrap().contains("some notice"));
        let embedded = value.get("_embedded").unwrap();
        let version = embedded.get("version").unwrap();
        assert_eq!(version.get("number").unwrap(), provider_version_number);
    }

    #[test]
    fn publish_provider_contract_with_missing_verification_results_file() {
        // Arrange - set up the command line arguments with a non-existent file
        let matches = add_publish_provider_contract_subcommand().get_matches_from(vec![
            "publish-provider-contract",
            "tests/fixtures/provider-contract.yaml",
            "-b",
            "http://localhost:1234",
            "--provider",
            "Bar",
            "--provider-app-version",
            "1",
            "--verification-results",
            "tests/fixtures/non-existent-file.txt",
            "--verification-results-content-type",
            "text/plain",
            "--verifier",
            "spectral",
        ]);

        // Act
        let result = publish(&matches);

        // Assert
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            crate::cli::pact_broker::main::PactBrokerError::IoError(_) => {
                // Expected IoError when verification results file does not exist
            }
            _ => panic!("Expected IoError but got: {:?}", error),
        }
    }

    #[test]
    fn publish_provider_contract_rejects_incomplete_self_verification_before_any_request() {
        let matches = add_publish_provider_contract_subcommand().get_matches_from(vec![
            "publish-provider-contract",
            "tests/fixtures/provider-contract.yaml",
            "-b",
            "http://localhost:1",
            "--provider",
            "Bar",
            "--provider-app-version",
            "1",
            "--verifier-version",
            "1.0.0",
        ]);

        match publish(&matches) {
            Err(crate::cli::pact_broker::main::PactBrokerError::ValidationError(errors)) => {
                let message = errors.join(" ");
                assert!(message.contains("--verification-results"), "was: {message}");
                assert!(
                    message.contains("--verification-results-content-type"),
                    "was: {message}"
                );
                assert!(message.contains("--verifier"), "was: {message}");
            }
            other => panic!("Expected ValidationError but got: {:?}", other),
        }
    }
}

#[cfg(test)]
mod publish_provider_contract_url_tests {
    use crate::cli::pact_broker::main::PactBrokerError;
    use crate::cli::pact_broker::main::test_utils::spawn_recording_broker_with_index;
    use crate::cli::pactflow::main::provider_contracts::publish::publish;
    use crate::cli::pactflow::main::subcommands::add_publish_provider_contract_subcommand;

    fn run(provider: &str) -> (Result<serde_json::Value, PactBrokerError>, Vec<String>) {
        let (broker_url, requests) = spawn_recording_broker_with_index(
            r#"{"pf:publish-provider-contract":{"href":"BASE/provider-contracts/provider/{provider}/publish","templated":true}}"#,
        );
        let matches = add_publish_provider_contract_subcommand().get_matches_from(vec![
            "publish-provider-contract",
            "tests/fixtures/provider-contract.yaml",
            "-b",
            broker_url.as_str(),
            "--provider",
            provider,
            "--provider-app-version",
            "1.0.0",
        ]);
        let result = publish(&matches);
        (result, requests.lock().unwrap().clone())
    }

    #[test]
    fn publish_encodes_provider() {
        assert_eq!(
            run("my provider/x").1,
            vec![
                "GET /",
                "POST /provider-contracts/provider/my%20provider%2Fx/publish"
            ]
        );
    }

    #[test]
    fn publish_rejects_dot_segment_provider() {
        let (result, requests) = run("..");
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("provider value '..' cannot be sent"),
            "{message}"
        );
        assert_eq!(requests, vec!["GET /"]);
    }
}
