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

use crate::cli::pact_broker::main::utils::get_ssl_options;
use crate::cli::utils;
use crate::pact_broker::main::HALClient;
use crate::pact_broker::main::utils::{
    get_auth, get_broker_relation, get_broker_url, handle_error,
};

use super::verification::{VerificationResult, display_results, verify_json};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    #[serde(rename = "_embedded")]
    pub embedded: Embedded,
    #[serde(rename = "_links")]
    pub links: Links3,
    pub logs: Vec<Log>,
    pub notices: Vec<Notice>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Embedded {
    pub pacticipant: Pacticipant,
    pub version: Version,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pacticipant {
    #[serde(rename = "_links")]
    pub links: Links,
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    #[serde(rename = "self")]
    pub self_field: SelfField,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfField {
    pub href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    #[serde(rename = "_links")]
    pub links: Links2,
    pub number: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links2 {
    #[serde(rename = "self")]
    pub self_field: SelfField2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfField2 {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links3 {
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
pub struct Contract {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PbPacticipant {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PbPacticipantVersion {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Log {
    pub deprecation_warning: String,
    pub level: String,
    pub message: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub text: String,
    #[serde(rename = "type")]
    pub type_field: String,
}

pub fn handle_matches(args: &ArgMatches) -> Result<Vec<VerificationResult>, i32> {
    if args.get_flag("validate") == false {
        return Ok(vec![]);
    }
    let files = load_files(args).map_err(|_| 1)?;
    let results = files
        .iter()
        .map(|(source, pact_json)| {
            let spec_version =
                pact::determine_spec_version(source, &pact::parse_meta_data(pact_json));
            let results = verify_json(pact_json, spec_version, source, args.get_flag("strict"));

            let verification_results = VerificationResult::new(source, results);
            verification_results
        })
        .collect();

    let display_result = display_results(&results, "console");
    if display_result.is_err() {
        return Err(3);
    } else if results.iter().any(|res| res.has_errors()) {
        return Err(2);
    } else {
        return Ok(results);
    }
}

fn get_git_branch() -> String {
    let git_branch_output = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .output()
        .expect("Failed to get git branch");
    let git_branch = std::str::from_utf8(&git_branch_output.stdout)
        .unwrap()
        .trim();
    return git_branch.to_string();
}

fn get_git_commit() -> String {
    let git_commit_output = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .expect("Failed to get git commit");
    let git_commit = std::str::from_utf8(&git_commit_output.stdout)
        .unwrap()
        .trim();
    return git_commit.to_string();
}

pub fn publish_pacts(args: &ArgMatches) -> Result<Value, i32> {
    let files = load_files(args);
    if files.is_err() {
        println!("{}", files.err().unwrap());
        return Err(1);
    }
    let files = files.map_err(|_| 1)?;

    let broker_url = get_broker_url(args);
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
    println!("🔗 Broker URL: {:?}", publish_pact_href_path);

    match publish_pact_href_path {
        Ok(publish_pact_href) => {
            let mut consumer_app_version = args.get_one::<String>("consumer-app-version");
            let mut branch = args.get_one::<String>("branch");
            let auto_detect_version_properties = args.get_flag("auto-detect-version-properties");
            let _tag_with_git_branch = args.get_flag("tag-with-git-branch");
            let build_url = args.get_one::<String>("build-url");
            let git_commit = get_git_commit();
            let git_branch = get_git_branch();
            if auto_detect_version_properties == true {
                if consumer_app_version == None {
                    consumer_app_version = Some(&git_commit);
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

            let on_conflict = if args.get_flag("merge") {
                "merge"
            } else {
                "overwrite"
            };
            let output: Result<Option<&String>, clap::parser::MatchesError> =
                args.try_get_one::<String>("output");
            // publish the pacts
            for (source, pact_json) in files.iter() {
                let pact_res = pact::load_pact_from_json(source, pact_json);
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
                                .post_json(&(publish_pact_href), &payload.to_string())
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
                                                parsed_res.notices.iter().for_each(|notice| {
                                                    match notice.type_field.as_str() {
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
                                                    }
                                                });
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
                            }
                        }
                    }
                    _ => {
                        println!("❌ Failed to load pact from JSON: {:?}", pact_res);
                    }
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

pub fn load_files(args: &ArgMatches) -> anyhow::Result<Vec<(String, Value)>> {
    let mut sources: Vec<(String, anyhow::Result<Value>)> = vec![];
    if let Some(values) = args.get_many::<String>("dir") {
        for value in values {
            let files = load_files_from_dir(value)?;
            for (source, pact_json) in files {
                sources.push((source, Ok(pact_json)));
            }
        }
    };
    if let Some(values) = args.get_many::<String>("file") {
        sources.extend(
            values
                .map(|v| (v.to_string(), load_file(v)))
                .collect::<Vec<(String, anyhow::Result<Value>)>>(),
        );
    };
    if let Some(values) = args.get_many::<String>("url") {
        sources.extend(
            values
                .map(|v| (v.to_string(), fetch_pact(v, args).map(|(_, value)| value)))
                .collect::<Vec<(String, anyhow::Result<Value>)>>(),
        );
    };

    if let Some(values) = args.get_many::<String>("glob") {
        for value in values {
            for entry in glob(value)? {
                let entry = entry?;
                let file_name = entry
                    .to_str()
                    .ok_or(anyhow!("Glob matched non-UTF-8 entry"))?;
                sources.push((file_name.to_string(), load_file(file_name)));
            }
        }
    };

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
