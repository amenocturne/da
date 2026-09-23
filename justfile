set positional-arguments

default:
    @just --list

check-model:
    scripts/check-model.sh

build *ARGS:
    cargo build --locked "$@"

test *ARGS:
    cargo test --locked "$@"

lint *ARGS:
    cargo clippy --all-targets --locked "$@" -- -D warnings

package TARGET BINARY OUTPUT="tmp/dist":
    scripts/package-release.sh "{{ TARGET }}" "{{ BINARY }}" "{{ OUTPUT }}"

smoke BINARY TARGET VERSION:
    scripts/smoke-release.sh "{{ BINARY }}" "{{ TARGET }}" "{{ VERSION }}"
