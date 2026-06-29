# Release Runbook

Rototo releases use one canonical SemVer version, for example
`0.1.0-alpha.6`. The same `v<version>` tag publishes the Rust crate and every
supported SDK package for that version.

## Prepare

Start from a clean branch off `main`.

```sh
git checkout main
git pull --ff-only origin main
git checkout -b release/v0.1.0-alpha.6
just release-prep 0.1.0-alpha.6
```

`just release-prep` updates the canonical version surfaces, regenerates package
README content, runs `just release-check`, and runs `just check`.

Review the diff before opening the release PR. The expected files are version
manifests, generated package READMEs, lockfiles, and the changelog entry if one
was added manually.

## Check

Before tagging, run the local release gate explicitly:

```sh
just release-check 0.1.0-alpha.6
just release-package-dry-run 0.1.0-alpha.6
```

`release-check` verifies:

- canonical SemVer spelling;
- Cargo package version;
- SDK package manifests;
- generated package README freshness;
- release artifact manifest generation.

`release-package-dry-run` verifies local package shapes where the local tooling
is available. It also prints registry links for post-publish smoke checks.

## Tag

After the release PR lands on `main`, tag exactly the merged commit:

```sh
git checkout main
git pull --ff-only origin main
git tag v0.1.0-alpha.6
git push origin v0.1.0-alpha.6
```

The tag starts `.github/workflows/release.yml`.

## Publish

The release workflow validates the version, runs the release gate, builds SDK
artifacts, and publishes to the configured registries. Required secrets:

- `CENTRAL_USERNAME`
- `CENTRAL_PASSWORD`
- `MAVEN_GPG_PRIVATE_KEY`
- `MAVEN_GPG_PASSPHRASE`

PyPI and crates.io publish through their GitHub Actions authentication flows.
npm publish uses the workflow's npm registry configuration.

## Verify

After publish, check each registry page:

- crates.io
- PyPI
- npm
- Maven Central
- Go module page
- docs.rs for the Rust crate
- public docs site after the Cloudflare Pages workflow finishes

## Failed Publish

If validation fails before any publish step, fix the branch and tag a new commit.
Delete and recreate the tag only if no package was published.

If one registry published and a later registry failed, do not reuse the same
version with different bits. Fix the release workflow or package metadata, then
publish the missing registry artifact for the same tag only if the artifact
content already came from that tag. If package content must change, prepare a
new patch or alpha version.
