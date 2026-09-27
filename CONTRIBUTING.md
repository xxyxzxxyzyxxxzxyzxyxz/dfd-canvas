# Contributing to DFD Canvas

If a public contribution repository is provided, use its issues for reproducible bugs and focused feature proposals. For a larger change, describe the problem and intended behavior before starting implementation. A source archive alone does not establish a contribution channel. Follow [Security and privacy](SECURITY.md) for suspected vulnerabilities.

## Development setup

The supported development environment is amd64 on Linux / WSL2 with Docker, Compose v2, and a browser. Build tools run inside containers; you do not need Rust or Node.js on the host.

Prepare a working copy of the source. From its root, run:

```sh
export DFD_UID="$(id -u)" DFD_GID="$(id -g)"
mkdir -p test-results
docker compose stop app
docker compose up --build dev
```

Open `http://127.0.0.1:8080`. The development server watches source changes. Stop it with `Ctrl+C`. The `app` and `dev` services share a port, so run only one at a time. See [Container operations](docs/containers.md) for build caches, file ownership, pinned tools, and dependency updates.

## Making changes

- Keep changes focused, and describe any changes to saved JSON compatibility or editing behavior.
- Keep the Rust document model as the source of truth. Put format and validation changes in `src/model.rs`, editing and history in `src/editor.rs`, geometry in `src/geometry.rs`, browser interaction in `src/app.rs`, and browser storage in `src/storage.rs`.
- Write documentation and code comments in English. The current product interface is Japanese; preserve intentional Japanese UI text and language-specific test cases unless the change specifically concerns them.
- Add or update tests when behavior changes. Use small synthetic diagrams for reproductions and fixtures. Do not commit personal diagrams, credentials, generated build output, or `test-results/`.
- Keep dependencies pinned and include relevant lockfile changes. Follow the container workflow when updating dependencies or running rustfmt with a writable source mount.

## Verification

Run the checks from the repository root after code changes:

```sh
docker compose run --build --rm tools check
docker compose run --build --rm tools browser-test
```

`check` verifies formatting, runs native tests and Clippy, checks the Wasm frontend, and builds the release distribution. `browser-test` starts its own temporary server and exercises that distribution in Chromium. Keep `--build` after source changes so the image contains your changes. Reports and screenshots are written to `test-results/`.

For an iteration limited to interaction behavior, you can run `browser-test --only acceptance`. Changes to rendering or performance should also be checked with the performance and startup suites. See [Browser verification](tests/browser/README.md) for options and measurement limitations. For documentation-only changes, review the rendered text, links, examples, and `git diff --check`; application tests are unnecessary unless an executable example or configuration changes.

## Submitting changes

Use the contribution channel supplied by the owner. Explain the problem, the resulting behavior, and how you verified the change. Link related issues when available and include screenshots when they help review a visible change. Report any checks you could not run and why. Keep documentation consistent with the final implementation, and distinguish newly measured performance results from historical records.

Review every attachment and diff before sharing. Public materials must omit private personal information, unapproved email addresses and account links, machine-specific paths, private development history, and private diagram content. The project's approved public commit identity is `dddd <dddd@dddd>`. New public update history may use that identity; private development history must never be imported or merged into it. Source archives exclude Git metadata. These requirements also apply to reports, screenshots, and logs.

## Merge authorization

CI results indicate validation only. Final merge authorization belongs exclusively to the owner or the owner's locally operated Codex. Passing checks, automated reviews, bots, and auto-merge settings do not grant that authorization. Do not use automatic or bot-driven merging.

This is the project's review policy; it does not claim that GitHub rules or other enforcement settings are configured.
