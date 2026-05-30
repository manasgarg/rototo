# Contributing to rototo

## Worktrees

Use `/home/manas/projects/rototo/main` as a clean reference worktree. Create a
separate worktree for each task:

```sh
cd /home/manas/projects/rototo
git --git-dir=.bare worktree add -b my-branch my-branch main
```

Do development inside the task worktree, not `main`.

## Setup

Install `mise` and `just`, then run:

```sh
mise trust
just setup
```

Rust is pinned by `rust-toolchain.toml`. Python is pinned by `.tool-versions`
and is used to install `pre-commit`.

## Checks

Run the local gate before pushing:

```sh
just check
```

`just check` runs formatting checks, clippy with warnings denied, and the test
suite.
