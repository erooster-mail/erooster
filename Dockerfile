FROM rustlang/rust:nightly-slim AS chef

# We only pay the installation cost once,
# it will be cached from the second build onwards
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare  --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin erooster

# We do not need the Rust toolchain to run the binary!
FROM gcr.io/distroless/cc:nonroot
LABEL maintainer="mtrnord@nordgedanken.dev"
USER nonroot

EXPOSE 993
EXPOSE 143
EXPOSE 25
EXPOSE 465

WORKDIR /
COPY --from=builder /app/target/release/erooster /
ENTRYPOINT ["/erooster"]