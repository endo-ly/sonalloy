#!/usr/bin/env bash
set -euo pipefail

REPO="endo-ly/sonalloy"
BIN_NAME="sonalloy"
API_URL="https://api.github.com/repos/${REPO}/releases/latest"

OS=""

log() {
  printf '%s\n' "$*"
}

err() {
  printf 'Error: %s\n' "$*" >&2
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1
}

print_help() {
  cat <<'EOF'
Usage: install-sonalloy.sh

Downloads the latest Sonalloy release and installs it to ~/.local/bin.
Run under Git Bash on Windows.

Options:
  -h, --help   Show this help.
EOF
}

parse_args() {
  case "${1:-}" in
    "")
      ;;
    -h|--help)
      print_help
      exit 0
      ;;
    *)
      err "Unknown argument: $1"
      print_help >&2
      exit 1
      ;;
  esac
}

detect_os() {
  case "$(uname -s)" in
    Darwin) OS="darwin" ;;
    Linux) OS="linux" ;;
    MINGW*|MSYS*) OS="windows" ;;
    *)
      err "Unsupported OS: $(uname -s)"
      exit 1
      ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo "x86_64" ;;
    arm64|aarch64) echo "aarch64" ;;
    *)
      err "Unsupported architecture: $(uname -m)"
      exit 1
      ;;
  esac
}

detect_install_dir() {
  if [ -d "$HOME/.local/bin" ] || mkdir -p "$HOME/.local/bin" 2>/dev/null; then
    echo "$HOME/.local/bin"
    return
  fi
  err "Could not create install directory: $HOME/.local/bin"
  exit 1
}

download_release_json() {
  if need_cmd curl; then
    curl -fsSL "$API_URL"
  elif need_cmd wget; then
    wget -qO- "$API_URL"
  else
    err "Neither curl nor wget is available"
    exit 1
  fi
}

extract_asset_url() {
  local release_json="$1"
  local arch="$2"
  local os_regex arch_regex

  case "$OS" in
    darwin) os_regex="apple-darwin|darwin" ;;
    linux) os_regex="unknown-linux-gnu|linux" ;;
    windows) os_regex="pc-windows-msvc|windows" ;;
    *)
      err "Unsupported OS for release matching: $OS"
      return 1
      ;;
  esac

  case "$arch" in
    x86_64) arch_regex="x86_64|amd64" ;;
    aarch64) arch_regex="aarch64|arm64" ;;
    *)
      err "Unsupported architecture for release matching: $arch"
      return 1
      ;;
  esac

  printf '%s\n' "$release_json" \
    | grep -Eo 'https://[^"]+' \
    | grep '/releases/download/' \
    | grep -E "/${BIN_NAME}-[0-9]+\.[0-9]+\.[0-9]+-.*\.tar\.gz$" \
    | grep -Ei "(${arch_regex}).*(${os_regex})|(${os_regex}).*(${arch_regex})" \
    | head -n1
}

download_file() {
  local url="$1"
  local output="$2"
  if need_cmd curl; then
    curl -fL "$url" -o "$output"
  else
    wget -O "$output" "$url"
  fi
}

bin_filename() {
  if [ "$OS" = "windows" ]; then
    echo "${BIN_NAME}.exe"
  else
    echo "$BIN_NAME"
  fi
}

install_binary() {
  local archive="$1"
  local install_dir="$2"
  local tmpdir="$3"
  local bin_name bin_path target_path tmp_target

  tar -xzf "$archive" -C "$tmpdir"

  bin_name="$(bin_filename)"
  bin_path="$(find "$tmpdir" -type f -name "$bin_name" | head -n1)"
  if [ -z "$bin_path" ]; then
    err "Could not find '$bin_name' in archive"
    return 1
  fi

  chmod +x "$bin_path"
  if [ ! -w "$install_dir" ]; then
    err "No write permission for $install_dir"
    return 1
  fi

  target_path="$install_dir/$bin_name"
  tmp_target="$install_dir/.${BIN_NAME}.tmp.$$"
  cp "$bin_path" "$tmp_target"
  chmod +x "$tmp_target"
  mv -f "$tmp_target" "$target_path"
}

main() {
  local arch install_dir release_json asset_url tmpdir archive asset_filename

  parse_args "$@"

  detect_os
  arch="$(detect_arch)"
  install_dir="$(detect_install_dir)"

  log "Installing ${BIN_NAME} for ${OS}/${arch}..."
  release_json="$(download_release_json)"
  asset_url="$(extract_asset_url "$release_json" "$arch" || true)"
  if [ -z "$asset_url" ]; then
    err "No prebuilt binary found for ${OS}/${arch} in the latest GitHub release."
    err "Build from source instead (see README):"
    err "  cargo build --workspace --release"
    err "  Releases: https://github.com/${REPO}/releases"
    exit 1
  fi

  tmpdir="$(mktemp -d)"
  trap 'if [ -n "${tmpdir:-}" ]; then rm -rf "$tmpdir"; fi' EXIT
  asset_filename="${asset_url##*/}"
  asset_filename="${asset_filename%%\?*}"
  if [ -z "$asset_filename" ] || [ "$asset_filename" = "$asset_url" ]; then
    asset_filename="${BIN_NAME}.tar.gz"
  fi
  archive="$tmpdir/$asset_filename"
  log "Downloading: $asset_url"
  download_file "$asset_url" "$archive"
  install_binary "$archive" "$install_dir" "$tmpdir"

  log ""
  log "Installed ${BIN_NAME} to ${install_dir}."
  log "Verify: ${install_dir}/$(bin_filename) --version"
  "${install_dir}/$(bin_filename)" --version

  if [ "$OS" != "windows" ] && ! need_cmd "$BIN_NAME"; then
    log ""
    log "Add this directory to PATH:"
    log "  export PATH=\"$install_dir:\$PATH\""
  fi

  log ""
  log "Usage examples:"
  log "  sonalloy instrument validate <instrument.json>"
  log "  sonalloy render midi <instrument.json> <input.mid> --output out.wav"
  log "  sonalloy dev render-sine --frequency 440 --duration 1.0 --output out.wav"
  log "See https://github.com/${REPO} for documentation."
}

main "$@"