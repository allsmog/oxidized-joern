FROM rust:1.97-bookworm AS build

WORKDIR /src
COPY cpg-rs ./cpg-rs
RUN cargo build --manifest-path cpg-rs/Cargo.toml --release --locked -p cpg-cli

FROM debian:bookworm-slim

LABEL org.opencontainers.image.source="https://github.com/allsmog/oxidized-joern" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.title="Oxidized Joern cpg"

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /workspace \
    && chown 65532:65532 /workspace

COPY --from=build /src/cpg-rs/target/release/cpg /usr/local/bin/cpg

USER 65532:65532
WORKDIR /workspace
ENTRYPOINT ["cpg"]
