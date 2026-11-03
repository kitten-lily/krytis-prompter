# Build environment for `mise run live-check --container`, for hosts without the
# GTK 4, gtk4-layer-shell and gcr-4 development files (krytis has no package
# manager). The same Ubuntu and system packages as CI's Checks job
# (.github/workflows/checks.yml), plus the Rust that mise.toml pins, passed in
# by the task as RUST_VERSION. docs/skills/ui.md § The live check in a container.
FROM docker.io/library/ubuntu:26.04@sha256:6a06111c3b0a8600feadb270aec771dc239bf407b25625f93f44ab00e11f4d3c

# build-essential: cc links the test binaries. fontconfig: GTK finds the
# host's fonts the task mounts. rustup: installs the pinned toolchain below.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential ca-certificates fontconfig rustup \
        libgtk-4-dev libgtk4-layer-shell-dev libgcr-4-dev pkgconf dbus-daemon \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo
ENV PATH=/opt/cargo/bin:$PATH
ARG RUST_VERSION
RUN test -n "$RUST_VERSION" \
    && rustup toolchain install "$RUST_VERSION" --profile minimal \
    && rustup default "$RUST_VERSION"
