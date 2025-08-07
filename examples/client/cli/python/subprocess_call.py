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