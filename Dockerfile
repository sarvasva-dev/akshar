FROM rust:1.86-slim-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl unzip poppler-utils \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/akshar /usr/local/bin/akshar
COPY config.json THIRD_PARTY_NOTICES.md ./
COPY web ./web
COPY scripts ./scripts

RUN sh scripts/setup.sh

EXPOSE 8000
ENTRYPOINT ["akshar"]
CMD ["--serve", "--host", "0.0.0.0", "--port", "8000"]
