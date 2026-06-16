SHELL := /bin/bash

PACTICIPANT="pact-broker-cli"
GITHUB_WEBHOOK_UUID?="c1821a83-cf7e-4e4e-9015-bda1ddb00f2b"
PACT_CLI="docker run --rm -v ${PWD}:${PWD} -e PACT_BROKER_BASE_URL -e PACT_BROKER_TOKEN pactfoundation/pact-cli:latest"

## =====================
## PactFlow set up tasks
## =====================

# This should be called once before creating the webhook
# with the environment variable GITHUB_TOKEN set
create_github_token_secret:
	@curl -v -X POST ${PACT_BROKER_BASE_URL}/secrets \
	-H "Authorization: Bearer ${PACT_BROKER_TOKEN}" \
	-H "Content-Type: application/json" \
	-H "Accept: application/hal+json" \
	-d  "{\"name\":\"githubCommitStatusToken\",\"description\":\"Github token for updating commit statuses\",\"value\":\"${GITHUB_TOKEN}\"}"

# In order to setup the webhook, the pacticipant needs to be created. It is auto-created on publish
# but this is useful for setting up the webhook before publishing any pacts.
create_pacticipant:
	@"${PACT_CLI}" \
	  broker create-or-update-pacticipant \
	  --name ${PACTICIPANT}

# This webhook will update the Github commit status for this commit
# so that any PRs will get a status that shows what the status of
# the pact is.
create_or_update_github_webhook:
	@"${PACT_CLI}" \
	  broker create-or-update-webhook \
	  'https://api.github.com/repos/pact-foundation/pact-broker-cli/statuses/$${pactbroker.consumerVersionNumber}' \
	  --header 'Content-Type: application/json' 'Accept: application/vnd.github.v3+json' 'Authorization: token $${user.githubCommitStatusToken}' \
	  --request POST \
	  --data @${PWD}/github-commit-status-webhook.json \
	  --uuid ${GITHUB_WEBHOOK_UUID} \
	  --consumer ${PACTICIPANT} \
	  --contract-published \
	  --provider-verification-published \
	  --description "Github commit status webhook for ${PACTICIPANT}"

test_github_webhook:
	@curl -v -X POST ${PACT_BROKER_BASE_URL}/webhooks/${GITHUB_WEBHOOK_UUID}/execute -H "Authorization: Bearer ${PACT_BROKER_TOKEN}"

## ========================
## README maintenance tasks
## ========================

# Refresh all ```console help blocks in README.md.
# Uses TRYCMD=overwrite so the README exactly matches what the snapshot tests expect.
update_readme_help:
	TRYCMD=overwrite cargo test cli_tests

# Check that README.md is up to date with the current binary's --help output.
# Runs the snapshot tests; exits non-zero if any block would change. Useful in CI.
check_readme_help:
	cargo test cli_tests
