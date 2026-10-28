/// The self-verification inputs shared by `publish-provider-contract` and
/// `publish-provider-contracts`, reduced to whether each one was supplied.
pub struct SelfVerificationInputs {
    pub results: bool,
    pub results_content_type: bool,
    pub verifier: bool,
    pub verifier_version: bool,
}

/// A `selfVerificationResults` block is sent as soon as any evidence is given, and PactFlow rejects
/// it unless it carries the results, their content type and the verifier. Checking here turns that
/// server 400 into an error that names the missing key before anything is published.
///
/// Returns the names of the missing keys, in the order they should be reported. Empty when no
/// self-verification was supplied at all, or when it is complete.
pub fn missing_self_verification_keys(inputs: &SelfVerificationInputs) -> Vec<&'static str> {
    if !(inputs.results || inputs.verifier || inputs.verifier_version) {
        return Vec::new();
    }
    [
        (inputs.results, "verification-results"),
        (
            inputs.results_content_type,
            "verification-results-content-type",
        ),
        (inputs.verifier, "verifier"),
    ]
    .into_iter()
    .filter_map(|(present, key)| (!present).then_some(key))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(
        results: bool,
        content_type: bool,
        verifier: bool,
        version: bool,
    ) -> SelfVerificationInputs {
        SelfVerificationInputs {
            results,
            results_content_type: content_type,
            verifier,
            verifier_version: version,
        }
    }

    #[test]
    fn nothing_supplied_needs_nothing() {
        assert!(missing_self_verification_keys(&inputs(false, false, false, false)).is_empty());
    }

    #[test]
    fn a_complete_set_is_accepted() {
        assert!(missing_self_verification_keys(&inputs(true, true, true, false)).is_empty());
        assert!(missing_self_verification_keys(&inputs(true, true, true, true)).is_empty());
    }

    #[test]
    fn a_verifier_version_alone_is_missing_everything_else() {
        assert_eq!(
            missing_self_verification_keys(&inputs(false, false, false, true)),
            vec![
                "verification-results",
                "verification-results-content-type",
                "verifier"
            ]
        );
    }

    #[test]
    fn a_verifier_alone_is_missing_the_results_and_their_content_type() {
        assert_eq!(
            missing_self_verification_keys(&inputs(false, false, true, false)),
            vec!["verification-results", "verification-results-content-type"]
        );
    }

    #[test]
    fn results_without_a_verifier_names_only_the_verifier() {
        assert_eq!(
            missing_self_verification_keys(&inputs(true, true, false, false)),
            vec!["verifier"]
        );
    }
}
