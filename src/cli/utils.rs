use std::str::FromStr;

use console::Style;
use tracing_core::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn setup_loggers(level: &str) {
    let log_level = match level {
        "none" => LevelFilter::OFF,
        _ => LevelFilter::from_str(level).unwrap_or(LevelFilter::INFO),
    };

    tracing_subscriber::registry()
        .with({
            if log_level != LevelFilter::OFF {
                Some(
                    tracing_subscriber::fmt::layer()
                        .compact()
                        .with_thread_names(true)
                        .with_level(true),
                )
            } else {
                None
            }
        })
        .with({
            if log_level != LevelFilter::OFF {
                Some(tracing_subscriber::filter::LevelFilter::from_level(
                    log_level.into_level().unwrap(),
                ))
            } else {
                None
            }
        })
        .try_init()
        .unwrap_or_else(|err| eprintln!("ERROR: Failed to initialise loggers - {err}"));
}

pub fn glob_value(v: String) -> Result<String, String> {
    match glob::Pattern::new(&v) {
        Ok(res) => Ok(res.to_string()),
        Err(err) => Err(format!("'{}' is not a valid glob pattern - {}", v, err)),
    }
}

pub const RED: Style = Style::new().red();
pub const GREEN: Style = Style::new().green();
pub const YELLOW: Style = Style::new().yellow();
pub const CYAN: Style = Style::new().cyan();

/// A simple [`dbg!`](https://doc.rust-lang.org/std/macro.dbg.html)-like macro to help debugging `reqwest` calls.
/// Uses `tracing::debug!` instead of `eprintln!`.
#[macro_export]
macro_rules! dbg_as_curl {
    ($req:expr) => {
        match $req {
            tmp => {
                match tmp.try_clone().map(|b| b.build()) {
                    Some(Ok(req)) => tracing::debug!("{}", crate::cli::utils::AsCurl::new(&req)),
                    Some(Err(err)) => tracing::debug!("*Error*: {}", err),
                    None => tracing::debug!("*Error*: request not cloneable",),
                }
                tmp
            }
        }
    };
}

/// A wrapper around a request that displays as a cURL command.
pub struct AsCurl<'a> {
    req: &'a reqwest::Request,
}

impl<'a> AsCurl<'a> {
    /// Construct an instance of `AsCurl` with the given request.
    pub fn new(req: &'a reqwest::Request) -> AsCurl<'a> {
        Self { req }
    }
}

impl<'a> std::fmt::Debug for AsCurl<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        <Self as std::fmt::Display>::fmt(self, f)
    }
}

impl<'a> std::fmt::Display for AsCurl<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let AsCurl { req } = *self;

        write!(f, "curl ")?;

        let method = req.method();
        if method != "GET" {
            write!(f, "-X {} ", method)?;
        }

        for (name, value) in req.headers() {
            let value = value
                .to_str()
                .expect("Headers must contain only visible ASCII characters")
                .replace("'", r"'\''");

            write!(f, "--header '{}: {}' ", name, value)?;
        }

        // Body
        if let Some(body) = req.body() {
            // Try to get bytes if possible
            if let Some(bytes) = body.as_bytes() {
                let s = String::from_utf8_lossy(bytes).replace("'", r"'\''");
                write!(f, "--data-raw '{}' ", s)?;
            } else {
                write!(
                    f,
                    "# NOTE: Body present but not shown (stream or unknown type) "
                )?;
            }
        }

        // URL
        write!(f, "'{}'", req.url().to_string().replace("'", "%27"))?;

        Ok(())
    }
}

#[cfg(test)]
mod debug_as_curl_tests {

    use crate::dbg_as_curl;

    fn compare(req: reqwest::RequestBuilder, result: &str) {
        let req = dbg_as_curl!(req);

        let req = req.build().unwrap();
        assert_eq!(format!("{}", super::AsCurl::new(&req)), result);
    }

    #[test]
    fn basic() {
        let client = reqwest::Client::new();

        compare(
            client.get("http://example.org"),
            "curl 'http://example.org/'",
        );
        compare(
            client.get("https://example.org"),
            "curl 'https://example.org/'",
        );
    }

    #[test]
    fn escape_url() {
        let client = reqwest::Client::new();

        compare(
            client.get("https://example.org/search?q='"),
            "curl 'https://example.org/search?q=%27'",
        );
    }

    #[test]
    fn bearer() {
        let client = reqwest::Client::new();

        compare(
            client.get("https://example.org").bearer_auth("foo"),
            "curl --header 'authorization: Bearer foo' 'https://example.org/'",
        );
    }

    #[test]
    fn escape_headers() {
        let client = reqwest::Client::new();

        compare(
            client.get("https://example.org").bearer_auth("test's"),
            r"curl --header 'authorization: Bearer test'\''s' 'https://example.org/'",
        );
    }

    // The body cannot be included as there is not API to retrieve its content.
    #[test]
    fn body() {
        let client = reqwest::Client::new();

        compare(
            client.get("https://example.org").body("test's"),
            r"curl --data-raw 'test'\''s' 'https://example.org/'",
        );
    }
}

pub mod git_info {
    use std::env;
    use std::process::Command;

    const BRANCH_ENV_VAR_NAMES: &[&str] = &[
        "GITHUB_HEAD_REF",
        "GITHUB_REF",
        "BUILDKITE_BRANCH",
        "CIRCLE_BRANCH",
        "TRAVIS_BRANCH",
        "GIT_BRANCH",
        "GIT_LOCAL_BRANCH",
        "APPVEYOR_REPO_BRANCH",
        "CI_COMMIT_REF_NAME",
        "BITBUCKET_BRANCH",
        "BUILD_SOURCEBRANCHNAME",
        "CIRRUS_BRANCH",
    ];

    const COMMIT_ENV_VAR_NAMES: &[&str] = &[
        "GITHUB_SHA",
        "BUILDKITE_COMMIT",
        "CIRCLE_SHA1",
        "TRAVIS_COMMIT",
        "GIT_COMMIT",
        "APPVEYOR_REPO_COMMIT",
        "CI_COMMIT_ID",
        "BITBUCKET_COMMIT",
        "BUILD_SOURCEVERSION",
        "CIRRUS_CHANGE_IN_REPO",
    ];

    const BUILD_URL_ENV_VAR_NAMES: &[&str] = &[
        "BUILDKITE_BUILD_URL",
        "CIRCLE_BUILD_URL",
        "TRAVIS_BUILD_WEB_URL",
        "BUILD_URL",
    ];

    pub fn commit(raise_error: bool) -> Option<String> {
        find_commit_from_env_vars().or_else(|| commit_from_git_command(raise_error))
    }

    pub fn branch(raise_error: bool) -> Option<String> {
        find_branch_from_known_env_vars()
            .or_else(find_branch_from_env_var_ending_with_branch)
            .or_else(|| branch_from_git_command(raise_error))
    }

    pub fn build_url() -> Option<String> {
        github_build_url().or_else(|| {
            BUILD_URL_ENV_VAR_NAMES
                .iter()
                .filter_map(|&name| value_from_env_var(name))
                .next()
        })
    }

    fn find_commit_from_env_vars() -> Option<String> {
        COMMIT_ENV_VAR_NAMES
            .iter()
            .filter_map(|&name| value_from_env_var(name))
            .next()
    }

    fn find_branch_from_known_env_vars() -> Option<String> {
        BRANCH_ENV_VAR_NAMES
            .iter()
            .filter_map(|&name| value_from_env_var(name))
            .next()
            .map(|val| val.trim_start_matches("refs/heads/").to_string())
    }

    fn find_branch_from_env_var_ending_with_branch() -> Option<String> {
        let values: Vec<String> = env::vars()
            .filter(|(k, _)| k.ends_with("_BRANCH"))
            .filter_map(|(_, v)| {
                let v = v.trim();
                if !v.is_empty() {
                    Some(v.to_string())
                } else {
                    None
                }
            })
            .collect();
        if values.len() == 1 {
            Some(values[0].clone())
        } else {
            None
        }
    }

    fn value_from_env_var(name: &str) -> Option<String> {
        env::var(name).ok().and_then(|v| {
            let v = v.trim();
            if !v.is_empty() {
                Some(v.to_string())
            } else {
                None
            }
        })
    }

    fn branch_from_git_command(raise_error: bool) -> Option<String> {
        let branch_names = execute_and_parse_command(raise_error);
        if raise_error {
            validate_branch_names(&branch_names);
        }
        if branch_names.len() == 1 {
            Some(branch_names[0].clone())
        } else {
            None
        }
    }

    fn commit_from_git_command(raise_error: bool) -> Option<String> {
        match execute_git_commit_command() {
            Ok(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            Err(e) => {
                if raise_error {
                    panic!(
                        "Could not determine current git commit using command `git rev-parse HEAD`. {}",
                        e
                    );
                }
                None
            }
        }
    }

    fn validate_branch_names(branch_names: &[String]) {
        if branch_names.is_empty() {
            panic!(
                "Command `git rev-parse --abbrev-ref HEAD` didn't return anything that could be identified as the current branch."
            );
        }
        if branch_names.len() > 1 {
            panic!(
                "Command `git rev-parse --abbrev-ref HEAD` returned multiple branches: {}. You will need to get the branch name another way.",
                branch_names.join(", ")
            );
        }
    }

    fn execute_git_command() -> Result<String, String> {
        Command::new("git")
            .args(&["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .map_err(|e| e.to_string())
            .and_then(|output| {
                if output.status.success() {
                    Ok(String::from_utf8_lossy(&output.stdout).to_string())
                } else {
                    Err(String::from_utf8_lossy(&output.stderr).to_string())
                }
            })
    }

    fn execute_git_commit_command() -> Result<String, String> {
        Command::new("git")
            .args(&["rev-parse", "HEAD"])
            .output()
            .map_err(|e| e.to_string())
            .and_then(|output| {
                if output.status.success() {
                    Ok(String::from_utf8_lossy(&output.stdout).to_string())
                } else {
                    Err(String::from_utf8_lossy(&output.stderr).to_string())
                }
            })
    }

    fn execute_and_parse_command(raise_error: bool) -> Vec<String> {
        match execute_git_command() {
            Ok(output) => output
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(|l| l.split_whitespace().next().unwrap_or("").to_string())
                .map(|l| l.trim_start_matches("origin/").to_string())
                .filter(|l| l != "HEAD")
                .collect(),
            Err(e) => {
                if raise_error {
                    panic!(
                        "Could not determine current git branch using command `git rev-parse --abbrev-ref HEAD`. {}",
                        e
                    );
                }
                vec![]
            }
        }
    }

    fn github_build_url() -> Option<String> {
        let parts: Vec<String> = ["GITHUB_SERVER_URL", "GITHUB_REPOSITORY", "GITHUB_RUN_ID"]
            .iter()
            .filter_map(|&name| value_from_env_var(name))
            .collect();
        if parts.len() == 3 {
            Some(format!(
                "{}/{}/actions/runs/{}",
                parts[0], parts[1], parts[2]
            ))
        } else {
            None
        }
    }
}
