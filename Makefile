VERSION := $(shell egrep '^version = ' Cargo.toml | sed -E -e 's/[^"]+"([^"]+)"/\1/')
SRCS = src/*.rs
LINUX_OS_SUFFIXES = amzn2023 musl
ARCH_SUFFIXES = aarch64 x86_64
LINUX_ZIP_FILES = $(foreach os,${LINUX_OS_SUFFIXES},$(foreach arch,${ARCH_SUFFIXES},stream-logs-to-s3-${VERSION}-${os}-${arch}.zip))
MACOS_ZIP_FILE = stream-logs-to-s3-${VERSION}-macos-aarch64.zip
ZIP_FILES = ${LINUX_ZIP_FILES} ${MACOS_ZIP_FILE}

all: ${ZIP_FILES}
macos: ${MACOS_ZIP_FILE}

clean:
	rm -f stream-logs-to-s3-*.zip

stream-logs-to-s3-${VERSION}-%-x86_64.zip: %.dockerfile ${SRCS} Cargo.toml Cargo.lock
	docker buildx build --platform=linux/amd64 --progress=plain --build-arg ARCH=x86_64 --tag stream-logs-to-s3:${VERSION}-$*-x86_64 -f $*.dockerfile .
	docker run --rm stream-logs-to-s3:${VERSION}-$*-x86_64 cat stream-logs-to-s3-${VERSION}-$*-x86_64.zip > stream-logs-to-s3-${VERSION}-$*-x86_64.zip

stream-logs-to-s3-${VERSION}-%-aarch64.zip: %.dockerfile ${SRCS} Cargo.toml Cargo.lock
	docker buildx build --platform=linux/arm64 --progress=plain --build-arg ARCH=aarch64 --tag stream-logs-to-s3:${VERSION}-$*-aarch64 -f $*.dockerfile .
	docker run --rm stream-logs-to-s3:${VERSION}-$*-aarch64 cat stream-logs-to-s3-${VERSION}-$*-aarch64.zip > stream-logs-to-s3-${VERSION}-$*-aarch64.zip

stream-logs-to-s3-${VERSION}-macos-aarch64.zip: ${SRCS} Cargo.toml Cargo.lock
	@if [[ "$(shell uname -s)" != "Darwin" ]]; then echo "You must run this on a Mac to build the Mac target: $(shell uname -s)" 1>&2; exit 1; fi
	cargo build --target aarch64-apple-darwin --release
	rm -f stream-logs-to-s3-${VERSION}-macos-aarch64-release
	ln target/aarch64-apple-darwin/release/stream-logs-to-s3 stream-logs-to-s3-${VERSION}-macos-aarch64-release
	rm -f stream-logs-to-s3-${VERSION}-macos-aarch64.zip
	zip -9 stream-logs-to-s3-${VERSION}-macos-aarch64.zip stream-logs-to-s3-${VERSION}-macos-aarch64-release
	rm -f stream-logs-to-s3-${VERSION}-macos-aarch64-release

upload:
	mkdir upload-temp && cd upload-temp && for file in ../stream-logs-to-s3-${VERSION}-*.zip; do unzip $${file}; done && aws --profile iono s3 sync . s3://dist.ionosphere.io/bin/ && cd .. && rm -rf upload-temp

.PHONY: all clean macos upload
