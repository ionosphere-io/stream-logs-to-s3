FROM alpine:3
ARG ARCH
RUN apk add --no-cache curl
RUN ls -la
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs > rustup.sh
RUN /bin/sh rustup.sh --default-host ${ARCH}-unknown-linux-musl --default-toolchain nightly -y
ENV PATH=/root/.cargo/bin:$PATH
RUN cargo install rls || true

RUN mkdir /stream-logs-to-s3
COPY ["Cargo.toml", "Cargo.lock", "/stream-logs-to-s3/"]
COPY src /stream-logs-to-s3/src
WORKDIR /stream-logs-to-s3
RUN apk add --no-cache clang gcc musl-dev openssl-dev openssl-libs-static zip
RUN cargo build --target ${ARCH}-unknown-linux-musl
RUN cargo build --target ${ARCH}-unknown-linux-musl --release
RUN VERSION=$(egrep '^version = ' Cargo.toml | sed -E -e 's/[^"]+"([^"]+)"/\1/') && \
ln target/${ARCH}-unknown-linux-musl/debug/stream-logs-to-s3 stream-logs-to-s3-${VERSION}-musl-${ARCH}-debug && \
ln target/${ARCH}-unknown-linux-musl/release/stream-logs-to-s3 stream-logs-to-s3-${VERSION}-musl-${ARCH}-release && \
zip -9 stream-logs-to-s3-${VERSION}-musl-${ARCH}.zip stream-logs-to-s3-${VERSION}-musl-${ARCH}-debug stream-logs-to-s3-${VERSION}-musl-${ARCH}-release
