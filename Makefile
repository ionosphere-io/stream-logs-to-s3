VERSION := $(shell egrep '^version = ' Cargo.toml | sed -E -e 's/[^"]+"([^"]+)"/\1/')
SRCS = src/*.rs
OS_SUFFIXES = amzn2 el7 musl macos
ARCH_SUFFIXES = aarch64 x86_64
ZIP_FILE_BASES = $(foreach os,${OS_SUFFIXES},stream-logs-to-s3-${VERSION}-${os}-)
ZIP_FILES = $(foreach arch,${ARCH_SUFFIXES},$(addsuffix ${arch}.zip,${ZIP_FILE_BASES}))

all: ${ZIP_FILES}
macos: stream-logs-to-s3-${VERSION}-macos.zip

clean:
	rm -f stream-logs-to-s3-*.zip

stream-logs-to-s3-${VERSION}-%-x86_64.zip: %.dockerfile ${SRCS} Cargo.toml Cargo.lock
	docker buildx build --platform=linux/amd64 --progress=plain --build-arg ARCH=x86_64 --tag stream-logs-to-s3:${VERSION}-$*-x86_64 -f $*.dockerfile .
	docker run --rm stream-logs-to-s3:${VERSION}-$*-x86_64 cat stream-logs-to-s3-${VERSION}-$*-x86_64.zip > stream-logs-to-s3-${VERSION}-$*-x86_64.zip

stream-logs-to-s3-${VERSION}-%-aarch64.zip: %.dockerfile ${SRCS} Cargo.toml Cargo.lock
	docker buildx build --platform=linux/arm64 --progress=plain --build-arg ARCH=aarch64 --tag stream-logs-to-s3:${VERSION}-$*-aarch64 -f $*.dockerfile .
	docker run --rm stream-logs-to-s3:${VERSION}-$*-aarch64 ls
	docker run --rm stream-logs-to-s3:${VERSION}-$*-aarch64 cat stream-logs-to-s3-${VERSION}-$*-aarch64.zip > stream-logs-to-s3-${VERSION}-$*-aarch64.zip

stream-logs-to-s3-${VERSION}-macos-x86_64.zip: ${SRCS} Cargo.toml Cargo.lock
	@if [[ "$(shell uname -s)" != "Darwin" ]]; then echo "You must run this on a Mac to build the Mac target: $(shell uname -s)" 1>&2; exit 1; fi
	cargo build --target x86_64-apple-darwin
	cargo build --target x86_64-apple-darwin --release
	rm -f stream-logs-to-s3-${VERSION}-macos-x86_64-debug stream-logs-to-s3-${VERSION}-macos-x86_64-release
	ln target/x86_64-apple-darwin/debug/stream-logs-to-s3 stream-logs-to-s3-${VERSION}-macos-x86_64-debug
	ln target/x86_64-apple-darwin/release/stream-logs-to-s3 stream-logs-to-s3-${VERSION}-macos-x86_64-release
	rm -f stream-logs-to-s3-${VERSION}-macos-x86_64.zip
	zip -9 stream-logs-to-s3-${VERSION}-macos-x86_64.zip stream-logs-to-s3-${VERSION}-macos-x86_64-debug stream-logs-to-s3-${VERSION}-macos-x86_64-release
	rm -f stream-logs-to-s3-${VERSION}-macos-x86_64-debug stream-logs-to-s3-${VERSION}-macos-x86_64-release

stream-logs-to-s3-${VERSION}-macos-aarch64.zip: ${SRCS} Cargo.toml Cargo.lock
	@if [[ "$(shell uname -s)" != "Darwin" ]]; then echo "You must run this on a Mac to build the Mac target: $(shell uname -s)" 1>&2; exit 1; fi
	cargo build --target aarch64-apple-darwin
	cargo build --target aarch64-apple-darwin --release
	rm -f stream-logs-to-s3-${VERSION}-macos-aarch64-debug stream-logs-to-s3-${VERSION}-macos-aarch64-release
	ln target/aarch64-apple-darwin/debug/stream-logs-to-s3 stream-logs-to-s3-${VERSION}-macos-aarch64-debug
	ln target/aarch64-apple-darwin/release/stream-logs-to-s3 stream-logs-to-s3-${VERSION}-macos-aarch64-release
	rm -f stream-logs-to-s3-${VERSION}-macos-aarch64.zip
	zip -9 stream-logs-to-s3-${VERSION}-macos-aarch64.zip stream-logs-to-s3-${VERSION}-macos-aarch64-debug stream-logs-to-s3-${VERSION}-macos-aarch64-release
	rm -f stream-logs-to-s3-${VERSION}-macos-aarch64-debug stream-logs-to-s3-${VERSION}-macos-aarch64-release

upload:
	mkdir upload-temp && cd upload-temp && for file in ../stream-logs-to-s3-${VERSION}-*.zip; do unzip $${file}; done && aws --profile iono s3 sync . s3://dist.ionosphere.io/bin/ && cd .. && rm -rf upload-temp

.PHONY: all clean macos
