#!/bin/sh
set -e

BIN=${BIN:-pact-broker-cli}
${BIN} --help
${BIN} pactflow --help
${BIN} completions --help
${BIN} list-latest-pact-versions
${BIN} create-environment --name name_foo1
${BIN} create-environment --name name_foo2 --display-name display_name_foo
${BIN} create-environment --name name_foo3 --display-name display_name_foo --contact-name contact_name_foo
${BIN} create-environment --name name_foo4 --display-name display_name_foo --contact-name contact_name_foo --contact-email-address contact.email.address@foo.bar
export ENV_UUID=$(${BIN} create-environment --name name_foo5 --output=id)
${BIN} describe-environment --uuid $ENV_UUID
${BIN} update-environment --uuid $ENV_UUID --name name_foo6
${BIN} update-environment --uuid $ENV_UUID --name name_foo7 --display-name display_name_foo6
${BIN} update-environment --uuid $ENV_UUID --name name_foo8 --contact-name contact_name_foo8
${BIN} update-environment --uuid $ENV_UUID --name name_foo9 --contact-name contact_name_foo9 --contact-email-address contact_name_foo7
${BIN} delete-environment --uuid $ENV_UUID
${BIN} list-environments | awk -F '│' '{print $2}' | sed -n '3,$p' | sed '$d' | awk '{print $1}' | xargs -I {} ${BIN} delete-environment --uuid {} 
${BIN} create-environment --name production --production
${BIN} publish pacts -r
${BIN} publish pacts -a foo --branch bar
${BIN} can-i-deploy --pacticipant GettingStartedOrderWeb --version foo --to prod || echo "can-i-deploy fails due to no verification result - expected"
${BIN} can-i-deploy --pacticipant GettingStartedOrderWeb --version foo --to prod --dry-run
${BIN} record-deployment --version foo --environment production --pacticipant GettingStartedOrderWeb
${BIN} record-undeployment --environment production --pacticipant GettingStartedOrderWeb
${BIN} record-release --version foo --environment production --pacticipant GettingStartedOrderWeb
${BIN} record-support-ended --version foo --environment production --pacticipant GettingStartedOrderWeb
${BIN} create-or-update-pacticipant --name foo --main-branch main --repository-url http://foo.bar
${BIN} describe-pacticipant --name foo
${BIN} list-pacticipants
${BIN} create-webhook https://localhost --request POST --contract-published
export WEBHOOK_UUID=$(${BIN} create-webhook https://localhost --request POST --contract-published | jq .uuid -r)
${BIN} create-or-update-webhook https://foo.bar --request POST --uuid $WEBHOOK_UUID --provider-verification-succeeded
${BIN} test-webhook --uuid $WEBHOOK_UUID
${BIN} create-or-update-version --version foo --pacticipant foo --branch bar --tag baz
${BIN} create-version-tag --version foo --pacticipant foo --tag bar
${BIN} describe-version --pacticipant foo
${BIN} can-i-merge --pacticipant foo --version foo
${BIN} delete-branch --branch bar --pacticipant foo
${BIN} describe-pacticipant --name foo
${BIN} generate-uuid

