#[test]
#[cfg(not(windows))]
fn cli_tests() {
    // Store original environment variable values
    let orig_ssl_cert_file = std::env::var("SSL_CERT_FILE").ok();
    let orig_pact_broker_base_url = std::env::var("PACT_BROKER_BASE_URL").ok();
    let orig_pact_broker_token = std::env::var("PACT_BROKER_TOKEN").ok();

    // Remove the environment variables
    unsafe { std::env::remove_var("SSL_CERT_FILE") };
    unsafe { std::env::remove_var("PACT_BROKER_BASE_URL") };
    unsafe { std::env::remove_var("PACT_BROKER_TOKEN") };

    // Run the test cases
    trycmd::TestCases::new()
        .case("tests/cmd/*.toml")
        .case("README.md");

    // Restore the environment variables
    if let Some(val) = orig_ssl_cert_file {
        unsafe { std::env::set_var("SSL_CERT_FILE", val) };
    }
    if let Some(val) = orig_pact_broker_base_url {
        unsafe { std::env::set_var("PACT_BROKER_BASE_URL", val) };
    }
    if let Some(val) = orig_pact_broker_token {
        unsafe { std::env::set_var("PACT_BROKER_TOKEN", val) };
    }
}
