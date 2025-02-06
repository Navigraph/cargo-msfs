FROM rust:1.84 AS builder

WORKDIR /build

COPY src src
COPY Cargo.toml Cargo.toml

# Install rust target
RUN rustup target install wasm32-wasip1

# Install cargo-msfs
RUN cargo install --path .

# Build cargo-msfs binary
RUN cargo build --release

# --------------------------------------------
# Compact image without rust deps and extras
# --------------------------------------------
FROM debian:bookworm-slim

# Install llvm deps
RUN apt update && \
    apt install -y --no-install-recommends lsb-release wget software-properties-common gnupg 

# Install llvm/clang
RUN wget https://apt.llvm.org/llvm.sh && \
    chmod +x llvm.sh && \
    ./llvm.sh 17 && \
    ln -s $(which clang-17) /usr/bin/clang && \
    ln -s $(which llvm-ar-17) /usr/bin/llvm-ar && \
    rm llvm.sh

WORKDIR /cargo-msfs

COPY --from=builder /build/target/release/cargo-msfs .
RUN chmod +x cargo-msfs

# Add cargo-msfs to PATH
ENV PATH="/cargo-msfs:${PATH}"

LABEL org.opencontainers.image.source=https://github.com/Navigraph/cargo-msfs
