# Fixed amd64 toolchain; see docs/containers.md for updates.
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS toolchain
ARG DFD_UID=1000
ARG DFD_GID=1000
ENV NO_COLOR=true CARGO_TARGET_DIR=/build/target CARGO_BUILD_JOBS=4
RUN rustup component add rustfmt clippy \
    && rustup target add wasm32-unknown-unknown
RUN curl -fsSL --retry 3 https://github.com/trunk-rs/trunk/releases/download/v0.21.14/trunk-x86_64-unknown-linux-gnu.tar.gz -o /tmp/trunk.tar.gz \
    && echo 'f2b4680cd239693a646a2795e4633c625328d7b2a044fbe749fa3a2fe9e7036b  /tmp/trunk.tar.gz' | sha256sum -c - \
    && tar -xzf /tmp/trunk.tar.gz -C /usr/local/bin trunk \
    && rm /tmp/trunk.tar.gz
RUN curl -fsSL --retry 3 https://github.com/wasm-bindgen/wasm-bindgen/releases/download/0.2.128/wasm-bindgen-0.2.128-x86_64-unknown-linux-musl.tar.gz -o /tmp/bindgen.tar.gz \
    && echo 'b51f0208fdff83515a787bd8ab9ac5865ed84dabb66d0c709957bb59793c645f  /tmp/bindgen.tar.gz' | sha256sum -c - \
    && tar -xzf /tmp/bindgen.tar.gz --strip-components=1 -C /usr/local/bin wasm-bindgen-0.2.128-x86_64-unknown-linux-musl/wasm-bindgen \
    && rm /tmp/bindgen.tar.gz
RUN curl -fsSL --retry 3 https://github.com/WebAssembly/binaryen/releases/download/version_123/binaryen-version_123-x86_64-linux.tar.gz -o /tmp/binaryen.tar.gz \
    && echo 'e959f2170af4c20c552e9de3a0253704d6a9d2766e8fdb88e4d6ac4bae9388fe  /tmp/binaryen.tar.gz' | sha256sum -c - \
    && tar -xzf /tmp/binaryen.tar.gz --strip-components=2 -C /usr/local/bin binaryen-version_123/bin/wasm-opt \
    && rm /tmp/binaryen.tar.gz
RUN (getent group ${DFD_GID} >/dev/null || groupadd --gid ${DFD_GID} dfd) \
    && useradd --uid ${DFD_UID} --gid ${DFD_GID} --create-home dfd \
    && mkdir -p /workspace /build /app /results /usr/local/cargo/registry /usr/local/cargo/git \
    && chown -R ${DFD_UID}:${DFD_GID} /workspace /build /app /results /usr/local/cargo
WORKDIR /workspace
USER dfd

FROM toolchain AS dev
EXPOSE 8080
CMD ["trunk", "serve", "--address", "0.0.0.0", "--port", "8080", "--locked", "--dist", "/build/dist", "--poll"]

FROM toolchain AS build
COPY --chown=${DFD_UID}:${DFD_GID} . .
RUN --mount=type=cache,id=dfd-registry-${DFD_UID}-${DFD_GID},target=/usr/local/cargo/registry,uid=${DFD_UID},gid=${DFD_GID} \
    --mount=type=cache,id=dfd-build-${DFD_UID}-${DFD_GID},target=/build/target,uid=${DFD_UID},gid=${DFD_GID} \
    cargo build --locked --release -p dfd-server \
    && cp /build/target/release/dfd-server /app/dfd-server \
    && trunk build --release --locked --dist /app/dist

FROM build AS tools-build
RUN --mount=type=cache,id=dfd-registry-${DFD_UID}-${DFD_GID},target=/usr/local/cargo/registry,uid=${DFD_UID},gid=${DFD_GID} \
    --mount=type=cache,id=dfd-build-${DFD_UID}-${DFD_GID},target=/build/target,uid=${DFD_UID},gid=${DFD_GID} \
    cargo build --locked -p xtask \
    && cp /build/target/debug/xtask /app/xtask

FROM toolchain AS browser-base
USER root
# Snapshot freezes Chromium, fonts and their transitive OS dependencies together.
RUN rm -f /etc/apt/sources.list.d/debian.sources \
    && printf 'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/20260924T000000Z/ bookworm main\ndeb [check-valid-until=no] https://snapshot.debian.org/archive/debian-security/20260924T000000Z/ bookworm-security main\n' > /etc/apt/sources.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends chromium fonts-noto-cjk \
    && rm -rf /var/lib/apt/lists/* \
    && chromium --version
USER dfd

FROM browser-base AS tools
COPY --from=tools-build --chown=${DFD_UID}:${DFD_GID} /workspace /workspace
COPY --from=tools-build /app/xtask /usr/local/bin/xtask
COPY --from=build /app/dist /app/dist
ENV DFD_DIST=/app/dist DFD_BUILD_DIST=/build/dist DFD_CHROMIUM=/usr/bin/chromium DFD_CHROMIUM_NO_SANDBOX=1
ENTRYPOINT ["xtask"]
CMD ["check"]

FROM debian:bookworm-slim@sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251 AS app
WORKDIR /app
COPY --from=build /app/dfd-server /usr/local/bin/dfd-server
COPY --from=build /app/dist /app/dist
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["dfd-server"]
CMD ["--listen", "0.0.0.0:8080", "--dir", "/app/dist"]
