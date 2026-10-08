# MergeSort Pro — reproducible release build
FROM rust:1.98-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/mergesort /usr/local/bin/mergesort
COPY --from=build /app/target/release/gen /usr/local/bin/mergesort-gen
EXPOSE 8080
ENTRYPOINT ["mergesort"]
CMD ["--help"]
# Examples:
#   docker build -t mergesort .
#   docker run --rm -v "$PWD:/data" mergesort --input /data/in.csv --output /data/out.csv --max-memory 1GB --format csv --key-column 0 --key-type numeric --verify
#   docker run --rm -v "$PWD:/data" -p 8080:8080 mergesort --input /data/in.csv --output /data/out.csv --max-memory 1GB --format csv --key-column 0 --key-type numeric --verify --dashboard 0.0.0.0:8080
# NOTE: `gen` is installed as `mergesort-gen` to match docs/installer naming.
