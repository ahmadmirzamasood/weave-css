# Releasing Weave

This is the maintainers' procedure for turning a release candidate commit into a published release, and for undoing a bad one. Nothing in the repository publishes anything by itself: the workflow in `.github/workflows/release.yml` builds, tests, packages and, only when asked, attests and creates a **draft** GitHub release. Publishing the draft, creating the tag and publishing to crates.io are separate manual steps that need a maintainer's credentials.

## What a release consists of

| Artifact | Produced by | Contents |
|---|---|---|
| `weavec-<version>-x86_64-unknown-linux-gnu.tar.gz` | release workflow, `ubuntu-latest` | `weavec`, LICENSE, THIRD-PARTY-NOTICES.md, README.md |
| `weavec-<version>-aarch64-apple-darwin.tar.gz` | release workflow, `macos-latest` | same |
| `weavec-<version>-x86_64-pc-windows-msvc.zip` | release workflow, `windows-latest` | `weavec.exe` and the same files |
| `weave-css-<version>-source.tar.gz` | `git archive` of the release commit | complete source, tests, examples, docs |
| `weave-syntax-<version>.vsix` | release workflow, `scripts/package_vscode.py` | the VS Code syntax extension for `.weave` files (highlighting only), for `code --install-extension` |
| `SHA256SUMS` | release workflow | SHA-256 of every file above |
| build-provenance attestation | `actions/attest-build-provenance` (draft-release job only) | signed statement over `SHA256SUMS` subjects |
| crates `weave-compiler`, `weave-css` | `cargo publish` by a maintainer | `src/`, README, LICENSE, manifest |

Only the three platforms CI tests get binaries. `weave-bench` and `weave-docs` are never published.

## Pre-flight (before the workflow)

All of these run in CI on the pull request; run them locally for the final commit as well.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo +1.85.0 test --workspace --all-features --locked        # MSRV
python3 scripts/check_spec.py && python3 scripts/check_deps.py \
  && python3 scripts/check_editor.py && python3 scripts/check_release.py
cargo package --workspace --locked --exclude weave-bench --exclude weave-docs
```

Then check by hand:

1. `git status` is clean and `git rev-parse HEAD` is the commit you mean to release. The release is that commit; the workflow refuses to build any other when `expected_commit` is set.
2. The version in `Cargo.toml` is the intended one, the CHANGELOG section is dated/final and `docs/RELEASE-NOTES-<version>.md` is accurate. Replace statements in the notes that are no longer true.
3. The package names are still free. Names can be taken between preparation and publication:

   ```bash
   for n in weave-css weave-compiler; do
     printf '%s: ' $n; curl -s -o /dev/null -w '%{http_code}\n' -A "weave-css release check" https://crates.io/api/v1/crates/$n   # 404 = free
   done
   ```

4. Open decisions listed in the release notes (the binary name above all) are settled.
5. Credentials exist and are narrow: a crates.io API token limited to publishing these two crate names (or trusted publishing, if configured), kept in the maintainer's secret store, never in the repository, a workflow or a log. The workflow needs no registry secret.
6. Optional but recommended: in the repository settings create the `release` environment and require a reviewer, so the draft-release job waits for a person.

## Reproducibility

What is verified, and what is not:

- The release workflow builds the Linux binary twice, in two jobs from clean checkouts of the same commit on the same runner image, with the same pinned toolchain (`rust-toolchain.toml`), `--locked`, and build paths remapped (`--remap-path-prefix`). The `Reproducibility (Linux)` job fails unless the two binaries have the same SHA-256. This shows that the build does not depend on the checkout location, time or randomness; it does not prove that a different machine or runner image gives the same bytes.
- Archives are deterministic given the binary: fixed order, owners, modes and timestamps (`SOURCE_DATE_EPOCH` set to the commit time) and a gzip header without a name or time.
- macOS and Windows binaries are built from the same pinned toolchain and `--locked` lockfile but are not claimed to be bit-for-bit reproducible.
- The compiler has no dependencies, so there are no registry crates in the binary other than the Rust standard library.

To reproduce a Linux release binary yourself, check out the release commit, install the toolchain from `rust-toolchain.toml`, and run the `Build` step of the workflow with the same `RUSTFLAGS`; compare the SHA-256 printed in the `binary-sha256-*.txt` artifact.

## Publication steps

Do these in order. Each can be stopped; none can be silently continued by CI.

1. **Merge the release candidate to `main`** by pull request after its required checks pass. Note the full commit SHA (`git rev-parse origin/main`). The commit that was tested must be the commit that is released: if anything changes, run the pre-flight again.
2. **Run the workflow.** Actions -> *Release candidate* -> *Run workflow* on `main`, with `expected_commit` = that SHA and `draft_release` = true. The jobs: verify (consistency checks, fmt, Clippy, tests, package dry run), build x3 with a smoke test of each binary (`weavec --version` must print `weave <version>` and every example must build to its `expected.css`), reproducibility, checksums, then attestation and a draft release. Nothing is public yet: the draft is visible only to maintainers, and no tag exists.
3. **Verify the draft** before publishing it. For each downloaded asset, in a clean directory (not the checkout):

   ```bash
   sha256sum --check SHA256SUMS            # macOS: shasum -a 256 --check SHA256SUMS
   gh attestation verify weavec-<version>-x86_64-unknown-linux-gnu.tar.gz --repo ahmadmirzamasood/weave-css
   tar xzf weavec-<version>-x86_64-unknown-linux-gnu.tar.gz
   ./weavec-<version>-x86_64-unknown-linux-gnu/weavec --version          # first line: weavec <version>
   printf 'bind c = #0b5fff;\nrule .a { color: #{c}; }\n' | ./weavec-<version>-x86_64-unknown-linux-gnu/weavec build -
   ```

   Do the same on macOS and Windows machines, or accept the workflow's smoke test as the evidence and say so.
4. **Dry-run the crates**, then publish them in dependency order. Publishing a version is permanent (it can be yanked, never deleted or replaced).

   ```bash
   cargo publish --workspace --dry-run --locked --exclude weave-bench --exclude weave-docs
   cargo publish -p weave-compiler --locked
   # wait until `cargo search weave-compiler` / the index shows the version, then:
   cargo publish -p weave-css --locked
   ```

   The first publication claims the crate names. Do it from a clean checkout of the release commit (cargo records the commit in `.cargo_vcs_info.json` and refuses a dirty tree).
5. **Publish the draft release.** In the GitHub UI open the draft, check the notes and assets once more, and publish. GitHub then creates the tag `v<version>` at the target commit. Confirm `git ls-remote --tags origin v<version>` shows the release SHA. A release candidate is marked *pre-release*.
6. **Verify the published artifacts from the outside.** In a clean container or VM:

   ```bash
   cargo install weave-css --version <version> --locked
   weavec --version
   # download a release asset, check SHA256SUMS and `gh attestation verify`, run it on a fixture
   ```

7. **Update the repository.** In a follow-up pull request change the "not published yet" wording in README.md and docs/INSTALL.md, add the release date to the CHANGELOG, and update the release notes' status. Do not change the tag or the published assets.

## Rollback and incident handling

- **Draft not yet published** (including a failed workflow): delete the draft release in the UI and fix forward. No tag exists, nothing needs to be undone.
- **Crate published but release not**: you cannot delete a version. If it is wrong, `cargo yank --version <version> weave-css` (and `weave-compiler`) and publish a fixed version. Yanking stops new dependency resolution to it; it does not remove copies already downloaded or locked.
- **Release published and wrong**: never move or delete the tag, and never replace assets under the same version. Edit the release notes to say what is wrong, mark the release as superseded, publish the fixed version (`rc.2` or a patch) and yank the registry versions where justified.
- **Security problem**: publish a GitHub security advisory (CVE if warranted) with affected and fixed versions, a fix release, and yanks. Credit the reporter if they wish. Rotate any credential that may have leaked and review the release workflow run.
- **Always**: keep a timeline (when found, who decided what, which versions affected, when fixed) in the advisory or an issue, and add a regression test for the cause before closing.
