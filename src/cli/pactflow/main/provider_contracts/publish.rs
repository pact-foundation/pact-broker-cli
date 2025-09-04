use base64::Engine;
use base64::engine::general_purpose::STANDARD as Base64;
use clap::ArgMatches;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::{
    pact_broker::main::{
        HALClient,
        pact_publish::{get_git_branch, get_git_commit},
        utils::{get_auth, get_broker_relation, get_broker_url, get_ssl_options, handle_error},
    },
    utils,
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
    // pacticipant: Pacticipant,
    version: Version,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pacticipant {
    name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelfField {
    href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    number: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Links {
    // #[serde(rename = "pb:pacticipant")]
    // pb_pacticipant: PbPacticipant,
    // #[serde(rename = "pb:pacticipant-version")]
    // pb_pacticipant_version: PbPacticipantVersion,
    #[serde(rename = "pb:pacticipant-version-tags")]
    pb_pacticipant_version_tags: Vec<Value>,
    // #[serde(rename = "pf:provider-contract")]
    // pf_provider_contract: PfProviderContract,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PbPacticipant {
    href: String,
    name: String,
    title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PbPacticipantVersion {
    href: String,
    name: String,
    title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PfProviderContract {
    href: String,
    name: String,
    title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Notice {
    text: String,
    #[serde(rename = "type")]
    type_field: String,
}

pub fn publish(args: &ArgMatches) -> Result<Value, i32> {
    // Load contract file
    let contract_file = args
        .get_one::<String>("contract-file")
        .expect("CONTRACT_FILE is required");
    let contract_content = std::fs::read_to_string(contract_file).map_err(|e| {
        println!("❌ Failed to read contract file: {}", e);
        1
    })?;

    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);
    let hal_client: HALClient =
        HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());

    // Use pf:publish-provider-contract relation
    let publish_contract_href_path = tokio::runtime::Runtime::new().unwrap().block_on(async {
        get_broker_relation(
            hal_client.clone(),
            "pf:publish-provider-contract".to_string(),
            broker_url.to_string(),
        )
        .await
    });
    println!("🔍 Using broker: {}", broker_url);
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
            let specification = args
                .get_one::<String>("specification")
                .unwrap_or(&default_specification);
            let default_content_type = "application/yaml".to_string();
            let content_type = args
                .get_one::<String>("content-type")
                .unwrap_or(&default_content_type);
            let auto_detect_version_properties: bool =
                args.get_flag("auto-detect-version-properties");
            let (git_commit, git_branch);
            if auto_detect_version_properties {
                git_commit = get_git_commit();
                git_branch = get_git_branch();
            } else {
                git_commit = "".to_string();
                git_branch = "".to_string();
            }
            if auto_detect_version_properties {
                if provider_app_version == None {
                    provider_app_version = Some(&git_commit);
                    println!(
                        "🔍 Auto detected git commit: {}",
                        provider_app_version.unwrap().to_string()
                    );
                } else {
                    println!(
                        "🔍 auto_detect_version_properties set to {}, but provider_app_version provided {}",
                        auto_detect_version_properties,
                        provider_app_version.unwrap().to_string()
                    );
                }
                if branch == None {
                    branch = Some(&git_branch);
                    println!(
                        "🔍 Auto detected git branch: {}",
                        branch.unwrap().to_string()
                    );
                } else {
                    println!(
                        "🔍 auto_detect_version_properties set to {}, but branch provided {}",
                        auto_detect_version_properties,
                        branch.unwrap().to_string()
                    );
                }
            }
            let publish_contract_href = publish_contract_href.replace("{provider}", provider_name);

            // Verification results
            let verification_exit_code = args.get_one::<String>("verification-exit-code");
            let verification_success = if args.contains_id("verification-success")
                && args.get_flag("verification-success")
            {
                true
            } else if args.contains_id("no-verification-success")
                && args.get_flag("no-verification-success")
            {
                false
            } else if let Some(exit_code_str) = verification_exit_code {
                match exit_code_str.parse::<i32>() {
                    Ok(code) => code == 0,
                    Err(_) => false,
                }
            } else {
                false
            };

            let verification_results = args.get_one::<String>("verification-results");
            let verification_results_content_type =
                args.get_one::<String>("verification-results-content-type");
            let verification_results_format = args.get_one::<String>("verification-results-format");
            let verifier = args.get_one::<String>("verifier");
            let verifier_version = args.get_one::<String>("verifier-version");

            // Build contract params
            let mut contract_params = json!({
                "content": Base64.encode(contract_content),
                "specification": specification,
                "contentType": content_type,
            });

            // Add selfVerificationResults if provided
            if verification_results.is_some() || verifier.is_some() || verifier_version.is_some() {
                let mut verification_results_params = serde_json::Map::new();
                verification_results_params
                    .insert("success".to_string(), Value::Bool(verification_success));
                if let Some(content) = verification_results {
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
                if !payload.get("tags").map_or(false, |v| v.is_array()) {
                    payload["tags"] = serde_json::Value::Array(vec![]);
                }
                payload["tags"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::Value::String(get_git_branch().to_string()));
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
                provider_app_version.unwrap().to_string()
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
                                    parsed_res.notices.iter().for_each(|notice| {
                                        match notice.type_field.as_str() {
                                            "success" => {
                                                println!("{}", utils::GREEN.apply_to(&notice.text))
                                            }
                                            "warning" | "prompt" => {
                                                println!("{}", utils::YELLOW.apply_to(&notice.text))
                                            }
                                            "error" | "danger" => {
                                                println!("{}", utils::RED.apply_to(&notice.text))
                                            }
                                            _ => println!("{}", notice.text),
                                        }
                                    });
                                }
                                Err(err) => {
                                    println!(
                                        "✅ Provider contract published successfully for provider: {} version: {}",
                                        provider_name,
                                        provider_app_version.unwrap().to_string()
                                    );
                                    println!(
                                        "⚠️ Warning: Failed to process response notices - Error: {:?}",
                                        err
                                    );
                                }
                            }
                        }
                    }
                    _ => {
                        println!("{:?}", res.clone());
                    }
                },
                Err(err) => {
                    println!("❌ {}", err.to_string());
                }
            }
            Ok(json!({}))
        }
        Err(err) => {
            handle_error(err);
            return Err(1);
        }
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
                           format!("/contracts/publish/{}", provider_name.to_string()),
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
        let matches = add_publish_provider_contract_subcommand()
            .args(crate::cli::add_ssl_arguments())
            .get_matches_from(vec![
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
                verification_results_content,
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
}
