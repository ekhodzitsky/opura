#!/usr/bin/env bash
set -euo pipefail

if ! command -v rustup >/dev/null; then
  case "$(uname -m)" in
    x86_64) architecture=x86_64; checksum=20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c ;;
    aarch64) architecture=aarch64; checksum=e3853c5a252fca15252d07cb23a1bdd9377a8c6f3efa01531109281ae47f841c ;;
    *) echo "Unsupported rustup bootstrap architecture: $(uname -m)" >&2; exit 1 ;;
  esac
  installer_dir=$(mktemp -d)
  curl --fail --silent --show-error --location --retry 3 \
    "https://static.rust-lang.org/rustup/archive/1.28.2/${architecture}-unknown-linux-gnu/rustup-init" \
    --output "$installer_dir/rustup-init"
  echo "$checksum  $installer_dir/rustup-init" | sha256sum --check
  chmod +x "$installer_dir/rustup-init"
  "$installer_dir/rustup-init" -y --no-modify-path --default-toolchain none
  export PATH="$HOME/.cargo/bin:$PATH"
  echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
fi
rustup toolchain install 1.87.0 --profile minimal --component rustfmt --component clippy
