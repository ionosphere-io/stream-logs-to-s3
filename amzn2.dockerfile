FROM public.ecr.aws/amazonlinux/amazonlinux:2
RUN yum update -y
RUN yum groupinstall -y 'Development Tools'
RUN amazon-linux-extras install -y epel
RUN yum install -y clang-devel llvm-devel openssl-devel zip

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs > rustup.sh
RUN /bin/sh rustup.sh --default-host x86_64-unknown-linux-gnu --default-toolchain nightly -y
ENV PATH=/root/.cargo/bin:$PATH
RUN cargo install rls || true

RUN mkdir /stream-logs-to-s3
COPY ["Cargo.toml", "Cargo.lock", "/stream-logs-to-s3/"]
COPY src /stream-logs-to-s3/src
WORKDIR /stream-logs-to-s3
RUN cargo build --target x86_64-unknown-linux-gnu
RUN cargo build --target x86_64-unknown-linux-gnu --release
WORKDIR /stream-logs-to-s3/target/x86_64-unknown-linux-gnu
RUN VERSION=$(egrep '^version = ' ../../Cargo.toml | sed -E -e 's/[^"]+"([^"]+)"/\1/'); \
cd debug; gzip < stream-logs-to-s3 > ../../../stream-logs-to-s3-${VERSION}-amzn2-debug.gz; \
cd ../release; gzip < stream-logs-to-s3 > ../../../stream-logs-to-s3-${VERSION}-amzn2-release.gz
WORKDIR /stream-logs-to-s3
RUN ls
RUN VERSION=$(egrep '^version = ' Cargo.toml | sed -E -e 's/[^"]+"([^"]+)"/\1/'); \
zip -0 stream-logs-to-s3-${VERSION}-amzn2.zip stream-logs-to-s3-${VERSION}-amzn2-debug.gz stream-logs-to-s3-${VERSION}-amzn2-release.gz
