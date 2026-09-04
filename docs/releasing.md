# Releasing duallity

This guide defines the release operation for the `duallity` bridge crate,
native SDK, `@vinary-tree/duallity` JavaScript facade, and `duallity` Python
facade. The current candidate is `4.0.0-rc.6`.

## Immutable source graph

Create `v4.0.0-rc.6` from the reviewed `release/4.0.0-rc.6` branch commit,
not from the independently changing primary worktree. The tag must agree with
`release/version.json`, `Cargo.toml`, native metadata, and the npm and PyPI
manifests.
Validation checks out `llattice@v0.1.0` and the exact `v4.0.0-rc.6` tags for
interop, libdictenstein, liblevenshtein, and lling-llang.

The repository synchronizer owns every family entry in `Cargo.lock`. A
read-only invocation rejects stale entries, and all validation/package builds
use `--locked` without changing the reviewed lockfile.

The reviewed development manifest resolves exact-version siblings from the
coordinated source graph. Isolated Python wheel builders run
`scripts/prepare-registry-cargo-manifest.py --write` in their disposable
checkout. That tool first verifies every expected path and exact version, then
removes only the validated path keys and regenerates the lockfile against
crates.io. The Python source distribution embeds the same normalized manifest
and the exact duallity Rust sources, so it can build without a repository
checkout once the RC.6 family crates are public. Its filesystem-free self-test
is part of the release contract.

The tag creates an immutable source boundary but triggers no workflow. A
manual `validate-only` dispatch tests the FFI and composition contracts, runs
strict Clippy and binding tests, builds Linux x86-64 and ARM64, macOS ARM64,
and Windows x86-64 archives, relocation-tests installed CMake packages under
shared and static linkage, packs npm, builds and installed-wheel-tests the
four platform-specific Python wheels, validates the self-contained Python
source distribution, and creates a checksummed GitHub prerelease. Tag creation
does not authorize registry publication.

## Validate, then publish one registry

A manual dispatch must target the immutable tag. `validate-only` enables no
registry uploader; `npm`, `pypi`, and `crates-io` each enable only their
matching protected job.

The checksummed GitHub prerelease is also a repository mutation. Its
`github-release` environment requires an operator review and a `v*` tag policy;
it stores no secret and gates only the job-scoped `GITHUB_TOKEN`.

The RC.6 train starts from the canonical source tag recorded as
`publication.sourceTag` in `release/version.json`. The workflow grants
`id-token: write` only to the crates.io job, obtains a short-lived token with
`rust-lang/crates-io-auth-action@v1`, and revokes that token after the job. If
a workflow-only correction is required before a coordinate is published, use
the next positive `v4.0.0-rc.6-release.N` tag; never move an existing tag.

```bash
gh workflow run release-bindings.yml \
  --repo vinary-tree/duallity \
  --ref v4.0.0-rc.6 \
  -f registry=validate-only

gh workflow run release-bindings.yml \
  --repo vinary-tree/duallity \
  --ref v4.0.0-rc.6 \
  -f registry=npm

gh workflow run release-bindings.yml \
  --repo vinary-tree/duallity \
  --ref v4.0.0-rc.6 \
  -f registry=pypi
```

Use the same reviewed source ref with `registry=crates-io`. A corrective ref may
publish only while the exact coordinate remains absent; a public RC.6 artifact
must never be rebuilt or overwritten.

Because duallity is the top Rust bridge, publish its crate only after
libdictenstein, liblevenshtein, lling-llang, and interop resolve publicly.
Publish npm only after `@vinary-tree/vinary-tree-interop` and
`@vinary-tree/javascript-runtime` resolve at `4.0.0-rc.6`. npm uses trusted
publishing, provenance, `next`, and the protected `npm` environment. PyPI
similarly uses a trusted publisher, provenance, and the protected `pypi`
environment.

## Public-byte verification and recovery

Install the public npm tarball and each applicable public Python wheel in clean
environments. Exercise dictionary-resource intake, Levenshtein-WFST
construction, composition, arc traversal, iteration, and deterministic close.
Then move the npm package's `latest` tag to the verified RC, remove `bootstrap`,
and deprecate the immutable `0.0.0` reservation. PyPI has no mutable equivalent
of npm distribution tags: verify the candidate in place and publish a new
version for any correction.

Tags and published versions are immutable. Use `registry=validate-only` for a
safe rerun. If public bytes are wrong, repair the source and issue
the next unused candidate; never move a tag, overwrite a version, or broaden
a failed registry run.
