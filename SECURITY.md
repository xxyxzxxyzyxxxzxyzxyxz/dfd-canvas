# Security and Privacy

## Reporting a vulnerability

Do not post vulnerability details, exploit instructions, credentials, or private diagrams in public issues or pull requests.

GitHub Private Vulnerability Reporting is enabled for this repository. Open the repository's **Security** tab and select **Report a vulnerability** to submit a private report. See [GitHub's reporting instructions](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/report-privately).

If the private reporting button is unavailable, retain the report privately until this policy provides an available private channel. Do not substitute a public issue or pull request.

In a private report, include the affected source version, environment, impact, and minimal reproduction steps using synthetic data. Omit personal names, email addresses, identifying account links, machine-specific paths, and unrelated private data. A fix or mitigation suggestion is helpful when available. There is no published security backport policy or guaranteed response time.

## Application scope

DFD Canvas edits diagrams in the browser. Normal editing, validation, file import and export, and autosave do not send document contents to the Rust server. The server serves static application files; the app has no server-side document storage, user accounts, LLM integration, telemetry, or external web font requests. Exporting JSON for AI use does not contact an AI service. Any later upload or paste into another service is a separate action by the user.

Autosave stores JSON in the browser's LocalStorage under the legacy key `dfd-studio.document.v1` to preserve compatibility with existing data. The app does not encrypt this value or downloaded JSON files. Browser profile access, extensions, and scripts running on the same origin can affect the confidentiality of browser data. Use a trusted browser profile and keep file backups; LocalStorage is not a backup or access-control mechanism.

Saved diagrams and AI-oriented JSON contain names, descriptions, and graph structure. Screenshots, verification reports, and logs may also reveal diagram content or local paths. Review and redact these before sharing them.

## Server and container scope

The supplied Compose configuration publishes `app` and `dev` only on the host's `127.0.0.1`. The server listens on `0.0.0.0:8080` inside the container. Its `--public-url` option changes the printed URL, not the listening address or network access controls. Running the server or container outside Compose requires choosing an appropriate bind address and port mapping.

The server has no built-in authentication or TLS. The documented deployment is local use; exposing it to a network requires separate access controls and transport protection. Serve only trusted build output from the configured static directory, and keep credentials, private documents, and other unrelated files out of that directory.

The production `app` service runs as a non-root user with a read-only filesystem. The development and test containers include build tools and have different permissions. Browser verification disables Chromium's namespace sandbox in its dedicated test container; use that environment only for project tests.

Tool and dependency versions are pinned for reproducibility, and updates are manual. Pinning does not guarantee that a version is free of vulnerabilities. See [Container operations](docs/containers.md) for the update procedure and [Contributing](CONTRIBUTING.md) for verification commands.
