require 'open3'

command = 'target/release/pact-broker-cli'
arguments = ARGV.empty? ? [] : ARGV
puts "Running command: #{command} #{arguments.join(' ')}"
stdout, stderr, status = Open3.capture3(command, *arguments)

puts stdout
puts stderr
puts "Exit status: #{status.exitstatus}"
exit status.exitstatus
