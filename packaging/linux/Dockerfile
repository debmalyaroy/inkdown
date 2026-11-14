# Linux (x86-64) build of Inkdown.
#   docker build -f packaging/linux/Dockerfile -t inkdown-linux .
#   docker run --rm -v "${PWD}/dist/linux:/out" inkdown-linux
# The container compiles the release binary, runs the unit tests, smoke-tests the
# window under a virtual display, and copies the result plus a .deb to /out.
FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
      libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libxdo-dev \
      pkg-config build-essential xvfb xauth dpkg-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY assets ./assets
RUN cargo build --release && cargo test --release && strip target/release/inkdown
COPY packaging/linux ./packaging/linux
RUN sh packaging/linux/make-deb.sh
CMD ["sh", "-c", "sh packaging/linux/smoke.sh; cp target/release/inkdown /out/inkdown && cp target/*.deb /out/ && ls -la /out"]
