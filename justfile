set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

default:
    @just --list

run *ARGS:
    cargo run -- {{ARGS}}

build:
    cargo build --release

check:
    cargo check --all-targets --all-features

lint:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings

fmt:
    cargo fmt --all

test:
    cargo test --all-features --no-fail-fast

fix:
    cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged
    cargo fmt --all

audit:
    cargo audit --deny warnings

deny:
    cargo deny check

ci: lint test audit

install-local: build
    cargo install --path . --force

uninstall-local:
    cargo uninstall monk

clean:
    cargo clean

doctor:
    cargo run -- doctor

daemon-run:
    cargo run -- daemon run

daemon-install:
    cargo run -- daemon install

release-dry version:
    cargo publish --dry-run --allow-dirty

package-deb:
    cargo deb

package-rpm:
    cargo generate-rpm
