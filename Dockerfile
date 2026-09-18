# MergeSort Pro — reproducible release build
FROM rust:1.98-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/mergesort /usr/local/bin/mergesort
COPY --from=build /app/target/release/gen /usr/local/bin/mergesort-gen
ENTRYPOINT ["mergesort"]
CMD ["--help"]
