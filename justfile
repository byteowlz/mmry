set positional-arguments

# Display help
help:
    just -l

# format code
fmt:
    cargo fmt -- --config imports_granularity=Item

fix *args:
    cargo clippy --fix --all-features --tests --allow-dirty "$@"

clippy:
    cargo clippy --all-features --tests "$@"

install:
    rustup show active-toolchain
    cargo fetch

# Install CLI and TUI with an optional shared ORT feature
install-all:
    ./scripts/install-mmry.sh

# Run `cargo nextest` since it's faster than `cargo test`, though including
# --no-fail-fast is important to ensure all tests are run.
#
# Run `cargo install cargo-nextest` if you don't have it installed.
test:
    cargo nextest run --no-fail-fast

# Run HMLR benchmark tests (RAGAS-style quality tests)
bench-hmlr:
    cargo test -p mmry-core hmlr::benchmarks --release -- --nocapture

# Run all benchmark tests with verbose output
bench-all:
    cargo test -p mmry-core benchmarks --release -- --nocapture
