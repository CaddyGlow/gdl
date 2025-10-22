# gdl (GitHub Downloader)

A fast way to download files or directories from GitHub repositories—paste a GitHub URL and grab what you need without cloning the whole project. Perfect for quickly fetching examples, demos, config files, or specific folders.

## Features
- Optimized for copy/paste workflows: drop in a `tree` or `blob` URL while browsing GitHub and fetch the content instantly.
- Fetches either a single file or entire directory trees while preserving the repository structure locally.
- Intelligent download strategies: automatically selects the fastest method (git sparse checkout, zip archive, or REST API) based on availability and request type.
- Smart overwrite protection: prompts before overwriting existing files in interactive mode, fails safely in non-interactive environments.
- Supports authenticated requests via personal access tokens for private repositories or higher rate limits.
- HTTP response caching and download resume: speeds up repeated requests and recovers from interrupted downloads.
- Automatically chooses a sensible default output directory and prevents path traversal outside the target folder.
- Emits structured logs via `env_logger`, making it easy to inspect progress or troubleshoot failures.

## Installation

### Quick install (recommended)

Download and install the latest release using the install script:

**Linux/macOS:**
```bash
curl -fsSL https://raw.githubusercontent.com/CaddyGlow/ghdl/main/install.sh | bash
```

Or with options:
```bash
curl -fsSL https://raw.githubusercontent.com/CaddyGlow/ghdl/main/install.sh | bash -s -- --prefix ~/.local/bin
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/CaddyGlow/ghdl/main/install.ps1 | iex
```

The install scripts download pre-built binaries from GitHub releases. Available options:
- `--prefix DIR` – installation directory (default: `~/.local/bin` on Linux/macOS, `%USERPROFILE%\.local\bin` on Windows)
- `--tag TAG` – install a specific release tag instead of the latest
- `--token TOKEN` – GitHub token to avoid rate limits
- `--force` – overwrite existing installation without prompting

### Install from crates.io
```bash
cargo install ghdl
```

### Install from Git
```bash
cargo install --git https://github.com/CaddyGlow/ghdl
```

### Build from a local checkout
```bash
git clone https://github.com/CaddyGlow/ghdl.git
cd ghdl
cargo install --path .
```

Alternatively, build the binary directly:
```bash
cargo build --release
# resulting binary at target/release/ghdl
```

## Usage

Pass one or more GitHub `tree` or `blob` URLs that include the branch name—just copy the address bar while browsing a folder or file.

```bash
gdl https://github.com/owner/repo/tree/main/path/to/dir
```

Optional flags:
- `-o, --output <path>` – destination directory for the downloaded files. When omitted, `ghdl` infers a directory based on the request (current directory for single files or the leaf folder name for directories). When multiple URLs are supplied, each download reuses the same output directory if this flag is specified.
- `-p, --parallel <N>` – maximum number of files to download concurrently (default: 4).
- `-s, --strategy <STRATEGY>` – preferred download strategy (default: `auto`):
  - `api` – use GitHub REST API exclusively
  - `git` – use git sparse checkout (requires git to be installed)
  - `zip` – download repository zip archive and extract specific files
  - `auto` – intelligent fallback strategy:
    - If git is available: tries git → zip → API
    - If git is not available:
      - For whole repository: tries zip → API
      - For specific paths: tries API → zip
- `-f, --force` – force overwrite existing files without prompting.
- `--token <token>` – GitHub personal access token. If not supplied, `ghdl` falls back to `GITHUB_TOKEN` or `GH_TOKEN` environment variables when present.
- `--api-rate` – display GitHub API rate limit information and exit.
- `--self-update` – replace the current `ghdl` binary with the latest GitHub release and exit. Honors `--token`/`GITHUB_TOKEN`/`GH_TOKEN` for private repositories.
- `--check-update` – report whether a newer release is available without downloading it.
- `--clear-cache` – clear all cached data and exit.
- `--no-cache` – disable HTTP response caching and download resume for this run.
- `-v, -vv, -vvv` – increase logging verbosity (info/debug/trace). Combine with `RUST_LOG` for fine-grained control.

### Examples

Download a single file to the current directory without opening the raw view:
```bash
gdl https://github.com/owner/repo/blob/main/path/file.yml
```

Download multiple paths in one invocation:
```bash
gdl https://github.com/owner/repo/blob/main/path/file.yml \
    https://github.com/owner/repo/tree/main/examples
```

Download an entire directory tree into `./examples`:
```bash
gdl https://github.com/owner/repo/tree/main/examples --output ./examples
```

Check for updates without downloading anything:
```bash
ghdl --check-update
```

Check your GitHub API rate limit:
```bash
ghdl --api-rate
```

Download from a private repository using a token:
```bash
export GITHUB_TOKEN=ghp_your_personal_access_token
gdl https://github.com/owner/private-repo/tree/main/config
```

**Tip:** If you have the GitHub CLI (`gh`) installed, you can automatically use your authenticated token:
```bash
export GITHUB_TOKEN=$(gh auth token)
gdl https://github.com/owner/private-repo/tree/main/config
```

### Logging and debugging

Logging levels can be adjusted with `RUST_LOG`:
```bash
ghdl -v https://github.com/owner/repo/tree/main/src
RUST_LOG=trace ghdl -vv https://github.com/owner/repo/tree/main/src
```

## Development

Set up a Rust toolchain (Rust 1.75+ recommended) and run:
```bash
cargo fmt
cargo clippy --all-features -- -D warnings
cargo test
```

### Nix workflow

If you use Nix, the provided `flake.nix` offers a reproducible development environment:
```bash
nix develop
```

## Limitations
- Uses the GitHub REST API v3 and therefore inherits API rate limits. Authenticating with a token increases the allowance.
- Symlinks, submodules, and other non-file content types are currently skipped with a warning.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
