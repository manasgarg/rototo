# The console

Most of rototo is built for machines: the CLI runs in a terminal or CI, and
the SDK resolves variables inside your services. The console is the piece
built for people. It is a web app for the humans who review and edit
configuration: browse a package, see how a variable resolves against a saved
context, edit a definition directly with the same lint the CLI runs marking
problems as you type, and open a pull request with the resolution impact
attached before anyone merges it.

It does not change the model. Git is still the source of truth, and every
edit still lands as a normal change on a branch, reviewed in a pull request.
The console is a lens over that, plus a safer way to make the edit. Nothing
you do in the console bypasses review: every write becomes a change set, a
branch and a PR through the GitHub API, the same way you would do it by hand.
In team mode a change set can also carry collaborators: add a teammate by
their GitHub login and they edit the same branch with you, as themselves.

The console ships separately from the CLI as its own npm package,
`@rototo/console`. It is a Node server that serves both a JSON API and the
web app from one process, and it reaches the same Rust core the CLI and SDKs
use, so what you see in the console resolves exactly the way your services
will.

## Running it

You need Node 24 or newer. That is the only prerequisite.

```sh
npx @rototo/console
```

With nothing else set, the console starts in **local mode**: no login, bound
to `127.0.0.1:7687`, trusting your workstation's own GitHub credentials (an
ambient token in the environment, stored credentials, or `gh auth token`).
Open `http://127.0.0.1:7687` and you are looking at the same GitHub
repositories your `gh` login can see. This is the fast way to try it on your
own machine.

From a checkout of the repository, the same server runs straight from source:

```sh
just console-build     # build the web app and stage it for serving
just console-dev       # start the server on http://127.0.0.1:7687
```

`console-build` compiles the web bundle so the server has a UI to serve.
Without it, the server still runs, but as API only, which is a fine way to
put it behind a separate static host.

## Auth modes

The console resolves how people sign in **once, at startup**, from the
environment. There is no runtime toggle.

- **Local mode** is the default. It assumes one trusted operator on the
  machine and uses that person's GitHub token for everything. No sign-in
  screen, no user records. This is for laptops and quick looks.
- **Team mode** turns on the moment you configure GitHub OAuth or an OIDC
  provider. Now people sign in as themselves, the console keeps a record of
  who they are, and authorization follows from what their own GitHub token
  can do. This is for a shared, hosted deployment.

You do not set the mode directly. Configure sign-in and you are in team mode;
configure nothing and you are in local mode.

## Registering repositories

The console shows the GitHub repositories an administrator has registered
as source trees. Registration lives on the Admin page (or the same API the
page calls): give it the owner and name, and the default branch fills in
from GitHub if you leave it blank. The default branch is the one fact you
can change later; the owner and name are the tree's identity, so a renamed
repository is a new registration.

Deregistering a tree hides it and stops new change sets, but keeps the row
and its merged history for audit; open change sets have to merge or be
abandoned first. Register the same repository again and the console
reactivates the tree it already knew, history intact.

## Configuration

Two kinds of settings. The **shape** of the process, where it listens and
where it keeps state, comes from command-line flags. The **secrets and
policy**, who can sign in and how, come from environment variables. The
`apps/console-server/.env.example` file in the repository is a commented
starting point you can copy to `.env` and fill in.

### Command-line flags

| Flag | Default | What it does |
| --- | --- | --- |
| `--host` | `127.0.0.1` | Address to bind |
| `--port` | `7687` | Port to listen on |
| `--public-url` | `http://{host}:{port}` | The externally visible origin. OAuth redirects and cookies use it, and the allowed-origin check is derived from it. Set this whenever people reach the console at a real hostname. |
| `--data-dir` | `$XDG_DATA_HOME/rototo/console` | Where the store, stored credentials, and staged package caches live. The default persists under your XDG data home; point it somewhere else to keep everything on a volume you choose. |
| `--web` | *(auto)* | Path to the built web bundle. Falls back to the packaged copy, then a repo checkout's build output. |

### Environment variables

Nothing here is required for local mode. You reach for these when you host
the console for a team.

**Sign-in (either one turns on team mode):**

| Variable | Notes |
| --- | --- |
| `ROTOTO_GITHUB_CLIENT_ID` | GitHub OAuth app client id. Set with the secret below or not at all. |
| `ROTOTO_GITHUB_CLIENT_SECRET` | GitHub OAuth app client secret. |
| `ROTOTO_CONSOLE_OIDC_ISSUER` | OIDC issuer URL for SSO through Okta, Entra, Google, and the like. Needs the two OIDC values below. |
| `ROTOTO_CONSOLE_OIDC_CLIENT_ID` | OIDC client id. |
| `ROTOTO_CONSOLE_OIDC_CLIENT_SECRET` | OIDC client secret. |
| `ROTOTO_CONSOLE_OIDC_DISPLAY_NAME` | Label on the SSO sign-in button. Defaults to `SSO`. |

**Secrets and the write path:**

| Variable | Notes |
| --- | --- |
| `ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY` | Encrypts each user's stored token. Required before the first GitHub sign-in in team mode. Generate one with `openssl rand -base64 32`. |
| `ROTOTO_GITHUB_APP_ID` | The console's GitHub App id, used to act for people who have no GitHub credential of their own. Set with the private key or not at all. |
| `ROTOTO_GITHUB_APP_PRIVATE_KEY` | The App's private key (PEM contents). |
| `ROTOTO_GITHUB_WEBHOOK_SECRET` | Shared secret for GitHub webhook nudges. Leave it unset and the webhook endpoint stays off; the console still reconciles on its own timer. |

**Who gets in, and who starts as admin:**

| Variable | Notes |
| --- | --- |
| `ROTOTO_CONSOLE_ADMINS` | Comma-separated bootstrap administrators, written as `github:<login>` or `oidc:<email>`. Each is matched once, at that person's first sign-in, to grant a durable admin role. |
| `ROTOTO_CONSOLE_ENROLLMENT` | Who becomes a user at sign-in: `invite-only` (the default), `domain-allowlist`, or `open`. |
| `ROTOTO_CONSOLE_ENROLLMENT_DOMAINS` | Comma-separated email domains that auto-enroll under `domain-allowlist`. Ignored otherwise. |

**Packages and serving:**

| Variable | Notes |
| --- | --- |
| `ROTOTO_PACKAGE_TOKEN` | Bearer token for private HTTPS or archive package sources. |
| `ROTOTO_CONSOLE_WEB_DIST` | Path to the built web bundle, if you want to override where the server looks for it. |

A couple of rules worth knowing before they surprise you. The two paired
settings, the OAuth client id and secret, and the App id and private key,
have to be set together: set one half without the other and the server
refuses to start. And `ROTOTO_CONSOLE_OIDC_ISSUER` needs its client id and
secret alongside it.

## State, and why it is disposable

The console keeps a small SQLite store in its data directory. That store is
coordination, not content: sign-in records, cached review state, the
bookkeeping the UI needs to feel fast. The configuration itself never lives
there. It lives in git, where it always has.

That split is deliberate, and it is what makes the store safe to lose. If
the directory is deleted or the container is replaced, the console rebuilds
what it needs from GitHub. The one thing you actually lose is stored
sign-ins, so people sign in again.

### Where files go

Everything the console writes follows the XDG base directories, so it lands
where the rest of your tools already keep things:

| What | Default location | With `--data-dir` |
| --- | --- | --- |
| Coordination store (`console.sqlite`) | `$XDG_DATA_HOME/rototo/console/` | the data dir |
| Stored credentials (`credentials.json`) | `$XDG_DATA_HOME/rototo/console/` | the data dir |
| Staged package caches (`pins/`, `pins-composed/`) | `$XDG_CACHE_HOME/rototo/console/` | under the data dir |

`$XDG_DATA_HOME` falls back to `~/.local/share` and `$XDG_CACHE_HOME` to
`~/.cache` when the variables are unset. The split matters for backups: the
data directory is worth a volume, while the caches are keyed by commit and
size-bounded, safe to delete at any time and never worth backing up. An
explicit `--data-dir` collapses the split on purpose, pulling the caches
alongside the store so one volume holds everything the console touches.

## Hosting it for a team

A hosted console is the same process with team-mode settings and TLS in front
of it. The console speaks plain HTTP and expects a reverse proxy to terminate
TLS and forward requests, so a typical deployment is the server on a local
port with something like Caddy or nginx handling the public hostname and
certificate.

Two things matter when you do this:

- Set `--public-url` to the real origin people visit. The console derives its
  allowed-origin check from it, and mutating requests carry an origin check
  plus a console-specific header, so an incorrect public URL shows up as
  writes being rejected.
- Point the proxy's API path and app path at the one console port. The server
  serves both from the same process, so a single upstream is enough.

Beyond that, hosting is ordinary: state persists under the XDG data home by
default, so set `--data-dir` when you want it on a volume you manage, set
the sign-in and encryption variables for team mode, and let the reverse
proxy own TLS.

## Where the console stops

The console is for reviewing and editing a package through a friendly view.
It is not a second way to write to git that skips the pull request, and it is
not a place where configuration values secretly live. Both of those would
break the thing that makes rototo trustworthy: that every change to
production configuration is a reviewed diff with an author and a history. The
console keeps that promise by construction, and everything it adds is in
service of making that reviewed change easier and safer to make.
