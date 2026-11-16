use serde_json::Value;

/// Index relation a PactFlow server advertises when it keeps several provider
/// contracts per provider version.
const PROVIDER_CONTRACTS_RELATION: &str = "pf:provider-contracts";

/// Error for a command that needs several contracts per provider version, run
/// against a server that keeps one. `needs` names what cannot be honoured.
pub(crate) fn unsupported_server_message(needs: &str) -> String {
    format!(
        "This PactFlow server keeps one provider contract per provider version, so {needs} cannot be honoured. Upgrade PactFlow, or publish one document per provider version without a name."
    )
}

/// Warning for an unnamed publish of a specification other than `oas` to a
/// server that keeps one contract per provider version.
pub(crate) fn old_server_publish_warning(
    specification: &str,
    name: Option<&str>,
    index: &Value,
) -> Option<&'static str> {
    (specification != "oas" && name.is_none() && !supports_several_contracts(index)).then_some(
        "This PactFlow server keeps one provider contract per provider version. Publishing replaces any contract of another type at this version.",
    )
}

/// Whether the server's index advertises `pf:provider-contracts`.
pub(crate) fn supports_several_contracts(index: &Value) -> bool {
    index
        .get("_links")
        .and_then(|links| links.get(PROVIDER_CONTRACTS_RELATION))
        .is_some()
}

/// The `href` of the `pf:provider-contracts` relation, a URI template left unexpanded.
pub(crate) fn provider_contracts_template(index: &Value) -> Option<String> {
    index
        .get("_links")?
        .get(PROVIDER_CONTRACTS_RELATION)?
        .get("href")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn supported_when_the_relation_is_present() {
        let index = json!({"_links": {"pf:provider-contracts": {"href": "http://x/{provider}", "templated": true}}});
        assert!(supports_several_contracts(&index));
    }

    #[test]
    fn unsupported_when_the_relation_is_absent() {
        let index = json!({"_links": {"pf:publish-provider-contract": {"href": "http://x"}}});
        assert!(!supports_several_contracts(&index));
    }

    #[test]
    fn unsupported_when_the_index_has_no_links() {
        assert!(!supports_several_contracts(&json!({})));
        assert!(!supports_several_contracts(&json!({"_links": null})));
    }

    #[test]
    fn warns_for_an_unnamed_non_oas_publish_to_an_old_server() {
        let old = json!({"_links": {}});
        let new = json!({"_links": {"pf:provider-contracts": {"href": "x"}}});
        assert!(old_server_publish_warning("asyncapi", None, &old).is_some());
        assert_eq!(old_server_publish_warning("oas", None, &old), None);
        assert_eq!(
            old_server_publish_warning("asyncapi", Some("a"), &old),
            None
        );
        assert_eq!(old_server_publish_warning("asyncapi", None, &new), None);
    }

    #[test]
    fn template_is_the_unexpanded_href() {
        let index = json!({"_links": {"pf:provider-contracts": {"href": "http://x/provider/{provider}/version/{version}", "templated": true}}});
        assert_eq!(
            provider_contracts_template(&index).as_deref(),
            Some("http://x/provider/{provider}/version/{version}")
        );
    }

    #[test]
    fn template_is_none_without_the_relation_or_href() {
        assert_eq!(provider_contracts_template(&json!({})), None);
        assert_eq!(
            provider_contracts_template(&json!({"_links": {"pf:provider-contracts": {}}})),
            None
        );
    }
}
