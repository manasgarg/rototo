# Self-Hosting the Console

Reviewed configuration only helps if the people changing values actually go
through review. The console exists so that an on-call engineer flipping an
operational switch, or a product engineer adjusting a rollout, lands on a
draft branch and a pull request — without hand-editing TOML. This page covers
running that console yourself, because the console is not a hosted service:
it runs next to your repositories, with your credentials, and rototo.dev
never sees them.

The console ships inside the `rototo` binary. There is no separate server to
install, no Node runtime, and no database to operate — console state (which
repositories you registered, draft sessions, activity) lives in a small
SQLite file under its data directory, and everything that matters lives in
Git.

```sh
rototo console
```

That starts the console on `http://127.0.0.1:7686`, bound to localhost.

## Local mode: one engineer, one laptop

By default the console runs in local mode: no login screen, no user accounts.
It needs exactly one credential — a GitHub token — because reading private
repositories and committing to draft branches both go through the GitHub API
as you. The console looks for that token in order:

1. `--workspace-token` / `ROTOTO_WORKSPACE_TOKEN`, the same token surface
   every other rototo command uses;
2. a token stored by a previous device-flow sign-in in the console UI;
3. the GitHub CLI, via `gh auth token`.

If none of those produce a token, the console starts anyway and the UI walks
you through connecting one. Edits you publish are attributed to your GitHub
account, because they are made with your token — the console adds no machinery
on top of GitHub's own permissions.

Console state defaults to the platform data directory (for example
`~/.local/share/rototo/console`); point `ROTOTO_CONSOLE_DATA_DIR` or
`--data-dir` somewhere else to keep per-project state separate.

## Team mode: one console, GitHub sign-in

A shared console deployment changes one thing: identity. Instead of a single
ambient token, each user signs in with GitHub OAuth and the console keeps
their token encrypted at rest, scoped to their session. Authorization stays
where it already lives — GitHub repository permissions. A user who cannot
push to the repository cannot edit drafts through the console, and every pull
request is attributed to the person who made it, not to a shared bot.

Team mode turns on when OAuth credentials are configured:

```sh
GITHUB_CLIENT_ID=… \
GITHUB_CLIENT_SECRET=… \
ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY=base64:… \
ROTOTO_CONSOLE_PUBLIC_URL=https://console.internal.example.com \
rototo console --bind 127.0.0.1:7686
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

## Read-only mode: a console anyone can look at

A read-only console serves one workspace with no sign-in and rejects every
write. It is the right shape for demos and for "what is configured right
now?" dashboards on a team TV:

```sh
rototo console --read-only \
  --workspace https://api.github.com/repos/acme/config/tarball/main
```

Workspace sources here are the same sources every rototo command accepts —
local paths, `git+https://`, or archive URLs, with `#ref:subdir` selection.

## The boundary worth knowing

The console writes through the GitHub API: draft branches, file commits, and
pull requests. Reading workspaces works from any source rototo supports, but
editing currently requires the workspace to live on GitHub. If your
configuration repository lives elsewhere, the CLI and SDK workflows work
today; console editing for other git hosts is a deliberate, separate piece of
work rather than something half-supported.
