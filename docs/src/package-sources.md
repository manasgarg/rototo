# Package Sources

Every time you run a rototo command, or load config from an app, the very first
question is the same: *where is the package?* That answer is a **package
source** - a short string that tells rototo where to go and get your config.

The nice part is that it's one string, and it works the same everywhere. The
`app-config` you type after `rototo lint` is the same thing you put in
`ROTOTO_PACKAGE_SOURCE` for your running service, and the same thing you hand to
`Package.load(...)` in code. Learn it once and you're done.

This page walks through every shape that string can take.

## The shortest one: a folder on your machine

Most of the time, especially while you're editing, the package is just a folder
sitting on your disk. So the source is just the path to that folder:

```sh
rototo lint app-config
rototo lint ./packages/checkout
rototo lint /home/me/work/runtime-config
```

Relative or absolute, both are fine. No prefix, no ceremony - if it looks like a
path, rototo treats it as a path.

And if you don't pass anything at all, rototo is helpful about it: it looks in
the current folder, then the one above it, and so on, until it finds a
`rototo-package.toml`. So when you're already standing inside your package, you
can just say:

```sh
rototo lint
```

If you ever want to be explicit that it's a local path - say, in a script where
you'd rather not leave it to guessing - you can spell it out with `file://`:

```sh
rototo lint file:///home/me/work/runtime-config
```

It means exactly the same thing as the plain path. One small catch: the
`file://` form is *just* a folder, so it doesn't take any of the `#...` extras
described below. If you find yourself wanting those, you probably want a git
source instead.

## Loading straight from a git repo

Here's where it gets useful. Your config lives in git - that's the whole point -
so you can point rototo straight at the repo without cloning it yourself first.
You do that by sticking `git+` in front of the repo URL:

```sh
# over HTTPS
rototo lint git+https://github.com/acme/runtime-config.git

# over SSH
rototo lint git+ssh://git@github.com/acme/runtime-config.git

# even a local bare repo
rototo lint git+file:///srv/git/runtime-config.git
```

rototo fetches the repo into a temporary spot, reads the package, and cleans up
after itself. You don't manage the checkout.

Two things you'll almost always want to add: *which version* and *which folder*.
That's what the part after the `#` is for.

### The bit after the `#`: version and folder

A git source can carry two extra details, written as `#ref:subdir`:

- the **ref** - a branch, a tag, or a full commit. This is the "which version"
  part.
- the **subdir** - the folder inside the repo where the package actually lives.
  Repos often hold more than one thing, so this says "the package is in *here*".

Put together:

```sh
rototo lint git+https://github.com/acme/runtime-config.git#main:packages/checkout
```

Read that as: *the `runtime-config` repo, on `main`, and the package is in
`packages/checkout`.*

You don't have to supply both. The colon is the divider, so:

- `#main` - just a version. (Package is at the repo root.)
- `#main:packages/checkout` - version *and* folder.
- `#:packages/checkout` - just a folder, on whatever the repo's default is.
  (Note the leading colon: nothing before it means "no specific ref".)

A quick word on which ref to pick, because it changes how things behave later:

- A **branch** like `main` keeps moving - point at it and you'll pick up newer
  reviewed commits over time.
- A **tag** is a friendly name for a fixed release.
- A **full commit** is nailed down forever - perfectly reproducible, but it
  never changes, so a long-running service watching it will never see anything
  new.

That last point matters for [refreshing services](./adoption.md): a moving ref
is how config reaches a running fleet without redeploying; a pinned ref is how
you guarantee an exact, reproducible build.

## Loading a packaged-up archive over HTTPS

Pulling from git is great for small setups, but once you've got a real fleet,
having every instance clone from GitHub on a schedule gets fragile - now your
config depends on GitHub being up and on fetch behaving at scale. So for
production, the usual move is to pack the package into a single `.tar.gz` file,
drop it on an object store behind a CDN, and load *that*:

```sh
rototo lint https://config.acme.com/rototo/checkout/prod/current.tar.gz
```

(You build that archive with `rototo package`, which writes a deterministic
tarball you can upload.)

An archive is already a self-contained, fixed thing, so there's no "ref" to pick
- a `.tar.gz` is whatever it is. The only extra you can add is the folder
inside it, using the leading-colon form:

```sh
rototo lint https://config.acme.com/rototo/checkout/prod/current.tar.gz#:packages/checkout
```

Because an archive is just a file at a URL, the *URL* is what decides whether it
moves or stays put:

- a URL addressed by content, like `.../sha256:0f4c...b91.tar.gz`, always points
  at the exact same bytes - safe to cache hard, perfect for reproducible
  releases.
- a "channel" URL like `.../prod/current.tar.gz` is meant to be re-pointed at a
  newer archive when you promote a release, so it should be cached only briefly.

## One thing rototo won't do: plain `http://`

You can load over `https://`, but not plain `http://`. Same goes for git:
`git+https://` and `git+ssh://` are fine, `git+http://` is not. This isn't an
oversight - config is exactly the kind of thing you don't want a network snoop
or man-in-the-middle messing with, so the unencrypted forms are turned off on
purpose. If you try one, rototo tells you to use `https://` instead.

## Private sources need a token

If your repo or archive is private, rototo needs a credential to fetch it. It
uses a **bearer token**, and there are two ways to hand it over:

```sh
# as a flag
rototo lint git+https://github.com/acme/private-config.git --package-token "$TOKEN"

# or as an environment variable
ROTOTO_PACKAGE_TOKEN="$TOKEN" rototo lint git+https://github.com/acme/private-config.git
```

The environment variable is the friendlier choice for CI and for running
services, since you set it once and forget it. The same token flows through to
private HTTPS archives, too.

One habit worth keeping: when CI checks that production can really load the
released package, use the *same* authenticated source production will use. That
way you find out about a permissions problem in CI, not at 2am.

## When one package builds on another

A package can stand on top of other packages - shared defaults, a common set of
qualifiers, that sort of thing. It does that by listing other sources in its
`rototo-package.toml`, and here's the part that matters for this page: those
parent sources use the **exact same grammar** as everything above. A parent can
be a local folder, a git repo with `#ref:subdir`, or an HTTPS archive.

Relative paths in that list are resolved against the package doing the
extending, so a package and the parents it leans on can travel together. The
mechanics of how the layers combine live in the [package format
reference](./package-format.md); the only thing to remember here is that there's
nothing new to learn about the sources themselves.

## The whole grammar on one page

If you just want the cheat sheet:

| You want to load… | Write it like this |
| --- | --- |
| A folder on disk | `app-config`, `./pkg`, `/abs/path` |
| Nearest package above you | *(omit the source entirely)* |
| A folder, spelled out | `file:///abs/path` |
| A git repo | `git+https://…`, `git+ssh://…`, `git+file://…` |
| A git repo, specific version | `git+https://…#main` |
| A git repo, version + folder | `git+https://…#main:packages/checkout` |
| A git repo, folder only | `git+https://…#:packages/checkout` |
| An HTTPS archive | `https://…/current.tar.gz` |
| An HTTPS archive, folder inside | `https://…/current.tar.gz#:packages/checkout` |

And the things rototo will refuse: plain `http://`, `git+http://`, and a `#...`
fragment on a `file://` source.
