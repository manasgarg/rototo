# Release process

rototo publishes the Rust crate to crates.io from GitHub Actions.

## One-time setup

1. Publish the first crate version manually from a clean `main` checkout:

   ```sh
   just check
   cargo publish --dry-run --locked
   cargo publish --locked
   ```

2. In the crates.io crate settings, configure Trusted Publishing for:

   - repository: `manasgarg/rototo`
   - workflow: `.github/workflows/release.yml`
   - environment: `release`

3. In GitHub, create a protected environment named `release`.

4. In Cloudflare, create a Pages project named `rototo-docs`.

   Use Direct Upload rather than Cloudflare Git integration. GitHub Actions will
   build the static site with `rototo docs export` and upload the generated
   `site` directory with Wrangler.

5. In Cloudflare Pages, add the custom domain:

   ```text
   docs.rototo.pirogram.com
   ```

6. Configure DNS for `docs.rototo.pirogram.com` according to Cloudflare Pages'
   custom-domain instructions.

7. Create a Cloudflare API token that can deploy to the Pages project.

   The token needs `Account` > `Cloudflare Pages` > `Edit` permission.

8. In GitHub repository secrets, add:

   - `CLOUDFLARE_ACCOUNT_ID`
   - `CLOUDFLARE_API_TOKEN`

9. In GitHub, create an environment named `cloudflare-pages`.

## Release checklist

1. Update `CHANGELOG.md`.
2. Bump `version` in `Cargo.toml`.
3. Run `just check`.
4. Run `cargo publish --dry-run --locked`.
5. Merge the release changes to `main`.
6. Create and push a tag matching the Cargo version:

   ```sh
   git tag v0.1.0-alpha.1
   git push origin v0.1.0-alpha.1
   ```

7. Approve the `release` environment deployment in GitHub Actions.
8. Confirm the new version appears on crates.io.

Published crates.io versions are immutable. If a release has a serious problem,
yank that version and publish a new patch version.
