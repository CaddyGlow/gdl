# Release Process

Follow these steps to cut a new `gdl` release without version mismatches.

## Prerequisites

- Rust toolchain (cargo).
- [`cargo-edit`](https://github.com/killercup/cargo-edit) so `cargo set-version` is available.

## Step-by-step

1. Ensure you are on `main` and your working tree is clean.
2. Decide the new semantic version (e.g. `0.2.2`).
3. Run the release helper:

   ```sh
   ./scripts/prepare-release.sh 0.2.2
   ```

   The script updates `Cargo.toml`/`Cargo.lock`, runs fmt + tests, commits the change, and tags `v0.2.2`. The build guard in `build.rs` ensures the manifest version matches the tag before CI accepts the commit.

4. Push the changes and tag:

   ```sh
   git push origin main
   git push origin v0.2.2
   ```

5. Draft the GitHub release based on tag `v0.2.2` and publish artifacts if needed.

If the script fails at any stage, no commits or tags are created; fix the issue and rerun. Delete the `vX.Y.Z` tag locally (`git tag -d vX.Y.Z`) if you need to retry after a failure mid-way.
