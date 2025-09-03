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
pub struct ProviderContractPublishRoot {
    #[serde(rename = "_embedded")]
    pub embedded: Embedded,
    #[serde(rename = "_links")]
    pub links: Links3,
    pub notices: Vec<Notice>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Embedded {
    pub pacticipant: Pacticipant,
    pub version: Version,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pacticipant {
    #[serde(rename = "_links")]
    pub links: Links,
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Links {
    #[serde(rename = "self")]
    pub self_field: Self_field,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Self_field {
    pub href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    #[serde(rename = "_links")]
    pub links: Links2,
    pub number: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Links2 {
    #[serde(rename = "self")]
    pub self_field: Self_field2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Self_field2 {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Links3 {
    #[serde(rename = "pb:pacticipant")]
    pub pb_pacticipant: PbPacticipant,
    #[serde(rename = "pb:pacticipant-version")]
    pub pb_pacticipant_version: PbPacticipantVersion,
    #[serde(rename = "pb:pacticipant-version-tags")]
    pub pb_pacticipant_version_tags: Vec<Value>,
    #[serde(rename = "pf:provider-contract")]
    pub pf_provider_contract: PfProviderContract,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PbPacticipant {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PbPacticipantVersion {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PfProviderContract {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Notice {
    pub text: String,
    #[serde(rename = "type")]
    pub type_field: String,
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
    // parse link url provider with provider_name

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
            let verification_success = args.get_flag("verification-success");
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
                    .post_json(&(publish_contract_href), &payload.to_string())
                    .await
            });
            match res {
                Ok(res) => match output {
                    Ok(Some(output)) => {
                        if output == "pretty" {
                            let json = serde_json::to_string_pretty(&res).unwrap();
                            println!("{}", json);
                        } else if output == "json" {
                            let json: String = serde_json::to_string(&res.clone()).unwrap();
                            println!("{}", json);
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
