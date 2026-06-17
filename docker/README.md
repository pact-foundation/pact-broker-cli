# Container Images

Images are published to both Docker Hub and GHCR on every release:

- `pactfoundation/pact-broker-cli:<version>`
- `pactfoundation/pact-broker-cli:<version>-alpine`
- `pactfoundation/pact-broker-cli:<version>-debian`
- `ghcr.io/pact-foundation/pact-broker-cli:<version>` (and variants)

The unsuffixed images default to the Alpine variants as they are smaller and have a typically smaller attack surface.

## Running

```shell
docker run --rm \
  -e PACT_BROKER_BASE_URL=https://broker.example.com \
  -e PACT_BROKER_TOKEN=your-token \
  pactfoundation/pact-broker-cli:latest \
  list-pacts --consumer my-consumer
```

## Building locally

Use the `build` script. It requires `CONTAINER_TAG` to be set and
handles multi-platform builds and registry pushes via environment variables:

| Variable           | Default                   | Description                                          |
| ------------------ | ------------------------- | ---------------------------------------------------- |
| `CONTAINER_TAG`    | _(required)_              | Version to build, without the `v` prefix             |
| `PUSH_IMAGE`       | `false`                   | Set to `true` to push to registries after build      |
| `TAG_LATEST`       | `false`                   | Set to `true` to also tag the base image as `latest` |
| `PLATFORMS`        | `linux/amd64,linux/arm64` | Target platforms for all builds                      |
| `PLATFORMS_ALPINE` | inherits `PLATFORMS`      | Override platforms for Alpine builds only            |
| `PLATFORMS_DEBIAN` | inherits `PLATFORMS`      | Override platforms for Debian builds only            |

Build a single architecture locally for testing:

```shell
CONTAINER_TAG=0.8.1 PLATFORMS=linux/arm64 ./build
```

Build and push all variants to both registries:

```shell
CONTAINER_TAG=0.8.1 PUSH_IMAGE=true TAG_LATEST=true ./build
```

## Developer notes

### Build platform ARGs

These are automatically injected by `docker buildx` and available in Containerfiles after being declared with `ARG`:

| Variable         | Description                                       | Example       |
| ---------------- | ------------------------------------------------- | ------------- |
| `BUILDPLATFORM`  | Platform of the build host                        | `linux/amd64` |
| `BUILDOS`        | OS component of `BUILDPLATFORM`                   | `linux`       |
| `BUILDARCH`      | Architecture of `BUILDPLATFORM`                   | `amd64`       |
| `BUILDVARIANT`   | Variant of `BUILDPLATFORM` (ARM only)             | `v7`          |
| `TARGETPLATFORM` | Platform being built for (from `--platform` flag) | `linux/arm64` |
| `TARGETOS`       | OS component of `TARGETPLATFORM`                  | `linux`       |
| `TARGETARCH`     | Architecture of `TARGETPLATFORM`                  | `arm64`       |
| `TARGETVARIANT`  | Variant of `TARGETPLATFORM` (ARM only)            | `v7`          |

### Platform support

Currently supported: `linux/amd64`, `linux/arm64`.

More can be added as guided by demand and feasibility. If you have a specific platform in mind, please raise an issue to discuss it.
