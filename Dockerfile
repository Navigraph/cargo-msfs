FROM rust:1.84-slim-bookworm

WORKDIR /build

# Install deps
RUN apt update && \
    apt install -y --no-install-recommends lsb-release wget software-properties-common gnupg g++ build-essential openssl gcc-multilib

# Install llvm/clang
RUN wget https://apt.llvm.org/llvm.sh && \
    chmod +x llvm.sh && \
    ./llvm.sh 17 && \
    ln -s $(which clang-17) /usr/bin/clang && \
    ln -s $(which llvm-ar-17) /usr/bin/llvm-ar && \
    rm llvm.sh

# Install rust target
RUN rustup target install wasm32-wasip1

# Copy over files
COPY src src
COPY Cargo.toml Cargo.toml

# Build and install cargo-msfs binary
RUN cargo install --path .

LABEL org.opencontainers.image.source=https://github.com/Navigraph/cargo-msfs
