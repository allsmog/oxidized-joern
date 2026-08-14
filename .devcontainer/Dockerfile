FROM rust:1.97-bookworm

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
       ca-certificates clang curl git libclang-dev pkg-config python3 shellcheck \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspaces/oxidized-joern
