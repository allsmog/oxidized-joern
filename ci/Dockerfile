FROM ubuntu:24.04@sha256:561618e2c15bf2397621dd04f96926663a3b5616c189cf7e38db7e82f5c538ea

ARG TARGETARCH

LABEL org.opencontainers.image.authors="Oxidized Joern contributors" \
      org.opencontainers.image.source="https://github.com/allsmog/oxidized-joern" \
      org.opencontainers.image.url="https://github.com/allsmog/oxidized-joern" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.title="Oxidized Joern cpg" \
      org.opencontainers.image.description="Rust-native code property graph CLI preview"

COPY ci/oxidized-joern-cpg-linux-${TARGETARCH}.tar.gz /tmp/cpg.tar.gz

RUN mkdir -p /opt/cpg /workspace \
    && tar -xzf /tmp/cpg.tar.gz --strip-components=1 -C /opt/cpg \
    && rm /tmp/cpg.tar.gz \
    && chown -R 65532:65532 /workspace \
    && /opt/cpg/cpg --version

ENV PATH=/opt/cpg:${PATH}

USER 65532:65532
WORKDIR /workspace

ENTRYPOINT ["cpg"]
