# Public demo of zvm on a stand-in chain (no Zcash wallet, no keys, pretend money).
# The mainnet vault is NOT deployed this way: its spending key stays on the operator's machine.
FROM rust:1.96-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p zvm-node

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /src/target/release/zvm /usr/local/bin/zvm
COPY web /app/web
COPY deploy/demo.sh /app/demo.sh
EXPOSE 8080
CMD ["/app/demo.sh"]
