#!/bin/sh
# Installs the latest timeout release into ~/.local/bin on Linux or macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/wolfeidau/timeout/main/install.sh | sh

set -eu

REPO="https://github.com/wolfeidau/timeout"

fail() {
    echo "install.sh: $*" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || fail "$1 is required"
}

# Prints the release tag that /releases/latest redirects to.
latest_tag() {
    if command -v curl >/dev/null 2>&1; then
        url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$REPO/releases/latest")
    else
        url=$(wget -S --spider -O /dev/null "$REPO/releases/latest" 2>&1 |
            sed -n 's/^ *[Ll]ocation: *//p' | tail -n 1 | tr -d '\r')
    fi
    echo "${url##*/}"
}

download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$2" "$1"
    else
        wget -q -O "$2" "$1"
    fi
}

verify() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -c - >/dev/null
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 -c - >/dev/null
    else
        fail "sha256sum or shasum is required"
    fi
}

main() {
    case "$(uname -s)" in
    Linux) os=Linux ;;
    Darwin) os=Darwin ;;
    *) fail "unsupported OS: $(uname -s)" ;;
    esac
    case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=arm64 ;;
    *) fail "unsupported architecture: $(uname -m)" ;;
    esac

    command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1 ||
        fail "curl or wget is required"
    need tar

    tag=$(latest_tag) || fail "could not find the latest release"
    case "$tag" in
    v[0-9]*) ;;
    *) fail "could not find the latest release" ;;
    esac
    version=${tag#v}
    archive="timeout_${os}_${arch}.tar.gz"
    checksums="timeout_${version}_checksums.txt"

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    cd "$tmp"

    echo "Downloading timeout $version for $os $arch"
    download "$REPO/releases/download/$tag/$archive" "$archive"
    download "$REPO/releases/download/$tag/$checksums" "$checksums"
    grep -F "  $archive" "$checksums" | verify || fail "checksum verification failed"
    tar xzf "$archive" timeout

    bin_dir="$HOME/.local/bin"
    mkdir -p "$bin_dir"
    install -m 0755 timeout "$bin_dir/timeout"
    echo "Installed $bin_dir/timeout"

    case ":${PATH:-}:" in
    *":$bin_dir:"*) ;;
    *) echo "Add $bin_dir to your PATH to use it" ;;
    esac
}

main
