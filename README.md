# DFD Canvas

DFD Canvas is a desktop web app with a Japanese interface for editing data flow diagrams in your browser. Place nodes on an empty canvas, drag between connection ports to connect them, and save or restore diagrams as JSON files.

[Quick start](#setup-and-startup) · [Controls](#basic-controls) · [Contributing](CONTRIBUTING.md) · [Security and privacy](SECURITY.md) · [License](LICENSE)

This is an early release for a single user on a desktop browser. It supports editable nodes and edges, groups, undo/redo, JSON import/export, and browser autosave. The interface is Japanese, and the project documentation is English.

The Rust DFD model is the source of truth for editing. Both the interface and the JSON for AI use are generated from this model. Connections are defined by the node IDs stored in each edge's `source` / `target`; nearby shapes and crossing lines do not imply a connection. JSON for AI use conveys the connection structure and descriptions, but does not guarantee that an AI will understand their business meaning.

Normal editing, saving, and recovery take place entirely in the browser. The Rust web server only serves static files. There is no server-side document processing, LLM communication, telemetry, or external web font loading.

## Technology and versions

The app uses Rust / WebAssembly with Leptos CSR, the server uses Rust with Axum / tower-http, and the development and verification CLI is a Rust `xtask`. Builds, serving, and verification run in Docker. The interface uses SVG, HTML, and plain CSS; Trunk generates the JavaScript that initializes Wasm. Measurement JavaScript injected by browser tests is excluded from the distribution.

| Component | Version and configuration |
| --- | --- |
| Rust | 1.98.1, edition 2024, pinned in `rust-toolchain.toml` |
| Leptos | 0.8.20, `default-features = false`, only `csr` enabled |
| Serde / serde_json | 1.0.229 / 1.0.151 (`float_roundtrip` enabled) |
| wasm-bindgen / web-sys | 0.2.128 / 0.3.105 |
| Trunk | 0.21.14 |
| Server | Axum 0.8.9 / tower-http 0.7.1 |
| Browser verification | chromiumoxide 0.9.1, pinned Chromium / Noto CJK fonts |
| Rendering | SVG (nodes and edges), HTML (controls), plain CSS |

The app uses [Leptos 0.8 APIs](https://docs.rs/leptos/0.8.20/leptos/) and [Trunk](https://github.com/trunk-rs/trunk), with dependencies pinned through `Cargo.lock`. CSR is the only enabled Leptos rendering feature. Release builds enable `opt-level = "s"`, LTO, `codegen-units = 1`, `panic = "abort"`, and symbol stripping. See the [official Leptos setup guide](https://book.leptos.dev/getting_started/index.html) for framework documentation.

Trunk uses wasm-opt 123 for release optimization. To process Rust's standard Wasm output, `index.html` specifies `--enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext`.

## Setup and startup

The host needs Docker Engine or Docker Desktop with Linux containers, Docker Compose v2, and a browser. The supported target is amd64 on Linux / WSL2. Rust, Python, and Node.js do not need to be installed on the host.

Extract the source archive and open a terminal in the directory containing `compose.yaml`.

```sh
docker compose up --build app
```

Open `http://127.0.0.1:8080` in your browser. The first run takes time to download the pinned tools and dependencies and build the app. Press `Ctrl+C` to stop it.

To edit the source, stop the running `app` service and switch to the development service.

```sh
docker compose stop app
docker compose up --build dev
```

Development uses the same URL. Trunk inside the container watches source changes on the host and rebuilds. `app` and `dev` share the same host port, so run only one at a time. Both expose the service only on the host's loopback interface. See [Container operations](docs/containers.md) for details about caches, output paths, and dependency updates.

## Basic controls

The control names below are English descriptions of the Japanese interface.

| Action | How to use it |
| --- | --- |
| Add a node | Use the process, data store, or external entity buttons above the canvas |
| Select | Click a node, edge, group header, or group border |
| Select multiple items | Hold Shift while clicking nodes, edges, or groups |
| Move nodes | Drag a node's body; selected nodes move together |
| Resize a node | Drag the handle at the bottom right of a selected node |
| Create a group | Select the nodes to include and use the create group button |
| Change group membership | Move the entire node into a group's content area or outside its frame |
| Move or resize a group | Drag its header or border to move it; drag its bottom-right handle to resize it |
| Connect nodes | Drag from a port on the top, right, bottom, or left of a node to a port on another node |
| Reattach an edge endpoint | Select one edge and drag either circular endpoint handle to another port on the same node |
| Edit names and descriptions | Use the properties panel on the right; changes commit when the field loses focus |
| Pan | Drag an empty area of the canvas |
| Zoom | Use the mouse wheel to zoom around the cursor; the + / − buttons zoom around the center of the view |
| Fit to view | Use the fit to view button |
| Delete | Use the delete button or press Delete / Backspace |
| Undo | Ctrl / Cmd + Z |
| Redo | Ctrl / Cmd + Shift + Z |
| Cancel an operation | Esc |

The zoom range is 10–400%. Panning can be undone, while wheel zoom, + / − zoom, and fit to view do not add entries to the undo history. Document name changes also commit when the field loses focus.

Processes appear as rounded rectangles, data stores as shapes with parallel lines, and external entities as rectangles. Long names are truncated to fit the diagram; the full text can be viewed and edited in the properties panel. Names and descriptions are plain text.

While connecting nodes, the app shows a preview edge and highlights a candidate port. The connection commits only when released over a valid port. Releasing over empty space or canceling with Esc leaves no dangling edge. Edges are arrowed Bézier curves with a clickable area wider than the visible line.

Selecting a single edge shows circular handles at its source and target. Drag a handle to another port on the same node to change the attachment point. The connected nodes and the edge's ID, name, and description stay the same, as does the JSON for AI use. Dropping over empty space or another node, pressing Esc, canceling the pointer, or losing focus preserves the original attachment. Reattachment can be undone in one step and is included in JSON saves and autosaves.

Multiple edges between the same nodes, edges in opposite directions, and cycles through multiple nodes are allowed. Self-connections are rejected. Deleting a node also deletes all its connected edges in one operation. Undo restores them with their original IDs.

The app retains up to 100 undo operations. Each drag, resize, bulk deletion, or committed property edit counts as one operation. A new edit after Undo clears the redo history. Diagram deletion and undo shortcuts are disabled while an input field has focus or IME composition is in progress.

## Groups and node membership

Groups have names, descriptions, and membership relationships, representing collections such as a resource group and its individual resources. There is **one level of grouping and at most one group per node**: a node is either ungrouped or belongs to one group. Groups cannot be nested. Edges continue to connect nodes to nodes.

Select ungrouped nodes and use the create group button to create a frame with a header and padding. Edit its name and description in the properties panel. If the frame would enclose an unselected node or overlap an existing group, creation is canceled and the reason is displayed.

Dragging a node highlights a candidate group frame. Joining, leaving, or transferring to another group commits on drop. **The entire node must fit inside the content area.** If it overlaps the header, padding, or border, both its position and membership are restored. Moving multiple nodes commits or cancels all of them together. Nodes created with the add buttons also join a group if they fit in its content area; creation is rejected if they cross its boundary.

Dragging a frame's header or border moves all member nodes by the same distance, with connected edges following them. The bottom-right handle expands or shrinks the frame, stopping at the minimum size needed to contain its members. Overlapping frames and moving or expanding a frame to enclose a nonmember node cannot be committed. Empty space inside a frame supports panning, and node operations and connections work as usual. Fit to view includes group frames and headers.

Deleting a group removes its frame and membership relationships while keeping the nodes and edges. An empty group remains when its last member leaves or is deleted. Creation, membership changes, movement, resizing, and deletion can each be undone in one step. Esc, pointer cancellation, loss of pointer capture, or loss of window focus restores the position, size, and membership from before the drag.

## JSON files and browser autosave

The save JSON action downloads a `diagram.dfd.json` file for further editing, transfer, or backup. The load action uses the standard file picker and validates the entire file before replacing the current document. On failure, it displays the reason in Japanese and preserves the document being edited.

LocalStorage recovers the most recent document in the same browser and origin. It does not replace file backups. Autosave is debounced for 550ms after an operation commits; it does not serialize and save JSON on every pointermove during a drag. Save status appears at the bottom of the interface. You can export a JSON file even if autosave fails because browser storage is full or restricted.

Creating or loading a document shows a browser confirmation dialog if it would replace existing nodes, groups, or unsaved changes. Canceling keeps the current document. The app also requests browser confirmation when leaving a page with unsaved edits, but recovery saving does not depend on the moment the page closes.

If recovery data is corrupt, the app displays an error and **disables autosave for the entire session** to protect the original value. Creating a new document or loading a file does not remove this protection. First save the document you are editing as a JSON file. To inspect the protected original data, copy the `dfd-studio.document.v1` value from LocalStorage in the browser's developer tools. After inspecting and preserving it, delete only that key in developer tools and reload the page to resume autosave. The app has no operation for deleting corrupt recovery data.

The app is intended for use in a single tab. It does not handle conflicts from concurrent editing in multiple tabs, browser data deletion, or data loss when a private browsing session ends. Undo history, selection state, and temporary edges during a drag are not saved.

## Saved JSON and JSON for AI use

| Content | Saved `.dfd.json` | JSON for AI use |
| --- | --- | --- |
| Schema version and document name | Included | Included |
| Node IDs, kinds, names, and descriptions | Included | Included |
| Group IDs, names, and descriptions; each node's group ID | Included | Included |
| Edge IDs, source IDs, target IDs, names, and descriptions | Included | Included |
| Node and group positions and sizes; top/right/bottom/left ports | Included | Excluded |
| Pan and zoom | Included | Excluded |
| Import into the editor | Supported | Not supported |

The JSON for AI use action displays the current document's semantic information, which can be copied or exported as `diagram.ai.json`. If clipboard access is denied, use the select all text action to copy manually or save it as a file. This JSON is derived from the document rather than maintained as a separate source of truth. The app does not continuously generate JSON for display while the dialog is closed.

Nodes, edges, and groups are output in ID order. Renaming, moving, or resizing does not change IDs. Changes limited to coordinates or view state do not change the JSON for AI use. Moving an entire group while retaining membership also leaves it unchanged, while joining, leaving, or transferring nodes changes semantic information.

### Format overview

- `schema_version`: Currently the integer `2`. Legacy `1` documents load without groups and use `2` on the next save. The autosave key remains `dfd-studio.document.v1` after the DFD Canvas rename so existing browser saves remain recoverable on the same origin.
- `name`: The document name.
- `graph.nodes`: An array of `{ id, kind, name, description, group_id }`. `kind` is `process` / `data_store` / `external_entity`. `group_id` is the containing group's ID, or `null` for an ungrouped node.
- `graph.groups`: An array of `{ id, name, description }`.
- `graph.edges`: An array of `{ id, source, target, name, description }`. `source` / `target` are IDs of existing nodes.
- `layout.nodes`: `{ x, y, width, height }` keyed by node ID.
- `layout.groups`: `{ x, y, width, height }` keyed by group ID, describing the entire frame rectangle including its header.
- `layout.edges`: `{ source_port, target_port }` keyed by edge ID. Ports are `top` / `right` / `bottom` / `left`.
- `layout.viewport`: `{ pan_x, pan_y, zoom }`.

IDs are nonempty strings that are unique across all nodes, edges, and groups. The editor assigns sequential IDs independently of names and coordinates, avoiding collisions with imported IDs. `description` is always present and uses an empty string when there is no description. The save format does not depend on Leptos state or the SVG DOM structure.

### Minimal examples

An empty document is valid saved JSON.

```json
{
  "schema_version": 2,
  "name": "New DFD",
  "graph": {
    "nodes": [],
    "edges": [],
    "groups": []
  },
  "layout": {
    "nodes": {},
    "edges": {},
    "groups": {},
    "viewport": { "pan_x": 0.0, "pan_y": 0.0, "zoom": 1.0 }
  }
}
```

This saved JSON example connects two nodes and places the process in a group.

```json
{
  "schema_version": 2,
  "name": "Order intake",
  "graph": {
    "nodes": [
      { "id": "node-000001", "kind": "external_entity", "name": "Customer", "description": "Person submitting an order", "group_id": null },
      { "id": "node-000002", "kind": "process", "name": "Receive order", "description": "Check order details", "group_id": "group-000004" }
    ],
    "edges": [
      { "id": "edge-000003", "source": "node-000001", "target": "node-000002", "name": "Order data", "description": "Product IDs and quantities" }
    ],
    "groups": [
      { "id": "group-000004", "name": "Order service", "description": "Resources for order intake" }
    ]
  },
  "layout": {
    "nodes": {
      "node-000001": { "x": 60.0, "y": 100.0, "width": 160.0, "height": 80.0 },
      "node-000002": { "x": 360.0, "y": 100.0, "width": 200.0, "height": 80.0 }
    },
    "edges": {
      "edge-000003": { "source_port": "right", "target_port": "left" }
    },
    "groups": {
      "group-000004": { "x": 344.0, "y": 52.0, "width": 232.0, "height": 144.0 }
    },
    "viewport": { "pan_x": 0.0, "pan_y": 0.0, "zoom": 1.0 }
  }
}
```

The JSON for AI use contains the same document with `layout` omitted.

### Input validation and limits

The maximum import file size is **5 MiB (5,242,880 bytes)**. There is no separate limit on the number of nodes, edges, or groups.

The app rejects invalid JSON, unsupported schema versions, node kinds or ports, unknown fields, empty IDs, duplicate IDs, broken references, self-connections, and missing or extra layout entries. Numeric values must be finite. Coordinates and pan values must be between −1,000,000 and 1,000,000; node widths between 80 and 10,000; node heights between 48 and 10,000; and zoom between 0.1 and 4.0. NaN / Infinity, which are invalid JSON syntax, are also rejected.

Validation also checks group references, nonoverlapping frames, complete containment of member nodes within the content area, and the absence of intersections between nonmember nodes and frames. Frame widths and heights range from 112 to 2,010,032. Headers are 32 units tall and inner padding is 16 units, in diagram coordinates. Nodes may touch the edges of a group's content area, and group frames may touch each other's borders, but placements that cross boundaries are rejected.

Validation checks connection reference integrity and format. It does not enforce connection restrictions based on node kinds or validate business-specific DFD notation.

## Tests and release builds

Create the verification output directory first and match the container user to the host's UID / GID. Both default to 1000.

```sh
export DFD_UID="$(id -u)" DFD_GID="$(id -g)"
mkdir -p test-results
docker compose run --build --rm tools check
docker compose run --build --rm tools browser-test
```

`check` runs formatting checks, native workspace tests and Clippy, frontend Wasm checks and Clippy, and a Trunk release build. It verifies the Rust model, editing logic, coordinate transformations, server, and helper CLI. Cargo and Trunk use `--locked`.

`browser-test` temporarily starts the Rust server in the same container and verifies the release distribution in Chromium. No separate server is needed. Result JSON, screenshots, and saved and restored documents remain in the host's `test-results/` directory.

Individual checks, distribution size measurements, and document generation at arbitrary sizes also run inside the container.

```sh
docker compose run --build --rm tools browser-test --only acceptance
docker compose run --build --rm tools browser-test --only performance
docker compose run --build --rm tools browser-test --only startup
docker compose run --build --rm tools measure-dist
docker compose run --build --rm tools generate-fixture 100 200 /results/normal.dfd.json
docker compose run --build --rm tools generate-fixture 500 1000 /results/stress.dfd.json
```

Large performance test documents are generated at runtime and excluded from the production image along with test code. Small import examples are in `tests/fixtures/example.dfd.json` and `tests/fixtures/invalid.dfd.json`. See [Browser verification](tests/browser/README.md) for browser CLI options and measurement definitions.

The Cargo workspace consists of the `dfd-canvas` frontend, the `dfd-server` server, and the `xtask` verification and helper CLI. Server and browser verification dependencies are excluded from Wasm. The frontend is split into `src/model.rs` (save format and validation), `src/editor.rs` (editing and history), `src/geometry.rs` (coordinates and curves), `src/app.rs` (Leptos interface and interactions), and `src/storage.rs` (saving through Web APIs).

The tests cover model validation, editing and undo/redo, fractional coordinate round trips, group membership, edge endpoint reattachment, file import/export, autosave recovery, invalid input, and cancellation of pointer operations. Browser verification uses synthetic diagrams. Generated reports and screenshots stay outside the source distribution.

## Serving static files

`docker compose up --build app` starts a minimal serving image containing the Rust server and release distribution. The server runs as a nonroot user and serves HTML, CSS, JavaScript, and Wasm. Wasm uses the `application/wasm` Content-Type. Missing files return 404; an invalid directory at startup or a port conflict causes an error.

To deploy to another static host, extract the entire `/app/dist` directory from the image.

```sh
docker compose build app
docker compose create app
docker compose cp app:/app/dist/. ./dist
```

Serve the files over HTTP rather than opening the HTML directly with `file://`. To deploy under a subdirectory, set `build.public_url` in `Trunk.toml` to match the destination before building. The default is the relative path `./`.

To enable compressed transfers, configure gzip / Brotli and the appropriate `Content-Encoding` on the serving host. Compressed file size and the amount actually transferred to the browser are separate measurements.

## Performance verification

Startup cost and responsiveness during interaction are measured separately. The normal workload uses 100 nodes and 200 edges; the stress workload uses 500 nodes and 1,000 edges. These counts are performance test sizes, not document format limits.

Use the startup and performance suites to measure your environment. Grouped documents contain one frame per five nodes. Frame intervals include browser rendering and automation overhead; they do not measure physical display refreshes or guarantee a particular frame rate. Large diagrams can lose responsiveness during dragging and continuous zoom. Fitting them to the view can make text small and cause edges or labels to overlap.

Distribution sizes can be measured separately:

```sh
docker compose run --build --rm tools measure-dist
```

The report distinguishes uncompressed sizes from independently gzipped file sizes. These are different from network transfer sizes, and gzip is not automatically enabled when serving the files. Treat performance reports, environment details, and screenshots as private until separately reviewed for publication.

## Initial release scope and limitations

- Designed for desktop viewports at least 900px wide, with one document, one canvas, and one browser tab. There is no dedicated smartphone interface.
- Marquee selection, copy and paste, automatic layout, obstacle routing, bend-point editing, and self-connections are outside the scope of this release.
- Edges between the same nodes use simple offsets to distinguish them. Adjust node positions or ports to resolve overlapping edges and labels in complex diagrams.
- The minimum zoom is 10%. Diagrams spanning a very large coordinate range may not fit entirely in the viewport even with fit to view.
- Image placement, freehand drawing, hierarchical DFDs, collaboration, management of multiple documents, and cloud sync are not supported.
- Mermaid / PlantUML output, SVG / PNG / PDF export, import of JSON for AI use, and automatic editing by an LLM are not supported.
- Copying JSON for AI use to the clipboard depends on browser permissions and the serving environment. Displayed text and file export are available as alternatives.
- The transitive dependency `proc-macro-error2 2.0.1` emits a Rust future compatibility warning concerning proc-macro re-exports. Builds succeed with the pinned toolchain. Check dependency compatibility when updating Rust in the future.

Automated browser verification has only been performed with the pinned headless Chromium environment. Firefox, Safari, Edge, ordinary Chrome windows, other operating systems, and interactions on physical displays have not been verified. Editing and saving Japanese strings and suppressing unintended shortcuts during synthetic `isComposing` events have been checked, but composition and confirmation using an actual OS Japanese IME have not. Verify these in the target environment using `--headed` browser tests or the basic controls described above.

## License

DFD Canvas is licensed under the [MIT License](LICENSE). Dependencies retain their respective licenses.
