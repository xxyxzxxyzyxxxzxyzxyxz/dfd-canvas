# Container Operations

Build, run, develop, and verify DFD Canvas inside Docker. The host needs Docker, Compose v2, and a browser. The supported environment is amd64 on Linux / WSL2; every Compose service specifies `platform: linux/amd64`. Other CPU architectures and emulation are outside the scope of functional and performance verification.

## Services and Artifacts

| Service | Purpose | Source and output |
| --- | --- | --- |
| `app` | Serve the release distribution with a Rust server | Serves `/app/dist` from the image. Does not mount host source files |
| `dev` | Rebuild and serve the application with Trunk | Mounts source files read-only at `/workspace`. Stores build artifacts in a dedicated volume at `/build` |
| `tools` | Run verification, generation, and measurement through `xtask` | Includes the source and release distribution from image build time. Mounts `test-results/` at `/results` |

```sh
docker compose up --build app
```

Open `http://127.0.0.1:8080`. Press `Ctrl+C` to stop, or run `docker compose stop app` in another terminal. To switch to development, stop `app`, then run `docker compose up --build dev`. Stop development with `docker compose stop dev` before returning to normal use. Both services use the same host port.

The host port is bound only to `127.0.0.1`. The `app` startup log shows the URL to open in a browser. To change the port, use a command such as `DFD_PORT=8081 docker compose up --build app`. The displayed URL will then be `http://127.0.0.1:8081`. Browser autosave is tied to the origin, so changing the port uses a separate storage area.

`app` runs as the non-root user `65532:65532` with a read-only filesystem. The runtime image contains the server and static distribution, without the Rust toolchain, tests, or Chromium. The server CLI accepts `--listen` and `--dir` to set the listening address and content directory. Their defaults inside the container are `0.0.0.0:8080` and `/app/dist`. `0.0.0.0` accepts connections on all IPv4 interfaces in the container. `--public-url` sets the URL shown in the startup log; Compose sets it to match the published host port. When `--public-url` is omitted, a listening address that covers all interfaces is displayed as a loopback address.

## Development, Verification, and File Ownership

By default, `dev` and `tools` run as a non-root user with UID / GID 1000. To match your regular host user, set the following before the first build:

```sh
export DFD_UID="$(id -u)" DFD_GID="$(id -g)"
mkdir -p test-results
docker compose run --build --rm tools check
docker compose run --build --rm tools browser-test
docker compose run --build --rm tools measure-dist
```

Create `test-results/` on the host first. Use `--output /results/name` to save reports in a subdirectory. `generate-fixture 100 200 /results/example.dfd.json` also writes to this mount. `measure-dist` writes JSON to standard output and measures the release distribution inside the image.

Because `tools` uses the source captured when the image was built, keep using `--build` after source changes. `check` rebuilds into `/build/dist`, while browser tests and size measurements use `/app/dist` in the same image. These paths are set by `DFD_BUILD_DIST` and `DFD_DIST`, respectively.

Cargo registry and Git downloads share named volumes. `dev` and `tools` have separate `/build` volumes and do not depend on the host's `target/` or `dist/`. Docker builds also cache dependencies and compilation results. A normal `docker compose down` preserves named volumes. Changing UID / GID later does not automatically update the ownership of existing caches; fix their ownership or recreate the development and verification cache volumes.

The normal development mount is read-only, and Cargo uses `--locked`. Only when maintaining dependencies, update `Cargo.lock` in a toolchain container with a writable source mount.

```sh
docker build --target toolchain \
  --build-arg DFD_UID="$DFD_UID" --build-arg DFD_GID="$DFD_GID" \
  --tag dfd-canvas-toolchain .
docker run --rm \
  --mount "type=bind,src=$PWD,dst=/workspace" \
  dfd-canvas-toolchain cargo update
```

To update a specific dependency, pass `-p` and `--precise` to `cargo update`. Review the diff, then rerun `tools check` and the browser tests. Do not use Rust or Cargo installed on the host.

## Pinned Tools and Updates

The Dockerfile has stages for the shared toolchain, development, application build, verification CLI build, browser dependencies, verification runtime, and serving. [Dockerfile](../Dockerfile), [Cargo.lock](../Cargo.lock), and [rust-toolchain.toml](../rust-toolchain.toml) are the authoritative sources for pinned values.

| Component | Pinning method |
| --- | --- |
| Rust 1.98.1 / Debian bookworm base | Version tag and SHA-256 image digest |
| Trunk 0.21.14 | Versioned download URL and SHA-256 verification |
| wasm-bindgen CLI 0.2.128 | Versioned download URL and SHA-256 verification. Must match the Cargo dependency |
| wasm-opt 123 | Versioned download URL and SHA-256 verification |
| Cargo dependencies | Committed Cargo.lock and `--locked` during builds |
| Chromium, Noto CJK, and OS dependencies | Debian / Debian security snapshot at `20260924T000000Z` |

Updates are manual. When updating the base image, update both the tag and digest. Check external tools against the checksums provided by their distributors. When updating Chromium or fonts, update the snapshot timestamp. Record the Chromium version printed during the build and the runtime version in the browser report again.

The verification container launches the pinned Chromium directly with `DFD_CHROMIUM=/usr/bin/chromium`. `DFD_CHROMIUM_NO_SANDBOX=1` disables Chromium's namespace sandbox in this dedicated test environment. The default mode is headless. Using the GUI with `--headed` requires a separately configured display-server connection.

Containers standardize tools and dependencies. They do not eliminate performance differences caused by host CPUs, GPUs, kernels, and other hardware or system differences. Keep historical Python / Playwright results separate from new Rust / Chromium reports and document their conditions. The pure Rust compression backend used by `measure-dist` also differs from the previous Python / zlib backend, so gzip sizes may not match exactly even with the same level 9 and mtime 0 settings.
