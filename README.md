# gdl

CLI for downloading files or directories from a GitHub repository using the GitHub REST API—paste a GitHub URL and grab what you need without cloning the whole project.

## Features
- Optimized for copy/paste workflows: drop in a `tree` or `blob` URL while browsing GitHub and fetch the content instantly.
- Fetches either a single file or entire directory trees while preserving the repository structure locally.
- Supports authenticated requests via personal access tokens for private repositories or higher rate limits.
- Automatically chooses a sensible default output directory and prevents path traversal outside the target folder.
- Emits structured logs via `env_logger`, making it easy to inspect progress or troubleshoot failures.

## Installation

### Quick install (recommended)

Download and install the latest release using the install script:

**Linux/macOS:**
```bash
curl -fsSL https://raw.githubusercontent.com/CaddyGlow/gdl/main/scripts/install.sh | bash
```

Or with options:
```bash
curl -fsSL https://raw.githubusercontent.com/CaddyGlow/gdl/main/scripts/install.sh | bash -s -- --prefix ~/.local/bin
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/CaddyGlow/gdl/main/scripts/install.ps1 | iex
```

The install scripts download pre-built binaries from GitHub releases. Available options:
- `--prefix DIR` – installation directory (default: `~/.local/bin` on Linux/macOS, `%USERPROFILE%\.local\bin` on Windows)
- `--tag TAG` – install a specific release tag instead of the latest
- `--token TOKEN` – GitHub token to avoid rate limits
- `--force` – overwrite existing installation without prompting

### Install from Git
```bash
cargo install --git https://github.com/CaddyGlow/gdl gdl
```

### Build from a local checkout
```bash
git clone https://github.com/CaddyGlow/gdl.git
cd gdl
cargo install --path .
```

Alternatively, build the binary directly:
```bash
cargo build --release
# resulting binary at target/release/gdl
```

## Usage

The `--url` flag is required and must point to a GitHub `tree` or `blob` URL that includes the branch name—just copy the address bar while browsing a folder or file.

```bash
gdl --url https://github.com/owner/repo/tree/main/path/to/dir
```

Optional flags:
- `--output <path>` – destination directory for the downloaded files. When omitted, `gdl` infers a directory based on the request (current directory for single files or the leaf folder name for directories).
- `--self-update` – replace the current `gdl` binary with the latest GitHub release and exit. Honors `--token`/`GITHUB_TOKEN`/`GH_TOKEN` for private repositories.
- `--check-update` – report whether a newer release is available without downloading it.
- `--token <token>` – GitHub personal access token. If not supplied, `gdl` falls back to `GITHUB_TOKEN` or `GH_TOKEN` environment variables when present.

### Examples

Download a single file to the current directory without opening the raw view:
```bash
gdl --url https://github.com/owner/repo/blob/main/path/file.yml
```

Download an entire directory tree into `./examples`:
```bash
gdl --url https://github.com/owner/repo/tree/main/examples --output ./examples
```

Check for updates without downloading anything:
```bash
gdl --check-update
```

Download from a private repository using a token:
```bash
export GITHUB_TOKEN=ghp_your_personal_access_token
gdl --url https://github.com/owner/private-repo/tree/main/config
```

### Logging and debugging

Logging levels can be adjusted with `RUST_LOG`:
```bash
RUST_LOG=debug gdl --url https://github.com/owner/repo/tree/main/src
```

## Development

Set up a Rust toolchain (Rust 1.75+ recommended) and run:
```bash
cargo fmt
cargo clippy --all-features -- -D warnings
cargo test
```

### Cutting a release

Follow the step-by-step guide in [`docs/release.md`](docs/release.md) to prepare and publish a new tagged release.

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
