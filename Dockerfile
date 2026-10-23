# syntax=docker/dockerfile:1
FROM rust:1.96-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends libdw-dev libpq-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p dynexus-processor -p dynexus-api \
    && cp target/release/dynexus-processor target/release/dynexus-api /usr/local/bin/

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends libdw1 libpq5 libssl3 ca-certificates \
    && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10001 dynexus
COPY --from=build /usr/local/bin/dynexus-processor /usr/local/bin/dynexus-api /usr/local/bin/
USER dynexus
CMD ["dynexus-api"]
