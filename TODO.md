# Todo list

## Pact Tests

- [X] publish_pacts
- [X] update_environment
- [X] delete_environment
- [X] describe_environment
- [X] record_support_ended
- [ ] can_i_deploy
- [ ] can_i_merge
- [X] create_or_update_pacticipant
- [X] describe_pacticipant
- [X] list_pacticipants
- [X] create_webhook
- [X] create_or_update_webhook
- [X] test_webhook
- [X] create_version_tag
- [X] describe_version
- [X] create_or_update_version
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



issues with query param ordering

            "can-i-deploy",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            "Foo",
            "--version",
            "1.2.3",
            "--pacticipant",
            "Bar",
            "--latest",
            "--tag",



Sent as 

http://127.0.0.1:60803/matrix?q[][pacticipant]=Foo&q[][version]=1.2.3&q[][pacticipant]=Bar&q[][latest]=true&q[][tag]=prod&latestby=cvpv

saved as 

latestby=cvpv&q[][latest]=true&q[][pacticipant]=Foo&q[][tag]=prod&q[][version]=1%2e2%2e3&q[][pacticipant]=Bar
