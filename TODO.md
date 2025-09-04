# Todo list

## Pact Tests

- [X] publish_pacts
- [X] update_environment
- [ ] delete_environment
- [ ] describe_environment
- [ ] record_support_ended
- [ ] can_i_deploy
- [ ] can_i_merge
- [ ] create_or_update_pacticipant
- [ ] describe_pacticipant
- [ ] list_pacticipants
- [X] create_webhook
- [X] create_or_update_webhook
- [ ] test_webhook
- [X] create_version_tag
- [ ] describe_version
- [ ] create_or_update_version
- [ ] generate_uuid
- [X] list_latest_pact_versions
- [X] create_environment
- [X] list_environments
- [X] record_deployment
- [X] record_undeployment
- [X] record_release
- [X] create_webhook_with_team_uuid (pactflow)
- [X] delete_branch

extra_goodies_spec.rb
list_latest_pact_versions_spec.rb.bak
pact_broker_client_matrix_ignore_spec.rb
pact_broker_client_matrix_spec.rb
pact_broker_client_pacticipant_version_spec.rb
pact_broker_client_publish_spec.rb
pact_broker_client_register_repository_spec.rb
pact_broker_client_retrieve_all_pacts_for_provider_spec.rb
pact_broker_client_retrieve_pact_spec.rb
pact_broker_client_versions_spec.rb
pact_helper.rb
pactflow_publish_provider_contract_spec.rb
pactflow_publish_provider_contract_the_old_way_spec.rb
pactflow_webhooks_create_spec.rb

## General

1. Add tests for all commands
2. Verify each pact against pact broker (branch pact_broker_cli)