# Browser Verification

The Rust `xtask` controls Chromium directly through chromiumoxide 0.9.1. It verifies behavior through node editing, Pointer Events, file inputs, and actual downloads. JavaScript is injected into the browser to observe the DOM, measure rendering, and reproduce storage failures. Production code has no test-only APIs, and the distribution does not include measurement code.

## Running the Tests

The host needs Docker and Compose v2. The supported environment is amd64 on Linux / WSL2. You do not need to install Python, Playwright, Node.js, Rust, or Chromium on the host. Run these commands from the repository root:

```sh
export DFD_UID="$(id -u)" DFD_GID="$(id -g)"
mkdir -p test-results
docker compose run --build --rm tools browser-test
```

UID / GID default to 1000. Creating `test-results/` in advance allows reports to be saved with the host user's ownership. This directory maps to `/results` inside the container. The first image build installs the build tools, pinned Chromium, shared libraries, and Japanese fonts.

When no URL is supplied, the runner starts the Rust server on an available port at `127.0.0.1` inside the test container and serves the release distribution from the image. It stops the server after the tests finish. Neither the normal `app` service nor the development `dev` service is required.

```sh
docker compose run --build --rm tools browser-test --only acceptance
docker compose run --build --rm tools browser-test --only performance
docker compose run --build --rm tools browser-test --only startup
docker compose run --build --rm tools browser-test --only all --output /results/recheck
```

`--only acceptance` runs acceptance, group operations, edge reconnection, and error-handling tests. `--only performance` measures interaction performance at two document sizes, each with and without groups. `--only startup` measures initial rendering only. The default is `all`. The default value of `--output` is `/results`.

Use `--url` to test an existing server. Specify a URL reachable from the container. The container's `127.0.0.1` belongs to a different network namespace from the host's.

The standard configuration uses headless Chromium. `--headed` is intended for environments with a separately configured display-server connection. The normal Compose configuration alone does not display a window. This runner does not support browsers other than Chromium.

## Output and Measurement Scope

`test-results/report-all.json` or `report-<only>.json` records the runtime environment, interaction results, initial rendering, and frame intervals. Screenshots, saved and restored JSON, and load-test documents are saved in the same output directory. This directory is excluded from Git and the release distribution. On failure, the runner records the cause in JSON, saves a screenshot if the page is available, and exits with a nonzero status.

Initial rendering is measured across three page loads, each in a new browser context with HTTP caching disabled, ending after two `requestAnimationFrame` callbacks following the canvas's appearance. The standard conditions are an empty document, local HTTP, and no CPU or network throttling. HTTP body size and Navigation / Resource Timing `transferSize` are recorded separately.

Interaction performance is measured by loading documents with 100 nodes / 200 edges and 500 nodes / 1,000 edges, then measuring `requestAnimationFrame` intervals during mouse movement, panning, and wheel zooming. Reports include the mean, median, 95th percentile, maximum, and number of frames over 25 ms. Mean fps is `1000 / mean frame interval in ms`. The tests also check that unrelated nodes retain their DOM identity during dragging and count element additions and removals under the SVG. Documents with groups have one frame per five nodes (20 / 100 groups) and include an additional measurement of dragging an entire group. Rows in the `performance` report with `groups` set to 0 have no groups; `group_drag` measures moving a frame along with its member nodes.

The `groups` acceptance results cover creation, name and description editing, membership changes, movement, size limits, rejection of invalid placement, deletion, empty groups, Undo/Redo, cancellation of each type of drag, connecting and panning inside a frame, fitting the entire diagram into view, saving and restoring, v1 compatibility, and protection against invalid recovery data. The screenshot is `groups.png`, and saved documents are named `groups-*.dfd.json`.

`reconnect` tests moving both ends of an edge to different connection points on their respective original nodes. It checks hit detection after panning and zooming and at 10% zoom, autosave protection during previews, preservation of the document and history on cancellation, invalid drops, and drops on the same port, Undo/Redo, preservation of parallel edges and AI-oriented JSON, and saving and restoring. The screenshot is `reconnect.png`, and saved documents are named `reconnect-*.dfd.json`.

Frame intervals include the overhead of headless rendering and test input. Load-to-display times also include file input and automation waits. These values do not measure physical display refreshes or input latency experienced by actual users. Performance values vary with the CPU, GPU, and host OS even when using containers.

To create documents of any size, use the generation command, which uses the same Rust model:

```sh
docker compose run --build --rm tools generate-fixture 100 200 /results/performance-100.dfd.json
docker compose run --build --rm tools generate-fixture 500 1000 /results/performance-500.dfd.json
```

## Reproducibility

Build the tools image from the source being tested and use the pinned toolchain and browser. Compare performance results only when the document sizes, test suites, viewport, browser, and host conditions are comparable. Containerization alone does not make results from different hosts equivalent.

The runner checks that the viewport is 1440×1000 at load time and remains unchanged after screenshots. It uses viewport screenshots because full-page screenshots in chromiumoxide 0.9.1 clear the configured viewport size. Keep generated reports and screenshots local; review the [privacy guidance](../../SECURITY.md) before sharing any results.

See [Container Operations](../../docs/containers.md) for Docker stages, version pinning, caches, and dependency updates.
