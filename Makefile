SHELL := /bin/bash



TARGET=
USE_CROSS=
BINARY_NAME?=pact-broker-cli
SLIM=false
BUILDER=cargo

ifeq ($(TARGET),)
	TARGET := $(shell rustup show | grep 'Default host' | awk '{print $$3}')
endif

ifeq ($(USE_CROSS),true)
	BUILDER := cross
endif


# Shows a list of available targets for cross-compilation
target_list = $(shell rustup target list)
rustup_target_list:
	@echo "$(target_list)" | sed 's/([^)]*)//g' | tr ' ' '\n' | sed '/^\s*$$/d'

is_slim:
	echo $(SLIM)

use_cross:
	echo $(BUILDER)

cargo_test:
	$(BUILDER) test --target=$(TARGET) --verbose -- --nocapture
# Build the release version of the library
# Can be used to build for a specific target by setting the TARGET environment variable
# e.g. `make cargo_build_release TARGET=x86_64-unknown-linux-gnu`
# by default will use the host target
cargo_build_release:
	echo "Building for target: $(TARGET)"
	if [[ $(SLIM) == "true" ]]; then \
		if [[ "$(shell uname -s)" == "Linux" ]]; then \
			sudo apt install libstd-rust-dev; \
			rustup toolchain install nightly; \
			rustup component add rust-src --toolchain nightly; \
		else \
			rustup component add rust-src --toolchain nightly --target=$(TARGET); \
		fi; \
		if [[ $(BUILDER) == "cross" ]]; then \
			cargo +nightly install cross@0.2.5; \
		fi; \
	fi
	if [[ $(TARGET) == "aarch64-unknown-freebsd" ]]; then \
		if [[ "$(shell uname -s)" == "Linux" ]]; then \
			sudo apt install libstd-rust-dev; \
		fi; \
		cargo +nightly install cross --git https://github.com/cross-rs/cross; \
	elif [[ $(TARGET) == *"android"* ]] || [[ $(TARGET) == "x86_64-unknown-netbsd" ]] || [[ $(TARGET) == "x86_64-pc-windows-gnu" ]] || [[ $(TARGET) == "x86_64-unknown-freebsd" ]]; then \
		echo "installing latest cross"; \
		if [[ $(SLIM) == "true" ]]; then \
			cargo +nightly install cross --git https://github.com/cross-rs/cross; \
		else \
			cargo install cross --git https://github.com/cross-rs/cross; \
		fi; \
	else \
		if [[ $(BUILDER) == "cross" ]]; then \
			cargo install cross@0.2.5; \
		fi; \
	fi
	if [[ $(SLIM) == "true" ]]; then \
		echo "building slimmest binaries"; \
		if [[ $(TARGET) == "aarch64-unknown-freebsd" ]]; then \
			echo "building with cargo nightly, plus std and core for aarch64-unknown-freebsd"; \
			RUSTFLAGS="-Zlocation-detail=none" $(BUILDER) +nightly build -Z build-std=std,panic_abort,core,alloc,proc_macro --profile release-aarch64-freebsd --target=$(TARGET); \
			mkdir -p target/aarch64-unknown-freebsd/release; \
			mv target/aarch64-unknown-freebsd/release-aarch64-freebsd/$(BINARY_NAME) target/aarch64-unknown-freebsd/release; \
		else \
			if [[ $(TARGET) == *"risc"* ]] && [[ $(TARGET) != *"musl"* ]]; then \
				echo "building for risc targets, refusing to build with nightly as unable to build-std"; \
				rustup toolchain install $(TARGET); \
				rustup component add rust-src --toolchain stable --target $(TARGET); \
				cargo install cross@0.2.5; \
				$(BUILDER) build --target=$(TARGET) --release; \
			elif [[ $(TARGET) == *"risc"* ]] && [[ $(TARGET) == *"musl"* ]]; then \
				echo "building for risc targets, build with nightly for build-std"; \
				RUSTFLAGS="-Zlocation-detail=none" $(BUILDER) +nightly build -Z build-std=std,panic_abort,core,alloc,proc_macro -Z build-std-features=panic_immediate_abort --target=$(TARGET) --bin $(BINARY_NAME) --release; \
			elif [[ $(TARGET) == *"s390x"* ]] && [[ $(TARGET) == *"musl"* ]]; then \
				echo "building for s390x musl targets, build with nightly for build-std"; \
				RUSTFLAGS="-Zlocation-detail=none" $(BUILDER) +nightly build -Z build-std=std,panic_abort,core,alloc,proc_macro -Z build-std-features=panic_immediate_abort --target=$(TARGET) --bin $(BINARY_NAME) --release; \
			elif [[ $(TARGET) == *"mips"* ]]; then \
				echo "building for mips targets, refusing to build with nightly as unable to build-std"; \
				rustup toolchain install $(TARGET); \
				rustup component add rust-src --toolchain stable --target $(TARGET); \
				cargo install cross --git https://github.com/cross-rs/cross; \
				$(BUILDER) build --target=$(TARGET) --release; \
			elif [[ $(TARGET) == "aarch64-unknown-linux-musl" ]] || [[ $(TARGET) == "armv5te-unknown-linux-musleabi" ]]; then \
				RUSTFLAGS="-Zlocation-detail=none -C link-arg=-lgcc" $(BUILDER) +nightly build -Z build-std=std,panic_abort,core,alloc,proc_macro -Z build-std-features=panic_immediate_abort --target=$(TARGET) --bin $(BINARY_NAME) --release; \
			elif [[ $(TARGET) == *"musl"* ]]; then \
				RUSTFLAGS="-Zlocation-detail=none" $(BUILDER) +nightly build -Z build-std=std,panic_abort,core,alloc,proc_macro -Z build-std-features=panic_immediate_abort --target=$(TARGET) --bin $(BINARY_NAME) --release; \
			else \
				RUSTFLAGS="-Zlocation-detail=none" $(BUILDER) +nightly build -Z build-std=std,panic_abort,core,alloc,proc_macro -Z build-std-features=panic_immediate_abort --target=$(TARGET) --release; \
			fi; \
		fi \
	elif [[ $(TARGET) == "aarch64-unknown-freebsd" ]]; then \
		echo "building with cargo nightly, plus std and core for aarch64-unknown-freebsd"; \
		$(BUILDER) +nightly build -Z build-std=std,core,alloc,proc_macro --profile release-aarch64-freebsd --target=$(TARGET); \
		mkdir -p target/aarch64-unknown-freebsd/release; \
		mv target/aarch64-unknown-freebsd/release-aarch64-freebsd/$(BINARY_NAME) target/aarch64-unknown-freebsd/release; \
	elif [[ $(TARGET) == *"musl"* ]]; then \
		$(BUILDER) build --release --target=$(TARGET) --bin $(BINARY_NAME); \
	else \
		$(BUILDER) build --release --target=$(TARGET); \
	fi