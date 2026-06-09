# Workspace Sources Reference

Applications and CLI commands do not have to read a workspace from the local
filesystem. They load a workspace source. That source is the deployment handle:
it says which reviewed configuration tree should be staged, linted, and used
for future resolutions.

The same source forms are used by the [CLI](reference-cli-overview.html),
[SDK loading](reference-sdk-loading.html), and
[workspace layering](reference-workspace-layering.html).

## Supported Forms

| Form | Example | Notes |
| --- | --- | --- |
| Local path | `examples/basic` | Reads the directory directly. |
| `file://` URL | `file:///srv/config` | Reads a local directory through a file URL. Fragments are not supported. |
| `git+file://` | `git+file:///repos/config.git#main:workspace` | Clones a local git repository. |
| `git+https://` | `git+https://github.com/acme/config.git#main` | Clones over HTTPS. |
| `git+ssh://` | `git+ssh://git@example.com/acme/config.git#main` | Clones over SSH using the process git configuration. |
| HTTPS archive | `https://example.com/config.tar.gz#:workspace` | Downloads and extracts an archive over HTTPS. |

Plain `http://` sources are rejected. Use HTTPS for archive sources.

`git+http://` sources are also rejected.

## Git Fragment Syntax

Git sources support a fragment after `#`:

```text
git+https://github.com/acme/config.git#main
git+https://github.com/acme/config.git#main:workspaces/prod
git+https://github.com/acme/config.git#:workspaces/prod
```

The part before `:` is the git ref. The part after `:` is the workspace
subdirectory inside the checkout. If no ref is supplied, git's default checkout
is used. If no subdirectory is supplied, the checkout root is the workspace.

Refs that start with `-` are rejected before invoking git.

When the ref is a full 40-character commit SHA, rototo treats the source as
immutable. Immutable sources load reproducibly, but periodic refresh does not
discover new commits from them.

## HTTPS Archive Fragments

HTTPS archive sources support only subdirectory fragments:

```text
https://example.com/config.tar.gz
https://example.com/config.tar.gz#:workspaces/prod
```

Archive sources do not support a ref. If an archive contains more than one
possible workspace root, pass `#:subdir` so rototo can stage the intended
workspace.

Archive downloads follow HTTPS-only redirects. Bearer auth is available through
the CLI `--workspace-token` flag, the `ROTOTO_WORKSPACE_TOKEN` environment
variable, or SDK `SourceAuth::Bearer`.

## Subdirectories

Subdirectory fragments must be relative paths inside the staged source. Rototo
rejects unsafe paths and paths that escape the staged checkout or archive.

For local `file://` sources, fragments are not supported. Use a local path that
points directly at the workspace root.

## Source Limits

`SourceOptions::default()` uses these limits:

| Option | Default |
| --- | --- |
| Git command timeout | 60 seconds |
| HTTP request timeout | 30 seconds |
| Maximum archive download | 50 MiB |
| Maximum decompressed archive size | 200 MiB |
| Maximum archive entries | 10,000 |

The SDK exposes builder methods on `SourceOptions` for these values.

## Fingerprints

After staging, rototo records a source fingerprint when it can:

| Fingerprint | Used for |
| --- | --- |
| `GitCommit` | Git source resolved to a commit. |
| `HttpValidator` | HTTPS archive response with `ETag` or `Last-Modified`. |
| `ContentHash` | Archive response without validators. |
| `WorkspaceLayers` | Layered workspace composed from multiple sources. |

Refresh compares fingerprints before replacing the active workspace. See
[SDK Refresh](reference-sdk-refresh.html).

## Unsupported Forms

These are intentionally unsupported:

```text
http://example.com/config.tar.gz
git+http://example.com/config.git
file:///srv/config#main
https://example.com/config.tar.gz#main
```

The first two weaken the transport boundary. The file URL fragment has no
clear local meaning. The archive ref syntax would imply git semantics for a
non-git source.
