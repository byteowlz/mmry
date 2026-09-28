set positional-arguments

# Display help
help:
    just -l

# format code
fmt:
    cargo fmt -- --config imports_granularity=Item

fix *args:
    cargo clippy --fix --all-features --tests --allow-dirty "$@"

# Alias for lint
clippy: lint

install:
    rustup show active-toolchain
    cargo fetch

# Build and install the mmry CLI (non-interactive)
install-all:
    ./scripts/install-mmry.sh

# Debug build (all crates)
build:
    cargo build --workspace

# Fast compile check
check:
    cargo check --workspace --all-targets

# Check formatting
fmt-check:
    cargo fmt --all -- --check

# Clippy with the strict workspace lint tables; warnings are errors
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run ast-grep guardrails (unwrap/expect, dbg/todo, clippy allows) on Rust sources
lint-rust-ai-guardrails:
    ast-grep scan --config .ast-grep/sgconfig.yml

# Build rustdoc with rustdoc lints denied
docs:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Run Rust tests (includes example config/schema drift checks) and script tests
test:
    cargo test --workspace --all-features --no-fail-fast
    python3 -m unittest discover -s scripts -p 'test_*.py'

# Regenerate examples/config.schema.json from the typed config model
generate-config:
    cargo run -p mmry-core --example generate_config

# Verify examples/ config files are current
validate-config:
    cargo test -p mmry-core example_

# Run every gate enforced by CI
check-all: fmt-check lint lint-rust-ai-guardrails docs test
