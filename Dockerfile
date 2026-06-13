# rototo console image: one binary serving the console UI and API.
#
#   docker build -t rototo-console .
#   docker run -p 7686:7686 rototo-console \
#     --read-only --workspace https://api.github.com/repos/acme/config/tarball/main
#
# Team-mode deployments pass GITHUB_CLIENT_ID / GITHUB_CLIENT_SECRET /
# ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY / ROTOTO_CONSOLE_PUBLIC_URL instead of
# --read-only, and mount a volume at /data for console state.

FROM node:24-slim AS ui
WORKDIR /build
COPY apps/console/package.json apps/console/package-lock.json apps/console/
RUN npm --prefix apps/console ci
COPY apps/console apps/console
COPY spec spec
RUN npm --prefix apps/console run build

FROM rust:slim AS binary
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock build.rs rust-toolchain.toml ./
COPY src src
COPY docs docs
COPY spec spec
COPY examples/sdk-app examples/sdk-app
COPY sdks sdks
COPY go.mod go.mod
COPY --from=ui /build/apps/console/dist apps/console/dist
RUN cargo build --release --locked --package rototo

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/*
COPY --from=binary /build/target/release/rototo /usr/local/bin/rototo
ENV ROTOTO_CONSOLE_DATA_DIR=/data
VOLUME /data
EXPOSE 7686
ENTRYPOINT ["rototo", "console", "--bind", "0.0.0.0:7686"]
