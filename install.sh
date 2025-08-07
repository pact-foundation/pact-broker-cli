#!/bin/sh -e
## Tested with https://www.shellcheck.net/
# Usage: (install latest)
#   $ curl -fsSL https://raw.githubusercontent.com/you54f/pact-broker-cli/master/install.sh | sh
# or
#   $ wget -q https://raw.githubusercontent.com/you54f/pact-broker-cli/master/install.sh -O- | sh
#
# Usage: (install fixed version) - pass PACT_BROKER_CLI_VERSION=v<PACT_BROKER_CLI_VERSION> eg PACT_BROKER_CLI_VERSION=v1.92.0 or set as an env var
#   $ curl -fsSL https://raw.githubusercontent.com/you54f/pact-broker-cli/master/install.sh | PACT_BROKER_CLI_VERSION=v1.92.0 sh
# or
#   $ wget -q https://raw.githubusercontent.com/you54f/pact-broker-cli/master/install.sh -O- | PACT_BROKER_CLI_VERSION=v1.92.0 sh
#
if [ "$tag" ]; then
  echo "setting $tag as PACT_BROKER_CLI_VERSION for legacy reasons"
  PACT_BROKER_CLI_VERSION="$tag"
fi

if command -v curl >/dev/null 2>&1; then
  downloader="curl -sLO"
elif command -v wget >/dev/null 2>&1; then
  downloader="wget -q"
else
  echo "Sorry, you need either curl or wget installed to proceed with the installation."
  exit 1
fi

if [ -z "$PACT_BROKER_CLI_VERSION" ]; then
  if command -v curl >/dev/null 2>&1; then
    PACT_BROKER_CLI_VERSION=$(basename "$(curl -fs -o/dev/null -w "%{redirect_url}" https://github.com/you54f/pact-broker-cli/releases/latest)")
  elif command -v wget >/dev/null 2>&1; then
    PACT_BROKER_CLI_VERSION=$(basename "$(wget -q -S -O /dev/null https://github.com/you54f/pact-broker-cli/releases/latest 2>&1 | grep -i "Location:" | awk '{print $2}')")
  else
    echo "Sorry, you need set a version number PACT_BROKER_CLI_VERSION as we can't determine the latest at this time. See https://github.com/you54f/pact-broker-cli/releases/latest."
    exit 1
  fi

  echo "Thanks for downloading the latest release of pact-broker-cli $PACT_BROKER_CLI_VERSION."
  echo "-----"
  echo "Note:"
  echo "-----"
  echo "You can download a fixed version by setting the PACT_BROKER_CLI_VERSION environment variable eg PACT_BROKER_CLI_VERSION=v1.92.0"
  echo "example:"
  echo "curl -fsSL https://raw.githubusercontent.com/you54f/pact-broker-cli/master/install.sh | PACT_BROKER_CLI_VERSION=v1.92.0 sh"
else
  echo "Thanks for downloading pact-broker-cli $PACT_BROKER_CLI_VERSION."
fi

PACT_BROKER_CLI_VERSION_WITHOUT_V=${PACT_BROKER_CLI_VERSION#v}
MAJOR_PACT_BROKER_CLI_VERSION=$(echo "$PACT_BROKER_CLI_VERSION_WITHOUT_V" | cut -d '.' -f 1)

case $(uname -sm) in
'Linux x86_64')
    if ldd /bin/ls >/dev/null 2>&1; then
        ldd_output=$(ldd /bin/ls)
        case "$ldd_output" in
            *musl*) 
                os='x86_64-linux-musl'
                ;;
            *) 
                os='x86_64-linux-gnu'
                ;;
        esac
    else
      os='x86_64-linux-gnu'
    fi
  ;;
'Linux aarch64')
  if ldd /bin/ls >/dev/null 2>&1; then
      ldd_output=$(ldd /bin/ls)
      case "$ldd_output" in
          *musl*) 
              os='aarch64-linux-musl'
              ;;
          *) 
              os='aarch64-linux-gnu'
              ;;
      esac
  else
    os='aarch64-linux-gnu'
  fi
  ;;
'Darwin arm64')
  os='aarch64-macos'
  ;;
'Darwin x86' | 'Darwin x86_64')
  os='x86_64-macos'
  ;;
"Windows"* | "MINGW64"*)
  if [ "$(uname -m)" = "aarch64" ]; then
    os='aarch64-windows-msvc'
  else
    os='x86_64-windows-msvc'
  fi
  ;;
*)
  echo "Sorry, you'll need to install the pact-broker-cli manually."
  exit 1
  ;;
esac

case $os in
*'windows'*)
  filename="pact-${PACT_BROKER_CLI_VERSION#v}-${os}.exe"
  ;;
*'macos'* | *'linux'*)
  filename="pact-${PACT_BROKER_CLI_VERSION#v}-${os}"
  ;;
esac

echo 
echo "-------------"
echo "Downloading:"
echo "-------------"
($downloader https://github.com/you54f/pact-broker-cli/releases/download/"${PACT_BROKER_CLI_VERSION}"/"${filename}" && echo downloaded "${filename}") || (echo "Sorry, you'll need to install the pact-broker-cli manually." && exit 1)
(chmod +x "${filename}" && echo unarchived "${filename}") || (echo "Sorry, you'll need to unarchived the pact-broker-cli manually." && exit 1)
echo "pact-broker-cli ${PACT_BROKER_CLI_VERSION} installed to $(pwd)/pact"
echo "-------------------"
echo "available commands:"
echo "-------------------"
PROJECT_NAME=pact-broker-cli
PACT_BROKER_CLI_BIN_PATH=${PWD}
mv $filename $PROJECT_NAME

if [ "$GITHUB_ENV" ]; then
echo "Added the following to your path to make ${PROJECT_NAME} available:"
echo ""
echo "PATH=$PACT_BROKER_CLI_BIN_PATH:\${PATH}"
echo "PATH=$PACT_BROKER_CLI_BIN_PATH:${PATH}" >>"$GITHUB_ENV"
elif [ "$CIRRUS_CI" ]; then
echo "Added the following to your path to make ${PROJECT_NAME} available:"
echo ""
echo "PATH=$PACT_BROKER_CLI_BIN_PATH:\${PATH}"
echo "PATH=$PACT_BROKER_CLI_BIN_PATH:${PATH}" >>"$CIRRUS_ENV"
else
echo "Add the following to your path to make ${PROJECT_NAME} available:"
echo "--- Linux/MacOS/Windows Bash Users --------"
echo ""
echo "  PATH=$PACT_BROKER_CLI_BIN_PATH:\${PATH}"
fi
