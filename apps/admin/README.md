# rototo Admin

This is the first hosted admin UI skeleton for rototo workspaces:

- sign in with GitHub;
- add a GitHub repository by `owner/name`;
- discover `rototo-workspace.toml` files in that repository;
- open a discovered workspace and render semantic lint diagnostics;
- create a draft branch, edit primitive variable defaults, and publish a pull
  request.

## Local Setup

Create a GitHub OAuth App with this callback URL:

```text
http://localhost:3000/api/auth/github/callback
```

Then configure:

```sh
cp apps/admin/.env.example apps/admin/.env.local
```

Fill in `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`, and
`ROTOTO_ADMIN_TOKEN_ENCRYPTION_KEY`. Generate the token encryption key with:

```sh
openssl rand -base64 32
```

The admin UI requests the fixed GitHub OAuth scopes it needs for its current API
calls:

- `read:user` to identify the signed-in user;
- `repo` to read workspaces and create branches, commits, and pull requests in
  repositories the user can write to.

Then run:

```sh
just admin-setup
just admin-dev
```

The app stores local development state in `apps/admin/.rototo-admin` by default.
The browser receives only an opaque session cookie. SQLite stores only a hash of
that session token. GitHub access tokens are encrypted before they are written
to the local SQLite database.
