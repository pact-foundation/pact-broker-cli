//! CLI to publish Pact files to a Pact broker.

#![warn(missing_docs)]

use std::fs::File;

use anyhow::{Context, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as Base64;
use clap::ArgMatches;
use log::*;

use glob::glob;
use pact_models::http_utils::HttpAuth;
use pact_models::{http_utils, pact};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::pact_broker::main::HALClient;
use crate::cli::pact_broker::main::utils::get_ssl_options;
use crate::cli::pact_broker::main::utils::{get_auth, get_broker_relation, get_broker_url};
use crate::cli::utils::{self, git_info};
use std::collections::HashMap;

use super::verification::{VerificationResult, display_results, verify_json};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Root {
    #[serde(rename = "_embedded")]
    pub embedded: Embedded,
    #[serde(rename = "_links")]
    pub links: Links3,
    pub logs: Vec<Log>,
    pub notices: Option<Vec<Notice>>,
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
    pub self_field: SelfField,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelfField {
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
    pub self_field: SelfField2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelfField2 {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Links3 {
    #[serde(rename = "pb:contracts")]
    pub pb_contracts: Vec<Contract>,
    #[serde(rename = "pb:pacticipant")]
    pub pb_pacticipant: PbPacticipant,
    #[serde(rename = "pb:pacticipant-version")]
    pub pb_pacticipant_version: PbPacticipantVersion,
    #[serde(rename = "pb:pacticipant-version-tags")]
    pub pb_pacticipant_version_tags: Vec<Value>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Contract {
    pub href: String,
    pub name: String,
    pub title: String,
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
struct Log {
    pub deprecation_warning: String,
    pub level: String,
    pub message: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Notice {
    pub text: String,
    #[serde(rename = "type")]
    pub type_field: String,
}

pub fn handle_matches(args: &ArgMatches) -> Result<Vec<VerificationResult>, i32> {
    if args.get_flag("validate") == false {
        return Ok(vec![]);
    }
    let files = load_files(args).map_err(|_| 1)?;
    let results: Vec<VerificationResult> = files
        .iter()
        .map(|(source, pact_json)| {
            let spec_version =
                pact::determine_spec_version(source, &pact::parse_meta_data(pact_json));
            let results = verify_json(pact_json, spec_version, source, args.get_flag("strict"));

            let verification_results = VerificationResult::new(source, results);
            verification_results
        })
        .collect();

    if results.is_empty() {
        println!("❌ No pact files found to publish");
        return Err(1);
    }
    let display_result = display_results(&results, "json");

    if display_result.is_err() {
        return Err(3);
    } else if results.iter().any(|res| res.has_errors()) {
        return Err(2);
    } else {
        return Ok(results);
    }
}

pub fn publish_pacts(args: &ArgMatches) -> Result<Value, i32> {
    let files: Result<Vec<(String, Value)>, anyhow::Error> = load_files(args);
    if files.is_err() {
        println!("{}", files.err().unwrap());
        return Err(1);
    }
    let files = files.map_err(|_| 1)?;

    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);
    let hal_client: HALClient =
        HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());

    let publish_pact_href_path = tokio::runtime::Runtime::new().unwrap().block_on(async {
        get_broker_relation(
            hal_client.clone(),
            "pb:publish-contracts".to_string(),
            broker_url.to_string(),
        )
        .await
    });

    match publish_pact_href_path {
        Ok(publish_pact_href) => {
            let mut consumer_app_version = args.get_one::<String>("consumer-app-version");
            let mut branch = args.get_one::<String>("branch");
            let auto_detect_version_properties: bool =
                args.get_flag("auto-detect-version-properties");
            let tag_with_git_branch = args.get_flag("tag-with-git-branch");
            let build_url = args.get_one::<String>("build-url");
            let (git_commit, git_branch);
            if auto_detect_version_properties {
                git_commit = git_info::commit(false);
                git_branch = git_info::branch(false);
            } else {
                git_commit = Some("".to_string());
                git_branch = Some("".to_string());
            }
            if auto_detect_version_properties {
                if consumer_app_version == None {
                    consumer_app_version = git_commit.as_ref();
                    println!(
                        "🔍 Auto detected git commit: {}",
                        consumer_app_version.unwrap().to_string()
                    );
                } else {
                    println!(
                        "🔍 auto_detect_version_properties set to {}, but consumer_app_version provided {}",
                        auto_detect_version_properties,
                        consumer_app_version.unwrap().to_string()
                    );
                }
                if branch == None {
                    branch = git_branch.as_ref();
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

            let on_conflict = if args.get_flag("merge") {
                "merge"
            } else {
                "overwrite"
            };
            let output: Result<Option<&String>, clap::parser::MatchesError> =
                args.try_get_one::<String>("output");
            // publish the pacts
            // Group pacts by (consumer, provider) pair and merge their interactions
            let mut merged_pacts: HashMap<(String, String), Value> = HashMap::new();
            for (source, pact_json) in files.iter() {
                tracing::debug!("Processing pact file: {}", source);

                // Load pact and extract consumer/provider names
                let pact_res = pact::load_pact_from_json(source, pact_json);
                if let Ok(pact) = &pact_res {
                    let consumer_name = pact.consumer().name.clone();
                    let provider_name = pact.provider().name.clone();
                    let key = (consumer_name.clone(), provider_name.clone());

                    tracing::debug!(
                        "Loaded pact for consumer: '{}' and provider: '{}'",
                        consumer_name,
                        provider_name
                    );

                    // If already present, merge interactions
                    if let Some(existing_json) = merged_pacts.get_mut(&key) {
                        tracing::debug!(
                            "Merging interactions for consumer: '{}' and provider: '{}'",
                            consumer_name,
                            provider_name
                        );
                        // Merge interactions arrays
                        if let (Some(existing_interactions), Some(new_interactions)) = (
                            existing_json.get_mut("interactions"),
                            pact_json.get("interactions"),
                        ) {
                            if let (Some(existing_arr), Some(new_arr)) = (
                                existing_interactions.as_array_mut(),
                                new_interactions.as_array(),
                            ) {
                                tracing::debug!(
                                    "Existing interactions: {}, New interactions: {}",
                                    existing_arr.len(),
                                    new_arr.len()
                                );
                                existing_arr.extend(new_arr.iter().cloned());
                                tracing::debug!(
                                    "Total interactions after merge: {}",
                                    existing_arr.len()
                                );
                            }
                        }
                    } else {
                        tracing::debug!(
                            "Inserting new pact for consumer: '{}' and provider: '{}'",
                            consumer_name,
                            provider_name
                        );
                        // Insert new pact
                        merged_pacts.insert(key, pact_json.clone());
                    }
                } else {
                    println!("❌ Failed to load pact from JSON: {:?}", pact_res);
                    error!("Failed to load pact from JSON: {:?}", pact_res);
                    return Err(1);
                }
            }

            // Publish merged pacts
            for ((consumer_name, provider_name), pact_json) in merged_pacts.iter() {
                let pact_res = pact::load_pact_from_json(
                    &format!("{}-{}", consumer_name, provider_name),
                    pact_json,
                );
                match pact_res {
                    Ok(pact) => {
                        let consumer_name = pact.consumer().name.clone();
                        let provider_name = pact.provider().name.clone();
                        let pact_spec = pact.specification_version();
                        let pact_json_data = pact.to_json(pact_spec).unwrap();
                        let mut payload = json!({});
                        payload["pacticipantName"] = Value::String(consumer_name.clone());
                        if consumer_app_version != None {
                            payload["pacticipantVersionNumber"] =
                                Value::String(consumer_app_version.unwrap().to_string());
                        } else {
                            println!("❌ Error: Consumer app version is required to publish pact");
                            return Err(1);
                        }
                        if branch != None {
                            payload["branch"] = Value::String(branch.unwrap().to_string());
                        }
                        if build_url != None {
                            payload["buildUrl"] = Value::String(build_url.unwrap().to_string());
                        }
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
                            payload["tags"].as_array_mut().unwrap().push(
                                serde_json::Value::String(
                                    git_info::commit(false).unwrap_or_default(),
                                ),
                            );
                        }

                        payload["contracts"] = serde_json::Value::Array(vec![json!({
                          "consumerName": consumer_name,
                          "providerName": provider_name,
                          "specification": "pact",
                          "contentType": "application/json",
                          "content": Base64.encode(pact_json_data.to_string()),
                          "onConflict": on_conflict
                        })]);
                        println!();
                        println!(
                            "📨 Attempting to publish pact for consumer: {} against provider: {}",
                            consumer_name, provider_name
                        );
                        let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
                            hal_client
                                .clone()
                                .post_json(&(publish_pact_href), &payload.to_string(), None)
                                .await
                        });
                        match res {
                            Ok(res) => match output {
                                Ok(Some(output)) => {
                                    if output == "pretty" {
                                        let json = serde_json::to_string_pretty(&res).unwrap();
                                        println!("{}", json);
                                    } else if output == "json" {
                                        let json: String =
                                            serde_json::to_string(&res.clone()).unwrap();
                                        println!("{}", json);
                                    } else {
                                        let parsed_res = serde_json::from_value::<Root>(res);
                                        match parsed_res {
                                            Ok(parsed_res) => {
                                                print!("✅ ");
                                                if let Some(notices) = parsed_res.notices {
                                                    notices.iter().for_each(|notice| match notice
                                                        .type_field
                                                        .as_str()
                                                    {
                                                        "success" => {
                                                            let notice_text =
                                                                notice.text.to_string();
                                                            let formatted_text = notice_text
                                                                .split_whitespace()
                                                                .map(|word| {
                                                                    if word.starts_with("https")
                                                                        || word.starts_with("http")
                                                                    {
                                                                        format!(
                                                                            "{}",
                                                                            utils::CYAN
                                                                                .apply_to(word)
                                                                        )
                                                                    } else {
                                                                        format!(
                                                                            "{}",
                                                                            utils::GREEN
                                                                                .apply_to(word)
                                                                        )
                                                                    }
                                                                })
                                                                .collect::<Vec<String>>()
                                                                .join(" ");
                                                            println!("{}", formatted_text)
                                                        }
                                                        "warning" | "prompt" => {
                                                            let notice_text =
                                                                notice.text.to_string();
                                                            let formatted_text = notice_text
                                                                .split_whitespace()
                                                                .map(|word| {
                                                                    if word.starts_with("https")
                                                                        || word.starts_with("http")
                                                                    {
                                                                        format!(
                                                                            "{}",
                                                                            utils::CYAN
                                                                                .apply_to(word)
                                                                        )
                                                                    } else {
                                                                        format!(
                                                                            "{}",
                                                                            utils::YELLOW
                                                                                .apply_to(word)
                                                                        )
                                                                    }
                                                                })
                                                                .collect::<Vec<String>>()
                                                                .join(" ");
                                                            println!("{}", formatted_text)
                                                        }
                                                        "error" | "danger" => {
                                                            let notice_text =
                                                                notice.text.to_string();
                                                            let formatted_text = notice_text
                                                                .split_whitespace()
                                                                .map(|word| {
                                                                    if word.starts_with("https")
                                                                        || word.starts_with("http")
                                                                    {
                                                                        format!(
                                                                            "{}",
                                                                            utils::CYAN
                                                                                .apply_to(word)
                                                                        )
                                                                    } else {
                                                                        format!(
                                                                            "{}",
                                                                            utils::RED
                                                                                .apply_to(word)
                                                                        )
                                                                    }
                                                                })
                                                                .collect::<Vec<String>>()
                                                                .join(" ");
                                                            println!("{}", formatted_text)
                                                        }
                                                        _ => println!("{}", notice.text),
                                                    });
                                                }
                                            }
                                            Err(err) => {
                                                println!(
                                                    "✅ Pact published successfully for consumer: {} against provider: {}",
                                                    consumer_name, provider_name
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
                                Err(1)?
                            }
                        }
                    }
                    _ => {
                        println!("❌ Failed to load pact from JSON: {:?}", pact_res);
                        return Err(1);
                    }
                }
            }
            Ok(json!({}))
        }
        Err(err) => {
            return Err(1);
        }
    }
}

pub fn load_files(args: &ArgMatches) -> anyhow::Result<Vec<(String, Value)>> {
    let mut collected: Vec<(String, anyhow::Result<Value>)> = Vec::new();

    if let Some(inputs) = args.get_many::<String>("pact-files-dirs-or-globs") {
        for input in inputs {
            let path = std::path::Path::new(input);

            match (path.exists(), path.is_file(), path.is_dir()) {
                (true, true, _) => {
                    collected.push((input.to_string(), load_file(input)));
                }
                (true, false, true) => match load_files_from_dir(input) {
                    Ok(files) => {
                        for (name, pact) in files {
                            collected.push((name, Ok(pact)));
                        }
                    }
                    Err(e) => collected.push((input.to_string(), Err(e))),
                },
                _ => {
                    // Treat as glob pattern
                    match glob(input) {
                        Ok(paths) => {
                            for entry in paths {
                                match entry {
                                    Ok(pathbuf) => {
                                        if let Some(fname) = pathbuf.to_str() {
                                            collected.push((fname.to_string(), load_file(fname)));
                                        }
                                    }
                                    Err(e) => collected.push((input.to_string(), Err(anyhow!(e)))),
                                }
                            }
                        }
                        Err(e) => collected.push((input.to_string(), Err(anyhow!(e)))),
                    }
                }
            }
        }
    }

    let failures: Vec<_> = collected.iter().filter(|(_, res)| res.is_err()).collect();
    if !failures.is_empty() {
        error!("Failed to load the following pact files:");
        for (src, err) in failures {
            error!("    '{}' - {}", src, err.as_ref().unwrap_err());
        }
        Err(anyhow!("One or more pact files could not be loaded"))
    } else {
        Ok(collected
            .into_iter()
            .map(|(src, res)| (src, res.unwrap()))
            .collect())
    }
}

fn fetch_pact(url: &str, args: &ArgMatches) -> anyhow::Result<(String, Value)> {
    let auth = if args.contains_id("user") {
        args.get_one::<String>("password").map(|user| {
            HttpAuth::User(
                user.to_string(),
                args.get_one::<String>("password").map(|p| p.to_string()),
            )
        })
    } else if args.contains_id("token") {
        args.get_one::<String>("token")
            .map(|token| HttpAuth::Token(token.to_string()))
    } else {
        None
    };
    http_utils::fetch_json_from_url(&url.to_string(), &auth)
}

fn load_file(file_name: &str) -> anyhow::Result<Value> {
    let file = File::open(file_name)?;
    serde_json::from_reader(file).context("file is not JSON")
}

pub fn load_files_from_dir(dir: &str) -> anyhow::Result<Vec<(String, Value)>> {
    let mut sources: Vec<(String, anyhow::Result<Value>)> = vec![];

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_path = entry.path();
        if file_path.is_file()
            && file_path
                .extension()
                .map(|ext| ext == "json")
                .unwrap_or(false)
        {
            let file_name = file_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(anyhow!("Invalid file name"))?;
            sources.push((
                file_name.to_string(),
                load_file(file_path.to_str().unwrap()),
            ));
        }
    }

    if sources.iter().any(|(_, res)| res.is_err()) {
        error!("Failed to load the following pact files:");
        for (source, result) in sources.iter().filter(|(_, res)| res.is_err()) {
            error!("    '{}' - {}", source, result.as_ref().unwrap_err());
        }
        Err(anyhow!("Failed to load one or more pact files"))
    } else {
        Ok(sources
            .iter()
            .map(|(source, result)| (source.clone(), result.as_ref().unwrap().clone()))
            .collect())
    }
}

#[cfg(test)]
mod publish_contracts_tests {
    use crate::cli::pact_broker::main::pact_publish::publish_pacts;
    use crate::cli::pact_broker::main::subcommands::add_publish_pacts_subcommand;
    use base64::{Engine, engine::general_purpose::STANDARD as Base64};
    use pact_consumer::prelude::*;
    use pact_models::prelude::Generator;
    use pact_models::{PactSpecification, generators};
    use serde_json::{Value, json};
    use std::fs::File;
    use std::io::Read;

    #[test]
    fn publish_contracts_success() {
        // Arrange - set up the pact mock server (as v2 for compatibility with pact-ruby)
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };
        let pacticipant_name = "Foo";
        let provider_name = "Bar";
        let version_number = "5556b8149bf8bac76bc30f50a8a2dd4c22c85f30";
        let branch = "main";
        let tag = "dev";
        let build_url = "http://build";
        let pact_file_path = "tests/fixtures/foo-bar.json";

        // Load pact file and encode content
        let mut pact_file = File::open(pact_file_path).expect("Fixture pact file missing");
        let mut pact_json_str = String::new();
        pact_file.read_to_string(&mut pact_json_str).unwrap();
        let mut pact_json: serde_json::Value = serde_json::from_str(&pact_json_str).unwrap();
        // Merge with existing keys in metadata if present
        let mut metadata = pact_json
            .get("metadata")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert(
                "pactRust".to_string(),
                json!({ "models": pact_models::PACT_RUST_VERSION }),
            );
            pact_json["metadata"] = Value::Object(obj.clone());
        } else {
            pact_json["metadata"] = json!({
            "pactRust": { "models": pact_models::PACT_RUST_VERSION },
            });
        }
        let expected_content = Base64.encode(pact_json.to_string());

        let request_body = json!({
            "pacticipantName": pacticipant_name,
            "pacticipantVersionNumber": version_number,
            "branch": branch,
            "tags": [tag],
            "buildUrl": build_url,
            "contracts": [
                {
                    "consumerName": pacticipant_name,
                    "providerName": provider_name,
                    "specification": "pact",
                    "contentType": "application/json",
                    "content": expected_content,
                    "onConflict": "merge"
                }
            ]
        });

        let contract_path_generator = generators! {
            "BODY" => {
            "$._links.pb:pb:publish-contracts.href" => Generator::MockServerURL(
                            "/contracts/publish".to_string(),
                            ".*\\/contracts\\/publish".to_string()
            )
            }
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for the index resource", "", |mut i| {
                i.given("the pb:publish-contracts relations exists in the index resource");
                i.request
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:publish-contracts": {
                                "href": term!("http:\\/\\/.*\\/contracts\\/publish", "http://localhost:1234/contracts/publish"),
                                "title": "Publish contracts",
                                "templated": false
                            }
                        }
                    }))
                    .generators()
                    .add_generators(contract_path_generator);
                i
            })
            .interaction("a request to publish contracts", "", |mut i| {
                i.request
                    .post()
                    .path("/contracts/publish")
                    .header("Content-Type", "application/json")
                    .json_body(request_body.clone());
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_embedded": {
                            "pacticipant": {
                                "name": pacticipant_name
                            },
                            "version": {
                                "number": version_number
                            }
                        },
                        "logs": each_like!({
                            "level": "info",
                            "message": "some message"
                        }),
                        "_links": {
                            "pb:pacticipant-version-tags": each_like!({ "name": tag }),
                            "pb:contracts": each_like!({ "href": like!("http://some-pact") })
                        }
                    }));
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange - set up the command line arguments
        let matches = add_publish_pacts_subcommand().get_matches_from(vec![
            "publish",
            pact_file_path,
            "-b",
            mock_server_url.as_str(),
            "--consumer-app-version",
            version_number,
            "--branch",
            branch,
            "--tag",
            tag,
            "--build-url",
            build_url,
            "--merge",
        ]);

        // Act
        let result = publish_pacts(&matches);

        // Assert
        assert!(result.is_ok());
        let value = result.unwrap();

        assert!(value.is_object());
    }
}
