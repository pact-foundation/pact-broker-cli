
build a single arch image

```sh
docker buildx build -t you54f/pact-broker-cli:$DOCKER_TAG-alpine --build-arg VERSION=$DOCKER_TAG --platform linux/arm . -f Dockerfile.alpine --load
```

Run the image

```sh
docker run --platform=linux/arm -p 8080:8080 --rm --init you54f/pact-broker-cli:0.0.9-alpine mock start
```

Docker multi arch available args

```console
BUILDPLATFORM — matches the current machine. (e.g. linux/amd64)

BUILDOS — os component of BUILDPLATFORM, e.g. linux

BUILDARCH — e.g. amd64, arm64, riscv64

BUILDVARIANT — used to set ARM variant, e.g. v7

TARGETPLATFORM — The value set with --platform flag on build

TARGETOS - OS component from --platform, e.g. linux

TARGETARCH - Architecture from --platform, e.g. arm64

TARGETVARIANT - Variant from the --platform e.g. v7
```

## Docker targets

### Alpine

<https://hub.docker.com/_/alpine/tags>

#### Alpine to build

- linux/ppc64le
- linux/s390x

### Debian

#### Debian to build

- linux/mips64le
- linux/ppc64le
- linux/riscv64
- linux/s390x

## Rust Platforms

### rust platform support

- <https://doc.rust-lang.org/rustc/platform-support.html>

### Rust Targets to build

- i686-unknown-linux-musl
- mips64-unknown-linux-gnuabi64
- mips64-unknown-linux-muslabi64
- mips64el-unknown-linux-gnuabi64
- mips64el-unknown-linux-muslabi64
- riscv64gc-unknown-linux-musl
- riscv64gc-unknown-linux-gnu
- riscv64gc-unknown-freebsd
- riscv64gc-unknown-netbsd
- s390x-unknown-linux-musl
- s390x-unknown-linux-gnu

### cross supported targets

- <https://github.com/cross-rs/cross/blob/main/targets.toml>

- s390x-unknown-linux-gnu
- riscv64gc-unknown-linux-gnu
- mips64-unknown-linux-gnuabi64
- mips64el-unknown-linux-gnuabi64
- mips64-unknown-linux-muslabi64
- mips64el-unknown-linux-muslabi64

musl builds currently broken <https://github.com/cross-rs/cross/issues/1422>
