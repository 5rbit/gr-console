set shell := ["powershell", "-NoProfile", "-Command"]

check:
    cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

verify: check test

web-install:
    cd apps/gr-web; npm install

web-dev:
    cd apps/gr-web; npm run dev

web-check:
    cd apps/gr-web; npm run check; npm run test:unit

web-build:
    cd apps/gr-web; npm run build

dev:
    cargo run -p gr-console -- --demo

console: web-build
    $env:GR_CONSOLE_WEB_DIR = "$PWD/apps/gr-web/dist"; cargo run -p gr-console
