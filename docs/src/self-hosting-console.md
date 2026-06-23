# Self-Hosting the Console

Reviewed configuration only helps if the people changing values actually go
through review. The console exists so that an on-call engineer flipping an
operational switch, or a product engineer adjusting a rollout, lands on a
review branch and a pull request — without hand-editing TOML. This page covers
running that console yourself, because the console is not a hosted service:
it runs next to your repositories, with your credentials, and rototo.dev
never sees them.

The console ships inside the `rototo` binary. There is no separate server to
install, no Node runtime, and no database to operate — console state (which
source trees you registered, selected branches, activity) lives in a small
SQLite file under its data directory, and everything that matters lives in
Git.

```sh
rototo console --package examples/basic
```

That starts a local console on `http://127.0.0.1:7686`, bound to localhost,
with `examples/basic` registered as the fixed package.

## Local Deployment: One Engineer, One Laptop

When `--package` is present and `--deployment` is omitted, the console runs as
a local deployment: no login screen, no user database, and no requirement that
every package live on GitHub. Local folder packages can be read from disk
with the identity already present in the git checkout (`user.name` and
`user.email`). That keeps the laptop workflow close to normal git: if you are
working in a local clone, rototo does not make you authenticate to GitHub just
to inspect files.

You can also start local deployment without a fixed package:

```sh
rototo console --deployment local
```

A GitHub token still matters when the package source or write path needs
GitHub. Private GitHub repositories need credentials to read, pull-request
writes need credentials to create branches and PRs, and GitHub direct-push
writes need credentials to commit to the target branch. In local deployment the
console looks for that token in order:

1. `--package-token` / `ROTOTO_PACKAGE_TOKEN`, the same token surface
   every other rototo command uses;
2. a token stored by a previous device-flow sign-in in the console UI;
3. the GitHub CLI, via `gh auth token`.

If none of those produce a token, the console starts anyway. Local folder
packages still work; GitHub operations remain unavailable until a token is
present. Edits made through GitHub are attributed to your GitHub account,
because they are made with your token.

Console state defaults to the platform data directory (for example
`~/.local/share/rototo/console`); point `ROTOTO_CONSOLE_DATA_DIR` or
`--data-dir` somewhere else to keep per-project state separate.

## Hosted Deployment: One Console, GitHub Sign-In

A shared console deployment changes how identity and credentials are
established. Instead of deriving them from a laptop environment, each user
signs in with GitHub OAuth and the console keeps their token encrypted at rest,
scoped to their session. Authorization stays where it already lives: GitHub
repository permissions. A user who cannot push to the repository cannot edit
branches through the console, and every pull request is attributed to the person
who made it, not to a shared bot.

Hosted deployment is selected with `--deployment hosted`, or by omitting both
`--deployment` and `--package`. The OAuth credentials do not choose the
deployment mode; they are required configuration after hosted mode has been
selected:

```sh
ROTOTO_GITHUB_CLIENT_ID=… \
ROTOTO_GITHUB_CLIENT_SECRET=… \
ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY=base64:… \
ROTOTO_CONSOLE_PUBLIC_URL=https://console.internal.example.com \
rototo console --deployment hosted --bind 127.0.0.1:7686
```

- Register a GitHub OAuth App for your deployment with the callback URL
  `https://<your-host>/api/auth/github/callback`, and put its client id and
  secret in the environment.
- `ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY` must decode to 32 bytes (`base64:`,
  `hex:`, or raw base64). It encrypts stored GitHub tokens so a copied
  database file is not a credential leak. Generate one with
  `openssl rand -base64 32`.
- `ROTOTO_CONSOLE_PUBLIC_URL` is the origin users reach the console on; it
  drives OAuth redirects and cookie security.

The console serves plain HTTP and binds to localhost on purpose. Put your
reverse proxy in front of it for TLS and network exposure, the same way you
would for Prometheus or any other internal tool. The console does not try to
be your TLS terminator or your VPN.

## Fixed Package and Write Policy

Deployment is selected before the package source and write policy are applied.
`--package` fixes the console to one package source and defaults deployment
to local unless you pass `--deployment hosted`. `--write` says what the console
is allowed to do with that source:

```sh
rototo console --package examples/basic --write disabled
rototo console --package git+https://github.com/acme/config.git#main --write direct-push
rototo console \
  --package https://api.github.com/repos/acme/config/tarball/main \
  --write pull-request
```

Package sources here are the same sources every rototo command accepts —
local paths, `git+https://`, or archive URLs, with `#ref:subdir` selection.

- `--write disabled` turns the console into an inspection surface. It can load
  and lint the package, but branch edits and publishing are rejected.
- `--write pull-request` creates review branches and pull requests for GitHub
  packages.
- `--write direct-push` commits directly to the configured GitHub ref, or
  writes local folder packages directly to the local working tree.

## The boundary worth knowing

Reading packages works from any source rototo supports. Editing is narrower:
the console can write GitHub packages through the GitHub API, and local
deployments can edit local folder packages in the current working tree when
`--write direct-push` is set. Generic git remotes and archive sources are
read-only in the console. Other write backends are a deliberate, separate piece
of work rather than something half-supported behind a generic mode flag.
