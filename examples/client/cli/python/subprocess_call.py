# create a file to load the pact-broker-cli library and pass user arguments to it
#  ensure the exit code is preserved

# target/release/pact-broker-cli
# A pact cli tool

# Usage: pact-broker-cli [COMMAND]

# Commands:
#   pact-broker  
#   pactflow     
#   completions  Generates completion scripts for your shell
#   docker       Run the Pact Broker as a Docker container
#   examples     download example projects
#   project      Pact project actions for setting up and managing pact projects
#   standalone   Install & Run the Pact Broker in $HOME/traveling-broker
#   plugin       CLI utility for Pact plugins
#   mock         Standalone Pact mock server
#   stub         Pact Stub Server 0.0.9
#   verifier     
#   help         Print this message or the help of the given subcommand(s)

# Options:
#   -h, --help  Print help

import subprocess
import sys

# Command to execute
command = ['target/release/pact-broker-cli']

# Take user input
# Get additional arguments from command-line arguments
user_input = ' '.join(sys.argv[1:])

# Append user input to the command
command.extend(user_input.split())

# Execute the command and capture the exit code
exit_code = subprocess.call(command)

# Print the exit code
print(f"Exit code: {exit_code}")
# Exit with the exit code
exit(exit_code)