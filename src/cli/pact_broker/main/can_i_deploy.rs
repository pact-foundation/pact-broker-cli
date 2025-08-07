use comfy_table::{Table, presets::UTF8_FULL};
use tracing::debug;

use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_ssl_options, handle_error},
    },
    utils,
};

#[derive(Debug, serde::Deserialize)]
struct Summary {
    deployable: Option<bool>,
    reason: String,
    success: u32,
    failed: u32,
    unknown: u32,
}

#[derive(Debug, serde::Deserialize)]
struct Notice {
    #[serde(rename = "type")]
    notice_type: String,
    text: String,
}

#[derive(Debug, serde::Deserialize)]
struct Version {
    number: String,
    branch: String,
    branches: Vec<Branch>,
    #[serde(rename = "branchVersions")]
    branch_versions: Vec<BranchVersion>,
    environments: Vec<Environment>,
    _links: Links,
    tags: Vec<Tag>,
}

#[derive(Debug, serde::Deserialize)]
struct Branch {
    name: String,
    latest: Option<bool>,
    _links: Links,
}

#[derive(Debug, serde::Deserialize)]
struct BranchVersion {
    name: String,
    latest: Option<bool>,
    _links: Links,
}

#[derive(Debug, serde::Deserialize)]
struct Environment {
    uuid: String,
    name: String,
    #[serde(rename = "displayName")]
    display_name: String,
    production: Option<bool>,
    #[serde(rename = "createdAt")]
    created_at: String,
    _links: Links,
}

#[derive(Debug, serde::Deserialize)]
struct Links {
    #[serde(rename = "self")]
    self_link: SelfLink,
}

#[derive(Debug, serde::Deserialize)]
struct SelfLink {
    href: String,
}

#[derive(Debug, serde::Deserialize)]
struct Tag {
    name: String,
    latest: Option<bool>,
    _links: Links,
}

#[derive(Debug, serde::Deserialize)]
struct Consumer {
    name: String,
    version: Option<Version>,
    _links: Links,
}

#[derive(Debug, serde::Deserialize)]
struct Provider {
    name: String,
    version: Option<Version>,
    _links: Links,
}

#[derive(Debug, serde::Deserialize)]
struct Pact {
    #[serde(rename = "createdAt")]
    created_at: String,
    _links: Links,
}

#[derive(Debug, serde::Deserialize)]
struct VerificationResult {
    success: Option<bool>,
    #[serde(rename = "verifiedAt")]
    verified_at: Option<String>,
    _links: Links,
}

#[derive(Debug, serde::Deserialize)]
struct MatrixItem {
    consumer: Consumer,
    provider: Provider,
    pact: Pact,
    #[serde(rename = "verificationResult")]
    verification_result: Option<VerificationResult>,
}

#[derive(Debug, serde::Deserialize)]
struct Data {
    summary: Summary,
    notices: Vec<Notice>,
    matrix: Vec<MatrixItem>,
}

pub fn can_i_deploy(args: &clap::ArgMatches, can_i_merge: bool) -> Result<String, PactBrokerError> {
    let pacticipant = args.get_one::<String>("pacticipant");
    let version = args.get_one::<String>("version");
    let _ignore = args.try_get_one::<String>("ignore").unwrap_or(None);
    let latest = args
        .try_get_one::<bool>("latest")
        .unwrap_or(Some(&false))
        .copied()
        .unwrap_or(false);
    let branch = args.try_get_one::<String>("branch").unwrap_or(None);
    let main_branch = args
        .try_get_one::<bool>("main-branch")
        .unwrap_or(Some(&false))
        .copied()
        .unwrap_or(false);
    let to_environment = args.try_get_one::<String>("to-environment").unwrap_or(None);
    let to = args.try_get_one::<String>("to").unwrap_or(None);
    let _retry_while_unknown = args.get_one::<String>("retry-while-unknown");
    let _retry_interval = args.get_one::<String>("retry-interval");
    let dry_run = args.get_flag("dry-run");

    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());
        // let matrix_href_path = "/matrix?q[][pacticipant]=Example+App&q[][latest]=true&q[][branch]=foo&latestby=cvp&latest=true".to_string();
        // let matrix_href_path = "/matrix?q[][pacticipant]=Example+App&q[][version]=5556b8149bf8bac76bc30f50a8a2dd4c22c85f30&latestby=cvp&latest=true".to_string();
        let mut matrix_href_path = "/matrix?".to_string();

        if let Some(pacticipant) = pacticipant {
            matrix_href_path.push_str(&format!("q[][pacticipant]={}&", pacticipant));
        }

        if let Some(version) = version {
            matrix_href_path.push_str(&format!("q[][version]={}&", version));
        }

        if latest || version.is_none() {
            matrix_href_path.push_str("q[][latest]=true&");
        }

        if main_branch {
            matrix_href_path.push_str("q[][mainBranch]=true&");
        }

        if let Some(branch) = branch {
            matrix_href_path.push_str(&format!("q[][branch]={}&", branch));
        }

        if let Some(to) = to {
            matrix_href_path.push_str(&format!("tag={}&", to));
        }

        matrix_href_path.push_str("latestby=cvp");

        if let Some(to_environment) = to_environment {
            matrix_href_path.push_str(&format!("&environment={}", to_environment));
        }
        if to_environment.is_none() {
            matrix_href_path.push_str("&latest=true");
        }
        if can_i_merge {
            matrix_href_path.push_str("&mainBranch=true");
        }
        debug!(
            "Querying broker at: {}",
            broker_url.clone() + &matrix_href_path
        );
        let res = hal_client
            .clone()
            .fetch(&(broker_url.clone() + &matrix_href_path))
            .await;
        match res {
            Ok(res) => {
                let output: Result<Option<&String>, clap::parser::MatchesError> =
                    args.try_get_one::<String>("output");

                match output {
                    Ok(Some(output)) => {
                        if output == "json" {
                            let json: String = serde_json::to_string(&res.clone()).unwrap();
                            println!("{}", json);
                            Ok(json)
                        } else {

                            let data: Data = match serde_json::from_str(&res.clone().to_string()) {
                                Ok(data) => data,
                                Err(err) => {
                                    println!("❌ {}", utils::RED.apply_to(err.to_string()));
                                    Data {
                                        summary: Summary {
                                            deployable: Some(false),
                                            success: 0,
                                            failed: 0,
                                            reason: "No summary found".to_string(),
                                            unknown: 1,
                                        },
                                        notices: Vec::new(),
                                        matrix: Vec::new(),
                                    }
                                }
                            };

                            if data.matrix.len() > 0 {
                                let mut table = Table::new();

                                table.load_preset(UTF8_FULL).set_header(vec![
                                    "CONSUMER",
                                    "C.VERSION",
                                    "PROVIDER",
                                    "P.VERSION",
                                    "SUCCESS?",
                                    "RESULT",
                                ]);
                                for matrix_item in data.matrix {
                                    let verification_result = &matrix_item
                                        .verification_result
                                        .map(|result| result.success.unwrap_or(false).to_string())
                                        .unwrap_or_else(|| "false".to_string());

                                    table.add_row(vec![
                                        matrix_item.consumer.name,
                                        matrix_item
                                            .consumer
                                            .version
                                            .map(|result| result.number.to_string())
                                            .unwrap_or_else(|| "unknown".to_string()),
                                        matrix_item.provider.name,
                                        matrix_item
                                            .provider
                                            .version
                                            .map(|result| result.number.to_string())
                                            .unwrap_or_else(|| "unknown".to_string()),
                                        verification_result.to_string(),
                                        verification_result.to_string(),
                                    ]);
                                }
                                println!("{table}");
                            }

                            if data.notices.len() > 0 {
                                for notice in data.notices {
                                    if notice.notice_type == "warning" {
                                        println!("⚠️ {}", utils::YELLOW.apply_to(notice.text));
                                    } else if notice.notice_type == "error" {
                                        println!("❌ {}", utils::RED.apply_to(notice.text));
                                    } else {
                                        println!("📌 {}", utils::GREEN.apply_to(notice.text));
                                    }
                                }
                            }
                            if data.summary.deployable.unwrap_or(false) {
                                let computer_says_yes = utils::GREEN.apply_to("\\o/");
                                let message = format!("✅ Computer says yes {}", computer_says_yes);
                                println!("{}", message);
                                Ok(message)
                            } else {
                                let computer_says_no = utils::RED.apply_to("¯\\_(ツ)_/¯");
                                println!(r"❌ Computer says no {}", computer_says_no);
                                if dry_run == true {
                                    let message =
                                        "📌 Dry run enabled, suppressing failing exit code"
                                            .to_string();
                                    println!("{}", utils::YELLOW.apply_to(message.clone()));
                                    Ok(message)
                                } else {
                                    Err(PactBrokerError::NotFound(
                                        "No deployable version found".to_string(),
                                    ))
                                }
                            }
                        }
                    }
                    Err(res) => {
                        let message = format!(
                            "❌ No output match provided for {}",
                            res.clone().to_string()
                        );
                        println!("{}", utils::RED.apply_to(message.clone()));
                        Err(PactBrokerError::ValidationError([message].to_vec()))
                    }
                    _ => {
                        let message = res.clone().to_string();
                        println!("{}", message.clone());
                        Ok(message)
                    }
                }
            }
            Err(res) => Err(handle_error(res)),
        }
    })
}
