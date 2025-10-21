#!/usr/bin/env bash
set -euo pipefail

OWNER="CaddyGlow"
REPO="gdl"
API_URL="https://api.github.com/repos/${OWNER}/${REPO}"
BINARY_NAME="gdl"
ARCHIVE_EXT="tar.gz"

usage() {
  cat <<'EOF'
Usage: install.sh [--prefix DIR] [--tag TAG] [--token TOKEN] [--force]

Downloads the latest gdl release from GitHub and installs the binary for the
current platform. By default the binary is placed in ~/.local/bin.

Options:
  --prefix DIR   Installation directory (default: ~/.local/bin or $INSTALL_PREFIX)
  --tag TAG      Install a specific release tag instead of the latest
  --token TOKEN  GitHub token to avoid rate limits (falls back to $GITHUB_TOKEN or $GH_TOKEN)
  --force        Overwrite an existing installation without prompting (alias: --froce)
  --help         Show this message
EOF
}

parse_args() {
  prefix="${INSTALL_PREFIX:-"$HOME/.local/bin"}"
  tag=""
  token="${TOKEN:-}"
  force=0

  while [[ $# -gt 0 ]]; do
    case "$1" in
    --prefix)
      shift
      if [[ $# -eq 0 ]]; then
        echo "error: --prefix requires a directory argument" >&2
        exit 1
      fi
      prefix="$1"
      ;;
    --tag)
      shift
      if [[ $# -eq 0 ]]; then
        echo "error: --tag requires a tag name argument" >&2
        exit 1
      fi
      tag="$1"
      ;;
    --token)
      shift
      if [[ $# -eq 0 ]]; then
        echo "error: --token requires a token value" >&2
        exit 1
      fi
      token="$1"
      ;;
    --force|--froce)
      force=1
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown option '$1'" >&2
      usage >&2
      exit 1
      ;;
    esac
    shift
  done

  if [[ -z "${token}" ]]; then
    if [[ -n "${GITHUB_TOKEN:-}" ]]; then
      token="${GITHUB_TOKEN}"
    elif [[ -n "${GH_TOKEN:-}" ]]; then
      token="${GH_TOKEN}"
    fi
  fi
}

require_cmds() {
  local missing=0
  for cmd in "$@"; do
    if ! command -v "${cmd}" >/dev/null 2>&1; then
      echo "error: ${cmd} is required" >&2
      missing=1
    fi
  done
  if ((missing)); then
    exit 1
  fi
}

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "${os}" in
  Linux)
    # Check if running on Android
    if [[ -n "${ANDROID_ROOT:-}" ]] || [[ -f /system/build.prop ]] || [[ -f /system/bin/app_process ]]; then
      case "${arch}" in
      aarch64|arm64) echo "aarch64-linux-android" ;;
      *)
        echo "error: unsupported architecture '${arch}' on Android" >&2
        return 1
        ;;
      esac
    else
      case "${arch}" in
      x86_64) echo "x86_64-unknown-linux-gnu" ;;
      aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
      *)
        echo "error: unsupported architecture '${arch}' on Linux" >&2
        return 1
        ;;
      esac
    fi
    ;;
  Darwin)
    case "${arch}" in
    x86_64) echo "x86_64-apple-darwin" ;;
    arm64) echo "aarch64-apple-darwin" ;;
    *)
      echo "error: unsupported architecture '${arch}' on macOS" >&2
      return 1
      ;;
    esac
    ;;
  *)
    echo "error: unsupported operating system '${os}'" >&2
    echo "Use the PowerShell installer for Windows instead." >&2
    return 1
    ;;
  esac
}

extract_tag() {
  local path tag
  path="$1"
  tag="$(sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' "${path}" | head -n1)"
  if [[ -z "${tag}" ]]; then
    echo "unknown"
  else
    echo "${tag}"
  fi
}

select_asset() {
  local path target extension
  path="$1"
  target="$2"
  extension="$3"

  awk -v target="${target}" -v extension="${extension}" '
    /"name":/ {
      if (index($0, target) && index($0, extension)) {
        if (match($0, /"name":[[:space:]]*"([^"]+)"/, m)) {
          name = m[1]
          in_asset = 1
        }
        next
      }
    }
    in_asset && /"browser_download_url":/ {
      if (match($0, /"browser_download_url":[[:space:]]*"([^"]+)"/, m)) {
        printf "%s|%s\n", m[1], name
        found = 1
        exit 0
      }
    }
    in_asset && /"name":/ {
      in_asset = 0
    }
    END {
      if (!found) {
        exit 1
      }
    }
  ' "${path}"
}

prompt_overwrite() {
  local current_line latest_tag existing_version prompt answer
  current_line="$1"
  latest_tag="$2"

  if [[ ! -t 0 ]]; then
    echo "error: refusing to overwrite existing installation without --force in non-interactive mode" >&2
    exit 1
  fi

  prompt="Overwrite existing installation? [y/N] "
  existing_version="${current_line##* }"
  if [[ -n "${existing_version}" && "${latest_tag}" != "unknown" ]]; then
    local existing_clean="${existing_version#v}"
    local latest_clean="${latest_tag#v}"
    if [[ "${existing_clean}" == "${latest_clean}" ]]; then
      prompt="Reinstall gdl ${latest_tag}? [y/N] "
    else
      prompt="Replace gdl ${existing_version} with ${latest_tag}? [y/N] "
    fi
  fi

  read -r -p "${prompt}" answer
  case "${answer}" in
  [yY]|[yY][eE][sS])
    echo "Continuing with reinstall."
    ;;
  *)
    echo "Aborting installation."
    exit 0
    ;;
  esac
}

check_existing_install() {
  local install_path latest_tag
  install_path="$1"
  latest_tag="$2"

  if [[ ! -x "${install_path}" ]]; then
    return 0
  fi

  echo "Found existing gdl at ${install_path}"
  local existing_output existing_line
  existing_output="$("${install_path}" --version 2>/dev/null || true)"
  existing_line="${existing_output%%$'\n'*}"
  if [[ -n "${existing_line}" ]]; then
    echo "Current version: ${existing_line}"
  else
    echo "Current version: unknown (could not determine)"
  fi
  if [[ "${latest_tag}" != "unknown" ]]; then
    echo "Latest release: ${latest_tag}"
  else
    echo "Latest release: unknown (tag not found)"
  fi

  if [[ "${force}" -eq 1 ]]; then
    echo "Overwriting existing installation (--force)."
    return 0
  fi

  prompt_overwrite "${existing_line}" "${latest_tag}"
}

download_and_install() {
  local url name tmp_dir install_path archive_path binary_path
  url="$1"
  name="$2"
  tmp_dir="$3"
  install_path="$4"

  archive_path="${tmp_dir}/${name}"
  echo "Downloading ${name}..."
  if ! "${curl_args[@]}" "${url}" -o "${archive_path}"; then
    echo "error: failed to download ${name}" >&2
    exit 1
  fi

  tar -xzf "${archive_path}" -C "${tmp_dir}"
  binary_path="${tmp_dir}/${BINARY_NAME}"
  if [[ ! -x "${binary_path}" ]]; then
    echo "error: extracted archive did not contain the ${BINARY_NAME} binary" >&2
    exit 1
  fi

  mkdir -p "${prefix}"
  cp "${binary_path}" "${install_path}"
  chmod +x "${install_path}"
}

main() {
  parse_args "$@"
  require_cmds curl tar

  local target install_path release_endpoint tmp_dir release_json target_version asset_info asset_url asset_name
  if ! target="$(detect_target)"; then
    exit 1
  fi

  if [[ -z "${tag}" ]]; then
    release_endpoint="${API_URL}/releases/latest"
  else
    release_endpoint="${API_URL}/releases/tags/${tag}"
  fi
  echo "Querying ${release_endpoint}"

  curl_args=(
    curl
    -fsSL
    -H "Accept: application/vnd.github+json"
    -H "X-GitHub-Api-Version: 2022-11-28"
  )
  if [[ -n "${token}" ]]; then
    curl_args+=(-H "Authorization: Bearer ${token}")
  fi

  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "${tmp_dir:-}"' EXIT

  release_json="${tmp_dir}/release.json"
  if ! "${curl_args[@]}" "${release_endpoint}" -o "${release_json}"; then
    echo "error: failed to query GitHub releases" >&2
    exit 1
  fi

  target_version="$(extract_tag "${release_json}")"
  echo "Latest release tag: ${target_version}"

  if ! asset_info="$(select_asset "${release_json}" "${target}" "${ARCHIVE_EXT}")"; then
    echo "error: failed to locate a release asset for target ${target}" >&2
    exit 1
  fi
  IFS='|' read -r asset_url asset_name <<<"${asset_info}"
  if [[ -z "${asset_url}" || -z "${asset_name}" ]]; then
    echo "error: failed to determine download URL from GitHub response" >&2
    exit 1
  fi
  echo "Selected asset ${asset_name}"

  install_path="${prefix}/${BINARY_NAME}"
  check_existing_install "${install_path}" "${target_version}"

  download_and_install "${asset_url}" "${asset_name}" "${tmp_dir}" "${install_path}"

  echo "Installed ${BINARY_NAME} to ${install_path}"
  if ! command -v "${BINARY_NAME}" >/dev/null 2>&1; then
    echo "Note: add ${prefix} to your PATH to invoke ${BINARY_NAME}" >&2
  fi
}

main "$@"
