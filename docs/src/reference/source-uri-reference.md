# Source URI Reference

Workspace sources tell the CLI and SDK where to load a workspace from. A source
can be a local path, a file URI, a Git repository, or an HTTPS archive.

## Local Path

```text
./config
/srv/runtime-config/config
```

A local path points directly at a workspace directory containing
`rototo-workspace.toml`.

## `file://`

```text
file:///srv/runtime-config/config
```

`file://` sources point at local workspace directories. They do not support
fragments.

## Git Sources

Git sources use the `git+` prefix:

```text
git+https://github.com/acme/runtime-config.git#prod:config
git+ssh://git@github.com/acme/runtime-config.git#prod:config
git+file:///srv/git/runtime-config.git#prod:config
```

Format:

```text
git+<scheme>://<repo>#<ref>:<subdir>
```

Supported Git inner schemes:

```text
file
https
ssh
```

Fragment behavior:

- `#ref` selects a Git ref.
- `#ref:subdir` selects a ref and then a workspace subdirectory.
- `#:subdir` selects a subdirectory without naming a ref.

For long-running services with refresh, use a mutable controlled ref such as
`prod` or `release/prod`. A full 40-character Git commit is treated as
immutable: it can be loaded, but periodic refresh has nothing new to discover.

## HTTPS Archives

HTTPS archive sources point at `.tar.gz` archives:

```text
https://example.com/runtime-config.tar.gz
https://example.com/runtime-config.tar.gz#:config
```

Archive fragments support subdirectories only:

```text
#:subdir
```

`#ref` is not supported for HTTPS archives.

If no subdirectory is provided, rototo accepts an archive when either:

- the archive root contains `rototo-workspace.toml`; or
- the archive contains one top-level directory and that directory contains
  `rototo-workspace.toml`.

Otherwise, provide `#:subdir`.

## Unsupported Sources

Plain `http://` sources are rejected. Use `https://`.

Unsupported schemes fail before workspace loading.

## Authentication

HTTPS archive sources can use bearer token authentication through:

```text
ROTOTO_WORKSPACE_TOKEN
```

or the CLI `--workspace-token` option.

Private Git repositories use the authentication available to `git` on the host,
such as SSH agent configuration, credential helpers, or environment-specific Git
setup.

## Refresh and Fingerprints

rototo tracks source fingerprints where possible:

- Git sources use the resolved commit.
- HTTPS archives prefer `ETag`, then `Last-Modified`, then a content hash when
  loading.
- Local paths and `file://` sources do not provide a stable remote probe.

Refreshing workspaces use these fingerprints to decide whether a source changed.
Failed refreshes keep the last successfully loaded workspace active.

## Safety Rules

Subdirectories must be relative paths inside the loaded source. Absolute paths,
empty subdirectories, and paths containing `..` are rejected.

Archive extraction rejects unsafe paths and special entries.
