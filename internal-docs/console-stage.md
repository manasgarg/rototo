# Console Stage Behavior

This note documents what `src/console/stage/` does for the console today and
what a replacement must preserve. It is intentionally written as a behavior
contract, not as a map of the current implementation. The goal of a rewrite
should be a staging service that is easy to reason about, even if it gives up
some incidental optimizations from the current code.

## Role

The console needs a stable filesystem view of a workspace source before it can
lint files, build the semantic model, read entity definitions, serve LSP
requests, or preview runtime resolution. Workspace sources may be local paths,
`file://` URIs, git sources, or HTTPS archives. Remote sources are staged into
temporary directories whose lifetime must outlive the API response or LSP
session using them.

The stage layer is the console's process-local staging cache. It sits below API
routes and above the SDK `Workspace` type.

Its responsibilities are:

- turn a user-visible source tree into a local filesystem tree;
- discover or address workspace paths inside that tree;
- turn one workspace path into an inspected `Workspace`;
- compute and cache the semantic model for that inspected workspace;
- turn that workspace path into a runtime-capable `Workspace` for resolution
  previews, which requires lint-clean loading;
- keep staged temporary files alive for every workspace handle that points at
  them;
- avoid repeated remote fetches on hot request paths;
- refresh stale remote state without making every request wait;
- invalidate cached branch checkout state after console writes commit new files.

It should not own console persistence, branch write behavior, GitHub API writes,
inventory projection, resolution semantics, lint semantics, or LSP protocol
logic.

## Source Of Record Principle

The source tree is the system of record. Console state is either a cache,
derived index, user session, or temporary editing manifestation. A fresh
console instance should be able to authenticate, point at the same source tree,
discover the same workspaces and branches, and continue from the git/PR state
already present there.

That means:

- workspace files, git commits, branches, tags, and pull requests are durable;
- console staging state is disposable;
- console database rows are rehydratable indexes or session records, not the
  authority for configuration or branch work;
- branch work should be recoverable from git branches and PRs, not only from a
  local console row;
- there should be no local-only activity timeline in the target model.

The console can create a branch or attach to an existing branch. The branch is
the durable unit of work. The console may cache metadata about that branch, but
the user's ability to continue work should come from the branch and its files.

Terminology in this note:

- **Branch work** is the target model: editing against a git branch that can be
  created by the console or selected because it already exists.
- **Draft** refers to the current implementation's UI/API/store naming. During
  migration, draft records may index branch work, but they should not be the
  source of record.

## Proposed Data Model

The console is source-tree-oriented, not only workspace-oriented. The SDK
loads one workspace source at a time. The console starts from a source tree
registered for a user, discovers one or more workspace roots inside that tree,
and lets branch work modify files that may affect multiple workspaces in the
same tree.

The replacement should make that distinction explicit:

- git branches and PRs are the durable units of branch work;
- SQLite rows are indexes, sessions, or cached metadata that can be rebuilt;
- process-local staging state lives in `src/console/stage`;
- source-tree staging owns physical files and lifecycle state;
- workspace staging points at one workspace path inside a staged tree and uses
  the SDK to build workspace representations from that path.

Canonical data flow:

```text
remote tree
  -> locally staged tree
  -> selected branch/worktree
  -> inspected workspace(s)
  -> semantic workspace(s)
  -> runtime workspace(s), when lint-clean previews need resolution
```

The locally staged tree is disposable. It is a physical manifestation of the
remote tree plus the selected branch refs, not a second source of truth. For
each branch the user chooses to work on, stage can materialize one worktree and
one set of inspected, semantic, and runtime workspace views per workspace path.

Workspace representations are per branch/worktree. If a user works on `main`
and on `rototo-console/alice/change-checkout` at the same time, the same
workspace path has separate inspected, semantic, and runtime views for each
branch.

### Durable State And Runtime State

Target model:

- `TreeSourceRecord`: an index of a source tree registered by a user;
- `TrackedBranchRecord`: an index of a git branch the user is actively working
  on, recently opened, or recently edited.

These records are still not the source of truth for configuration. They are a
durable index and session aid for the console. The source tree, branch refs,
commits, and PRs remain authoritative.

The current implementation also has compatibility rows:

- current `DraftSessionRecord`: compatibility/index metadata for a branch
  started by the console;
- current `DraftChangeRecord`: compatibility/index metadata for semantic
  changes tracked by the current UI.

Those rows should be replaced, not preserved as design constraints. During
migration they can be read to recover branch names and changed files, but the
new store should follow the source-tree/tracked-branch model directly. If the
console database is deleted, branch work should still be recoverable when the
user selects the branch again or the provider can list relevant branches/PRs.

The stage layer should not persist these concepts again. Dropping the console
process should lose only cached checkouts, worktrees, extracted trees, derived
workspace views, and session-local editor state that was never committed.

### Top-Level Cache

`StageCache` should own staged source trees keyed by the user, the normalized
source tree, and the credential identity used to read that tree. It is the
process-local entry point used by API routes.

```rust
pub struct StageCache {
    tree_sources: Mutex<HashMap<CachedTreeSource, Arc<TreeSourceSlot>>>,
}

type TreeSourceSlot = Mutex<StagedTreeSource>;
type WorkspaceSlot = Mutex<StagedWorkspace>;
type BranchSlot = Mutex<BranchCheckout>;
```

The key must not contain raw bearer tokens. It should include a stable,
non-reversible token identity when the same remote source can be read with
different credentials.

```rust
pub struct CachedTreeSource {
    pub principal_id: String,
    pub tree: TreeSource,
    pub token: TokenIdentity,
}
```

### Staged Source Tree

`StagedTreeSource` represents the local physical manifestation of a source
tree for one user. For a GitHub repository this is the repo-level object: it
knows the remote repo identity, the local git cache/worktrees, discovered
workspace paths, branch checkouts, and lifecycle state.

```rust
pub struct StagedTreeSource {
    pub key: CachedTreeSource,
    pub tree: TreeSource,
    pub local: LocalTree,
    pub branches: HashMap<BranchName, Arc<BranchSlot>>,
    pub workspace_views: HashMap<WorkspaceViewKey, Arc<WorkspaceSlot>>,
    pub lifecycle: TreeSourceLifecycle,
}

pub struct BranchName(GitRefName);

pub struct WorkspaceViewKey {
    pub path: WorkspacePath,
    pub revision: TreeRevision,
}
```

`TreeSource` describes the source-of-record tree, not one workspace or branch
inside it. For GitHub this is owner/name. For local folders it is the
canonical root path. For archives it is the archive URL.

```rust
pub enum TreeSource {
    GitHub {
        owner: NormalizedGitHubName,
        name: NormalizedGitHubName,
    },
    GitRemote {
        remote: NormalizedGitRemote,
    },
    LocalFolder {
        root: CanonicalPath,
    },
    Archive {
        url: NormalizedHttpsUrl,
    },
}
```

`LocalTree` is the staged filesystem manifestation that the console can point
the SDK at. It is polymorphic because not every source tree supports the same
operations. Git-backed trees support worktrees and branch checkouts. Archive
and plain local-folder trees can still expose discovered workspaces, but they
do not naturally own durable edit branches.

```rust
pub enum LocalTree {
    GitRepo(Arc<LocalGitRepo>),
    Directory(Arc<LocalFolderTree>),
    ExtractedArchive(Arc<ExtractedArchiveTree>),
}
```

```rust
pub struct LocalFolderTree {
    pub root: PathBuf,
}

pub struct ExtractedArchiveTree {
    pub url: String,
    pub root: PathBuf,
    pub fingerprint: Option<SourceFingerprint>,
    pub keep_alive: Arc<TempDir>,
}
```

### Local Git Repo

`LocalGitRepo` is the staged git manifestation of a git-backed source tree. It
owns the bare repository and the worktree roots checked out from that
repository.

```rust
pub struct LocalGitRepo {
    pub remote: RepoRemote,
    pub bare_dir: PathBuf,
    pub worktrees_dir: PathBuf,
    pub worktrees: HashMap<WorktreeKey, Arc<RepoWorktree>>,
    pub fetch_lock: Mutex<()>,
    pub keep_alive: Arc<TempDir>,
}

pub struct RepoWorktree {
    pub key: WorktreeKey,
    pub ref_name: GitRefName,
    pub commit: Option<GitCommit>,
    pub root: PathBuf,
    pub keep_alive: Arc<TempDir>,
    pub lifecycle: WorktreeLifecycle,
}

pub enum WorktreeKey {
    GitRef(GitRefName),
    GitBranch(GitRefName),
    GitCommit(GitCommit),
}
```

The bare repo and worktrees are an optimization and a lifecycle tool for
repo-backed console workflows. They should not take over workspace semantics.
Once a workspace path is selected inside a worktree, rototo semantics should
come from the SDK.

### Staged Workspace

`StagedWorkspace` is a workspace path inside a selected tree checkout. Its
identity is `WorkspaceViewKey`: the tree-relative workspace path plus the tree
selection being read. It owns derived representations built from raw files at
that path in that selected tree.

```rust
pub struct StagedWorkspace {
    pub key: WorkspaceViewKey,
    pub path: WorkspacePath,
    pub root: PathBuf,
    pub backing: WorkspaceBacking,
    pub inspected: OnceCell<Arc<Workspace>>,
    pub semantic: OnceCell<Arc<WorkspaceSemanticModel>>,
    pub runtime: OnceCell<Arc<Workspace>>,
    pub lifecycle: WorkspaceLifecycle,
}

pub enum WorkspaceBacking {
    GitWorktree {
        worktree: Arc<RepoWorktree>,
    },
    LocalFolder {
        tree: Arc<LocalFolderTree>,
    },
    ExtractedArchive {
        tree: Arc<ExtractedArchiveTree>,
    },
}
```

Do not make a staged workspace hold an `Arc<StagedTreeSource>` if the source
tree also owns the workspace slot. That creates an easy reference cycle.
Workspace backing should hold only the physical manifestation needed to keep
the workspace root valid.

The construction flow should be:

1. choose a staged source tree;
2. choose a worktree/tree root;
3. choose a workspace path inside that tree;
4. call SDK APIs on that concrete workspace root;
5. cache derived representations on `StagedWorkspace`.

The SDK remains the semantic authority for `Workspace`, lint, runtime loading,
context validation, and resolution. The console stage layer is responsible for
finding and keeping alive the filesystem root passed to the SDK.

### Semantic Workspace

Most console callers need the semantic model together with the inspected
workspace root. Use a named return object rather than a tuple.

```rust
pub struct SemanticWorkspace {
    pub inspected: Arc<Workspace>,
    pub model: Arc<WorkspaceSemanticModel>,
}
```

### Branch Checkout

A `BranchCheckout` belongs to the source tree, not to one workspace. A branch
can change files that affect several workspaces under the same repo. The
current UI may still call this a draft, but the stage model should think in
terms of a branch checked out locally.

```rust
pub struct BranchCheckout {
    pub branch: GitRefName,
    pub base_ref: GitRefName,
    pub worktree: Arc<RepoWorktree>,
    pub modified_workspaces: HashSet<WorkspacePath>,
    pub modified_files: HashSet<RepoRelativePath>,
    pub lifecycle: BranchCheckoutLifecycle,
}
```

The branch and its commits are durable. `BranchCheckout` is the in-process
state that maps that durable branch to a local worktree, records which files
appear modified relative to the base, and knows which workspace caches must be
invalidated after a save. The target model should derive changed files from
the branch diff when possible. If diff computation becomes expensive, cache it
inside stage rather than adding durable file-diff rows.

### Lifecycle State

Lifecycle metadata should be explicit but small. Each level needs enough state
to prevent duplicate work, decide whether revalidation is needed, and keep the
last known good manifestation alive.

```rust
pub struct TreeSourceLifecycle {
    pub staged_at: Instant,
    pub last_revalidated_at: Option<Instant>,
    pub revalidation: RevalidationState,
}

pub struct WorktreeLifecycle {
    pub staged_at: Instant,
    pub last_commit_check_at: Option<Instant>,
    pub immutable: bool,
    pub revalidation: RevalidationState,
}

pub struct WorkspaceLifecycle {
    pub loaded_at: Instant,
    pub source_fingerprint: Option<SourceFingerprint>,
}

pub struct BranchCheckoutLifecycle {
    pub staged_at: Instant,
    pub last_saved_at: Option<Instant>,
    pub revalidation: RevalidationState,
}

pub enum RevalidationState {
    Idle,
    Running,
}
```

Keep this lifecycle state operational. Avoid turning it into a second
database: durable branch state lives in git and PRs; user sessions and cached
indexes may live in the store.

## Open Design Gaps

The data model above gives the main objects, but several contracts still need
to be decided before implementation. These are the gaps most likely to affect
the shape of the code.

### Identity Normalization

Resolved direction: normalize tree sources, workspace paths, tree revisions,
and credential identity before touching cache state. Stage code should not
match raw workspace source strings except in the adapter that converts current
store records into workspace sources.

The core address is:

```rust
pub struct WorkspaceSource {
    pub tree: TreeSource,
    pub revision: TreeRevision,
    pub path: WorkspacePath,
}
```

Read this as:

```text
WorkspaceSource = where the tree comes from
                + which revision of that tree to read
                + where the workspace lives inside that tree
```

Cache/user identity wraps the core address. It is not part of the source of
record.

#### Cache Key

`CachedTreeSource` is a process-local cache key. It should not be persisted.

```rust
pub struct CachedTreeSource {
    pub principal_id: String,
    pub tree: TreeSource,
    pub token: TokenIdentity,
}

pub enum TokenIdentity {
    None,
    Sha256Hex(String),
}
```

Rules:

- `principal_id` is the durable console principal id from the store.
- `tree` is the normalized tree source identity, not a workspace source URI.
- `token` is `None` for unauthenticated reads and `Sha256Hex(full_digest)` for
  authenticated reads.
- Raw bearer tokens must never appear in keys, logs, errors, or debug output.
- A token change creates a different process-local cache key. That is
  acceptable for the first implementation; old trees can be dropped by
  invalidation or eventual eviction.
- The digest is stable for the same token, but it is still cache identity, not
  durable product data.

Use the full SHA-256 hex digest in the key to avoid collisions. Logs may show a
short prefix when needed.

Examples:

```text
principal "user_123", GitHub "Rototo/Config", no token
  -> CachedTreeSource {
       principal_id: "user_123",
       source: TreeSource::GitHub { owner: "rototo", name: "config" },
       token: TokenIdentity::None,
     }

principal "user_123", same repo, bearer token "ghp_secret"
  -> CachedTreeSource {
       principal_id: "user_123",
       source: TreeSource::GitHub { owner: "rototo", name: "config" },
       token: TokenIdentity::Sha256Hex(
         "4c281411e1ccc93c230902001a09e7b863cb12a3f3b341089eb980a34aa9e434",
       ),
     }

principal "user_456", same repo, same token
  -> different CachedTreeSource because principal_id differs
```

#### Tree Source

`TreeSource` identifies a tree before selecting a workspace path inside
it.

```rust
pub enum TreeSource {
    GitHub {
        owner: NormalizedGitHubName,
        name: NormalizedGitHubName,
    },
    GitRemote {
        remote: NormalizedGitRemote,
    },
    LocalFolder {
        root: CanonicalPath,
    },
    Archive {
        url: NormalizedHttpsUrl,
    },
}
```

Rules:

- GitHub owner and repo names are lowercased for identity. Preserve the display
  spelling from store/API records outside the key when the UI needs it.
- A GitHub repo registered as `owner/name` and a GitHub remote source such as
  `git+https://github.com/owner/name.git#main:path` should normalize to the
  same `TreeSource::GitHub` when the adapter can recognize it.
- GitHub HTTPS, SSH, and `git@github.com:owner/name.git` forms should strip a
  single trailing `.git`, strip trailing slashes, lowercase owner/name, and
  keep the ref/branch separate from the tree identity.
- Generic git remotes use `TreeSource::GitRemote`. Normalize only the
  URL scheme and host casing and remove trailing slashes. Do not try to prove
  broader remote equivalence for non-GitHub hosts.
- Local folders use an absolute canonical path after filesystem
  canonicalization. This resolves symlinks and makes `./repo` and
  `/abs/path/repo` share identity.
- Archive identity is the HTTPS archive URL without the `#:subdir` fragment.
  Lowercase scheme and host, preserve path, query, and other URL components
  that affect the fetched bytes.
- GitHub archive URLs encountered during migration should be adapted to
  `TreeSource::GitHub` when owner/name are already available from a legacy
  repo row; the ref becomes the selected `TreeRevision`. Arbitrary archive
  URLs stay `TreeSource::Archive`.

Examples:

```text
GitHub repo record owner="Rototo", name="Config"
  -> TreeSource::GitHub { owner: "rototo", name: "config" }

git+https://github.com/Rototo/Config.git#main:workspaces/payments
  -> TreeSource::GitHub { owner: "rototo", name: "config" }
  -> TreeRevision::GitRef("main")
  -> WorkspacePath("workspaces/payments")

git+ssh://git@github.com/Rototo/Config.git#feature/payments:.
  -> TreeSource::GitHub { owner: "rototo", name: "config" }
  -> TreeRevision::GitRef("feature/payments")
  -> WorkspacePath(".")

git@github.com:Rototo/Config.git
  -> TreeSource::GitHub { owner: "rototo", name: "config" }

git+https://Git.Example.com/Team/Config.git#main:services/api
  -> TreeSource::GitRemote {
       remote: "git+https://git.example.com/Team/Config.git",
     }
  -> TreeRevision::GitRef("main")
  -> WorkspacePath("services/api")

/home/alice/config-link, where config-link resolves to /srv/repos/config
  -> TreeSource::LocalFolder { root: "/srv/repos/config" }

https://EXAMPLE.com/releases/config.tar.gz#:workspaces/payments
  -> TreeSource::Archive {
       url: "https://example.com/releases/config.tar.gz",
     }
  -> WorkspacePath("workspaces/payments")

GitHub archive URL migrated from legacy repo owner="Rototo", name="Config", ref="main"
  -> TreeSource::GitHub { owner: "rototo", name: "config" }
  -> TreeRevision::GitRef("main")
```

#### Workspace Path

`WorkspacePath` identifies a workspace root inside a source tree.

```rust
pub struct WorkspacePath(String);
```

Rules:

- The repository/tree root workspace is always `"."`.
- Other workspace paths are slash-separated, relative paths.
- Reject empty paths except when normalizing to `"."`.
- Reject absolute paths.
- Reject `.` and `..` components.
- Reject empty components, so `a//b` is invalid.
- Strip one or more trailing slashes before validation.
- Store paths with `/` separators, even on Windows.
- Do not canonicalize a `WorkspacePath` through the host filesystem; it is a
  logical tree-relative identity. Filesystem canonicalization happens only when
  joining the path against a staged root.

Examples:

```text
""                  -> WorkspacePath(".")
"."                 -> WorkspacePath(".")
"workspaces/payments" -> WorkspacePath("workspaces/payments")
"workspaces/payments/" -> WorkspacePath("workspaces/payments")
"workspaces\\payments" -> WorkspacePath("workspaces/payments") on Windows input
"/workspaces/payments" -> reject: absolute path
"../payments"       -> reject: parent traversal
"workspaces/../api" -> reject: parent traversal
"workspaces//api"   -> reject: empty component
```

#### Repo-Relative File Path

`RepoRelativePath` identifies a file path inside a source tree or worktree.

```rust
pub struct RepoRelativePath(String);
```

Rules:

- Same normalization as `WorkspacePath`, except `"."` is not a valid file path.
- The path must refer to a file or file-like target, not a workspace root.
- Use this for branch modified files and compatibility store change file paths.

Examples:

```text
"workspaces/payments/variables/checkout.toml"
  -> RepoRelativePath("workspaces/payments/variables/checkout.toml")

"rototo-workspace.toml"
  -> RepoRelativePath("rototo-workspace.toml")

"."                 -> reject: not a file path
""                  -> reject: not a file path
"/rototo-workspace.toml" -> reject: absolute path
"workspaces/payments/../api/file.toml" -> reject: parent traversal
```

#### Source Tree Selection

`TreeRevision` answers the question that `WorkspaceVersion` was trying to
answer: which content tree should this workspace view read from?

It is not a workspace version in the semantic-version sense. It is the selected
tree lane for a workspace path. For a git-backed source it might be the base
ref, a working branch, or an immutable commit. For non-git sources it might be
the current local directory or an extracted archive fingerprint.

```rust
pub enum TreeRevision {
    GitRef(GitRefName),
    GitBranch(BranchName),
    GitCommit(GitCommit),
    LocalWorkingTree,
    ArchiveSnapshot(SourceFingerprint),
}
```

Rules:

- `GitRef` is valid for git-backed trees. Preserve git ref case because refs
  are case-sensitive. Reject empty refs and refs beginning with `-`.
- `GitBranch` is valid for git-backed trees with a branch worktree. Target routes
  should get the branch name from `TrackedBranchRecord`. During migration, legacy
  draft rows can be converted into `TrackedBranchRecord` rows by reading their durable
  branch names.
- `GitCommit` is a 40-character lowercase hex commit id.
- `LocalWorkingTree` is valid for local-folder trees and represents the current
  filesystem contents at the canonical root.
- `ArchiveSnapshot` is valid for archive trees after staging or probing.
- Route selectors may start with requested selections such as `GitRef("main")`
  or `GitBranch(branch_name)`. Staged physical objects should record resolved
  facts such as commit SHA or archive fingerprint.

Examples:

```text
workspace route for repo default branch "main"
  -> TreeRevision::GitRef("main")

branch editor route for branch "rototo-console/alice/change-checkout"
  -> TreeRevision::GitBranch(BranchName("rototo-console/alice/change-checkout"))

TrackedBranchRecord { branch: "rototo-console/alice/change-checkout" }
  -> TreeRevision::GitBranch(BranchName("rototo-console/alice/change-checkout"))

immutable source git+https://github.com/rototo/config.git#8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5:.
  -> TreeRevision::GitCommit("8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5")

local folder source /srv/repos/config
  -> TreeRevision::LocalWorkingTree

staged archive with fingerprint sha256:abc123
  -> TreeRevision::ArchiveSnapshot("sha256:abc123")
```

#### Worktree Key

`WorktreeKey` identifies one physical git worktree under a `LocalGitRepo`.

```rust
pub enum WorktreeKey {
    GitRef(GitRefName),
    GitBranch(GitRefName),
    GitCommit(GitCommit),
}
```

Rules:

- `GitRef` worktrees are mutable and may move to a new commit after
  revalidation.
- `GitBranch` worktrees are mutable and invalidated after console saves.
- `GitCommit` worktrees are immutable.
- Worktree keys identify the requested worktree lane. The `RepoWorktree`
  stores the currently resolved commit separately.

Examples:

```text
TreeRevision::GitRef("main")
  -> WorktreeKey::GitRef("main")
  -> RepoWorktree {
       resolved_commit: "a1b2c3d4e5f60718293a4b58d3c4b5a6f7081920",
     }

after remote main advances
  -> same WorktreeKey::GitRef("main")
  -> refreshed RepoWorktree {
       resolved_commit: "c3d4e5f60718293a4b58d3c4b5a6f7081920a1b2",
     }

TreeRevision::GitBranch(BranchName("rototo-console/alice/change-checkout"))
  -> WorktreeKey::GitBranch("rototo-console/alice/change-checkout")

TreeRevision::GitCommit("8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5")
  -> WorktreeKey::GitCommit("8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5")
```

#### Stable Versus Process-Local

Stable enough to store or compare across process restarts:

- durable `principal_id`;
- `TreeSourceRecord.id`, `TrackedBranchRecord.id`;
- normalized `TreeSource`;
- `WorkspacePath`;
- `RepoRelativePath`;
- `BranchName`;
- `GitCommit`;
- git ref names as requested refs.

Process-local only:

- `CachedTreeSource` as a whole, because it includes `TokenIdentity`;
- tempdir paths;
- worktree root paths;
- `Arc` identity;
- `OnceCell` state;
- revalidation state.

This resolves the identity-normalization gap. Routes should build these
normalized identities from the target store records, not from raw source
strings.

### Target Store And Stage Mapping

The store should change to match the source-tree-first model. Do not preserve
the current workspace-oriented shape as a design constraint. Keep legacy
`RepoRecord`, workspace rows, `DraftSessionRecord`, and `DraftChangeRecord`
only as migration inputs while the console moves to the new shape.

#### Target Records

These records are durable console indexes. Keep the store small. It should
remember the source trees a user has registered and the branches the user is
actively or recently working on. Workspaces, affected workspace lists, and
changed file lists can be derived from the staged source tree and branch diff.

The records make the console quick to reopen, but they do not become the
source of truth for configuration. Source trees, branch refs, commits, and PRs
remain authoritative.

```rust
pub struct TreeSourceRecord {
    pub id: TreeSourceId,
    pub principal_id: String,
    pub source: TreeSource,
    pub default_ref: Option<GitRefName>,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
    pub last_opened_at: DateTime<Utc>,
    pub last_validated_at: Option<DateTime<Utc>>,
}

pub struct TrackedBranchRecord {
    pub id: TrackedBranchId,
    pub tree_source_id: TreeSourceId,
    pub branch: BranchName,
    pub base_ref: GitRefName,
    pub base_commit: Option<GitCommit>,
    pub pull_request: Option<PullRequestRef>,
    pub last_selected_workspace: Option<WorkspacePath>,
    pub last_seen_commit: Option<GitCommit>,
    pub tracking: BranchTrackingState,
    pub created_at: DateTime<Utc>,
    pub last_opened_at: DateTime<Utc>,
    pub last_edited_at: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
}

pub enum BranchTrackingState {
    Active,
    Recent,
    Archived,
}
```

The target store intentionally does not include bearer tokens, temp paths,
discovered workspace lists, branch affected workspace lists, changed file
lists, inspected workspaces, semantic models, runtime workspaces, LSP
sessions, or revalidation locks. Those are derived, process-local, or
request-local.

There is intentionally no durable `WorkspaceRecord` in the target model.
Workspace paths are facts discovered from a selected source tree checkout. A
route may pass a `WorkspacePath`, and a tracked branch may remember one
`last_selected_workspace` as a navigation hint, but the console does not need a
workspace catalog table to reconstruct state.

#### Invariants

Store constraints should keep identity decisions out of route handlers:

- `TreeSourceRecord` is unique by `(principal_id, source)`.
- `TrackedBranchRecord` is unique by `(tree_source_id, branch)`.
- `TrackedBranchRecord.base_ref` is the comparison and PR target ref. It is not part
  of `TreeSource`.
- `TrackedBranchRecord.last_selected_workspace` is a navigation hint, not an
  assertion that the branch affects only that workspace.
- `TrackedBranchRecord.last_seen_commit` and
  `TreeSourceRecord.last_validated_at` are freshness metadata. They should not
  be used as configuration authority.
- `BranchTrackingState::Active` means the user intentionally has this branch in
  their working set.
- `BranchTrackingState::Recent` means the branch should appear in recently
  seen or edited lists, but the user is not actively working on it.
- `BranchTrackingState::Archived` hides the branch from the normal working set
  without deleting the remote branch.
- A tracked branch may have no selected workspace. In that case the console
  should derive affected workspaces from the branch diff or ask the user to
  choose one.

These constraints are enough to answer the main lifecycle questions:

- one registered source tree per user/source identity;
- workspace lists are derived from source tree contents;
- many tracked branches per source tree;
- a branch can be reopened without knowing which workspace originally created
  it;
- a workspace view is derived from `(source tree, workspace path, source tree
  selection)`.

#### Recovery Behavior

If the console database is deleted, the user should be able to register the
same source tree again and recover durable work from the remote. Recovery has
different levels:

- `TreeSourceRecord` can be recreated from the user-provided source location and the
  authenticated principal.
- Workspace paths can be rebuilt by scanning a selected base tree or branch
  checkout for `rototo-workspace.toml`.
- `TrackedBranchRecord` rows can be recreated when the user selects an
  existing branch, or when the provider lists branches/PRs that the console can
  present as importable.
- Affected workspaces can be rebuilt by diffing the branch against `base_ref`
  and mapping changed files to discovered workspace paths.
- Changed file lists can always be rebuilt from the branch diff.
- The last selected workspace is recoverable only when it is encoded in branch
  metadata, PR metadata, or inferred from the diff. When it cannot be
  recovered, the branch should still be usable with no selected workspace.
- Local editor sessions, unsaved buffers, temp paths, activity timelines, and
  LSP sessions are not recovered.

This keeps the store aligned with the source-of-record principle: losing the
console database loses working-set and navigation convenience, not committed
branch work.

#### Workspace Source Mapping

The target mapping from store to stage is direct. Route handlers combine a
stored source tree, a workspace path derived from the source tree, and an
optional tracked branch into one `CachedWorkspaceSource` before touching stage:

```rust
pub struct WorkspaceSource {
    pub tree: TreeSource,
    pub revision: TreeRevision,
    pub path: WorkspacePath,
}

pub struct CachedWorkspaceSource {
    pub principal_id: String,
    pub token: TokenIdentity,
    pub workspace: WorkspaceSource,
}
```

```text
TreeSourceRecord + WorkspacePath + token + GitRef(default_ref)
  -> CachedWorkspaceSource {
       principal_id,
       token,
       workspace: WorkspaceSource {
         tree,
         revision: TreeRevision::GitRef(default_ref),
         path: workspace_path,
       },
     }

TreeSourceRecord + WorkspacePath + token + TrackedBranchRecord
  -> CachedWorkspaceSource {
       principal_id,
       token,
       workspace: WorkspaceSource {
         tree,
         revision: TreeRevision::GitBranch(branch.branch),
         path: workspace_path,
       },
     }

TreeSourceRecord + WorkspacePath + token + GitCommit
  -> CachedWorkspaceSource {
       principal_id,
       token,
       workspace: WorkspaceSource {
         tree,
         revision: TreeRevision::GitCommit(commit),
         path: workspace_path,
       },
     }
```

Examples:

```text
Open the base workspace at "."
  TreeSourceRecord {
    principal_id: "user_123",
    tree: TreeSource::GitHub { owner: "rototo", name: "config" },
    default_ref: Some("main"),
  }
  token: TokenIdentity::Sha256Hex(
    "4c281411e1ccc93c230902001a09e7b863cb12a3f3b341089eb980a34aa9e434",
  )
  workspace path: WorkspacePath(".")
  -> CachedWorkspaceSource {
       principal_id: "user_123",
       token: TokenIdentity::Sha256Hex(
         "4c281411e1ccc93c230902001a09e7b863cb12a3f3b341089eb980a34aa9e434",
       ),
       workspace: WorkspaceSource {
         tree: TreeSource::GitHub { owner: "rototo", name: "config" },
         revision: TreeRevision::GitRef("main"),
         path: WorkspacePath("."),
       },
     }

Open an existing branch for a nested workspace
  TreeSourceRecord { tree: TreeSource::GitHub { owner: "rototo", name: "config" } }
  TrackedBranchRecord { branch: BranchName("rototo-console/alice/change-checkout") }
  workspace path: WorkspacePath("workspaces/payments")
  -> CachedWorkspaceSource {
       principal_id,
       token,
       workspace: WorkspaceSource {
         tree,
         revision: TreeRevision::GitBranch(
           BranchName("rototo-console/alice/change-checkout"),
         ),
         path: WorkspacePath("workspaces/payments"),
       },
     }

Create a branch from a workspace screen
  1. create TrackedBranchRecord {
       tree_source_id,
       branch,
       base_ref,
       base_commit,
     }
  2. set last_selected_workspace to the current WorkspacePath
  3. set tracking to BranchTrackingState::Active
  4. create CachedWorkspaceSource with TreeRevision::GitBranch(branch)

Reopen a branch after restart
  1. load TrackedBranchRecord by (tree_source_id, branch)
  2. use last_selected_workspace if present
  3. otherwise derive affected workspace paths from branch diff
  4. otherwise ask the user to choose a workspace path
  5. create CachedWorkspaceSource with TreeRevision::GitBranch(branch)

Preview an immutable commit
  TreeSourceRecord + WorkspacePath + GitCommit(
    "8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5",
  )
  -> CachedWorkspaceSource {
       principal_id,
       token,
       workspace: WorkspaceSource {
         tree,
         revision: TreeRevision::GitCommit(
           "8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5",
         ),
         path: workspace_path,
       },
     }

Open a local folder workspace
  TreeSourceRecord {
    principal_id: "local",
    tree: TreeSource::LocalFolder { root: "/srv/repos/config" },
    default_ref: None,
  }
  workspace path: WorkspacePath("workspaces/payments")
  -> CachedWorkspaceSource {
       principal_id: "local",
       token: TokenIdentity::None,
       workspace: WorkspaceSource {
         tree: TreeSource::LocalFolder { root: "/srv/repos/config" },
         revision: TreeRevision::LocalWorkingTree,
         path: WorkspacePath("workspaces/payments"),
       },
     }

Open a staged archive workspace
  TreeSourceRecord {
    tree: TreeSource::Archive {
      url: "https://example.com/releases/config.tar.gz",
    },
    default_ref: None,
  }
  workspace path: WorkspacePath("workspaces/payments")
  fingerprint: SourceFingerprint("sha256:abc123")
  -> CachedWorkspaceSource {
       principal_id,
       token,
       workspace: WorkspaceSource {
         tree,
         revision: TreeRevision::ArchiveSnapshot("sha256:abc123"),
         path: WorkspacePath("workspaces/payments"),
       },
     }
```

Only ingestion and migration code should understand legacy source strings such
as GitHub archive URLs or `git+https://...#ref:path`. Once a source is
registered, API routes should work from normalized `TreeSourceRecord`,
`WorkspacePath`, and `TrackedBranchRecord` values.

The store should not cache discovery products or SDK-derived objects.
Discovered workspace paths, affected workspace paths, changed file paths,
inspected workspaces, semantic models, runtime workspaces, worktree paths,
tempdirs, and revalidation locks remain derived or process-local stage state.

### Source Tree Selection Semantics

`TreeRevision` is the requested lane for reading one source tree. It is
not the resolved physical identity. Stage should keep requested selections and
resolved facts separate.

```rust
pub enum ResolvedTreeSource {
    Git {
        requested: TreeRevision,
        commit: GitCommit,
        worktree: Arc<RepoWorktree>,
    },
    LocalFolder {
        root: PathBuf,
        fingerprint: Option<SourceFingerprint>,
    },
    Archive {
        fingerprint: SourceFingerprint,
        root: PathBuf,
        keep_alive: Arc<TempDir>,
    },
}
```

Rules by revision:

- `GitRef(ref)` is valid for git-backed source trees. It is mutable: a
  refresh may observe a different commit for the same ref. Stage records the
  resolved commit on the `RepoWorktree`, but callers keep requesting the ref.
- `GitBranch(branch)` is valid for git-backed source trees. It is mutable and is
  invalidated after console writes commit new files to that branch. Stage
  resolves the branch to the current branch commit before inspecting
  workspaces.
- `GitCommit(commit)` is valid for git-backed source trees. It is immutable:
  stage should not refresh it in the background. It may be evicted for disk
  pressure and restaged later from the same commit.
- `LocalWorkingTree` is valid only for local-folder source trees. It represents the
  current filesystem contents at the canonical root. Stage may use short TTLs,
  directory mtimes, or source fingerprints to avoid stale views, but local
  folders are not refreshed from a remote authority.
- `ArchiveSnapshot(fingerprint)` is valid for extracted archive contents.
  It is immutable for a staged archive. Refreshing an archive URL belongs to
  source probing; if the bytes change, probing yields a new fingerprint and a
  new source-tree selection.

Route behavior:

- Workspace, branch, and editor routes should normally request `GitRef` or
  `GitBranch`.
- Explicit historical views may request `GitCommit`.
- Local-folder routes request `LocalWorkingTree`.
- Archive routes request `ArchiveSnapshot` after the archive has been
  staged or probed.
- Routes should not rewrite `GitRef` or `GitBranch` into `GitCommit` just because
  stage observed a commit. The observed commit is a resolved fact for caching,
  logs, diagnostics, and stale checks.

Examples:

```text
CachedWorkspaceSource {
  workspace: WorkspaceSource {
    tree: github config,
    revision: TreeRevision::GitRef("main"),
    path: WorkspacePath("workspaces/payments"),
  },
}
  -> stages worktree for main
  -> records resolved commit "a1b2c3d4e5f60718293a4b58d3c4b5a6f7081920"
  -> later refresh may replace it with commit "c3d4e5f60718293a4b58d3c4b5a6f7081920a1b2"

CachedWorkspaceSource {
  workspace: WorkspaceSource {
    tree: github config,
    revision: TreeRevision::GitBranch(
      BranchName("rototo-console/alice/change-checkout"),
    ),
    path: WorkspacePath("workspaces/payments"),
  },
}
  -> stages the branch worktree
  -> branch save calls invalidate_branch(...)
  -> next request restages or reuses the updated branch checkout

CachedWorkspaceSource {
  workspace: WorkspaceSource {
    tree: github config,
    revision: TreeRevision::GitCommit(
      "8d3c4b5a6f7081920a1b2c3d4e5f60718293a4b5",
    ),
    path: WorkspacePath("."),
  },
}
  -> stages that commit
  -> no background refresh

CachedWorkspaceSource {
  workspace: WorkspaceSource {
    tree: local folder /srv/repos/config,
    revision: TreeRevision::LocalWorkingTree,
    path: WorkspacePath("."),
  },
}
  -> reads current local files
  -> may be invalidated by TTL or filesystem fingerprint change
```

### Derived Workspace Discovery

Workspace discovery is derived from a selected source tree checkout. It should
not be durable store state.

```rust
pub struct WorkspaceDiscovery {
    pub cached_tree: CachedTreeSource,
    pub revision: TreeRevision,
    pub workspaces: Vec<WorkspacePath>,
}
```

`discover_workspaces(cached_tree, revision)` scans the selected staged
checkout for `rototo-workspace.toml` and returns source-tree-relative
workspace paths. The source tree root workspace is `"."`.

Rules:

- Discovery works for base refs, branches, commits, local folders, and
  archives.
- Discovery reads the selected source-tree checkout, not the durable store.
- Results may be cached in stage by `(CachedTreeSource, TreeRevision)`.
- Cache entries are invalidated when the selected checkout changes.
- Deleted workspaces disappear from the next discovery result after the source
  tree is refreshed or the branch is invalidated.
- A route with an explicit `WorkspacePath` does not need discovery before
  building a `CachedWorkspaceSource`; discovery is for lists and fallbacks.

Example:

```text
selected checkout:
  rototo-workspace.toml
  workspaces/payments/rototo-workspace.toml
  workspaces/search/rototo-workspace.toml

discover_workspaces(source_tree, TreeRevision::GitRef("main"))
  -> WorkspaceDiscovery {
       workspaces: [
         WorkspacePath("."),
         WorkspacePath("workspaces/payments"),
         WorkspacePath("workspaces/search"),
       ],
     }
```

### Branch Scope In The Store

This is resolved by the target store schema above. `BranchCheckout` belongs to
a source tree, and `TrackedBranchRecord` follows that shape. A branch is not owned by
one workspace. It may remember the last selected workspace for UI navigation,
but it can affect several workspace paths.

The important rules are:

- `TrackedBranchRecord` belongs to `TreeSourceRecord`;
- `TrackedBranchRecord.last_selected_workspace` is optional and is only a
  navigation hint;
- affected workspaces are derived from the branch diff and discovered
  workspace paths;
- changed files are derived from the branch diff;
- current draft rows are migration inputs only.

The remote branch and commits are still authoritative. Store rows help the
console reopen a branch-oriented screen quickly and remember UI navigation.

### Derived Branch Changes

`BranchCheckout` tracks `modified_files` and `modified_workspaces`, but those
facts are derived from git and workspace discovery. The stage method should be
named after the returned data structure:

```rust
pub struct BranchChanges {
    pub branch: BranchName,
    pub base_ref: GitRefName,
    pub changed_files: Vec<RepoRelativePath>,
    pub affected_workspaces: Vec<WorkspacePath>,
}
```

`get_branch_changes(source_tree, branch)` returns a `BranchChanges` value. It
computes the branch diff against `base_ref`, discovers workspaces in the
selected source tree, and maps changed repo-relative files to affected
workspace paths. This is derived data, not durable state.

Initial mapping rule:

- A changed file affects a workspace when it is under that workspace root.
- For the root workspace `"."`, every changed file under the source tree may
  affect the root workspace.
- If a changed file could affect shared or layered behavior and the
  relationship is not known yet, invalidate broadly rather than narrowly.

Shared or layered files need a later dependency-aware pass. A changed file can
affect a workspace when:

- it is a schema referenced by that workspace;
- it is a context/example file used by previews;
- it is a custom lint file loaded by that workspace;
- it belongs to a parent layer used by that workspace;
- it is the workspace manifest.

Example:

```text
selected checkout:
  rototo-workspace.toml
  workspaces/payments/rototo-workspace.toml
  workspaces/payments/variables/checkout.toml
  workspaces/search/rototo-workspace.toml

branch diff:
  workspaces/payments/variables/checkout.toml

get_branch_changes(source_tree, BranchName("rototo-console/alice/change-checkout"))
  -> BranchChanges {
       branch: BranchName("rototo-console/alice/change-checkout"),
       base_ref: "main",
       changed_files: [
         RepoRelativePath("workspaces/payments/variables/checkout.toml"),
       ],
       affected_workspaces: [
         WorkspacePath("."),
         WorkspacePath("workspaces/payments"),
       ],
     }
```

The root workspace is included because its scope is the whole source tree. If
that is too broad for the UI, routes can prefer the most specific affected
workspace first and keep the root as a conservative invalidation target.

### Branch Workspace Fallback

Opening a branch screen needs one workspace path for editor and preview routes.
Because the target store does not persist workspace rows or branch-workspace
rows, routes should choose that workspace path in this order:

1. Use the explicit `WorkspacePath` from the route when present.
2. Otherwise use `TrackedBranchRecord.last_selected_workspace` when present.
3. Otherwise call `get_branch_changes(source_tree, branch)` and use the most
   specific affected workspace when there is a clear choice.
4. Otherwise ask the user to choose a workspace path from
   `discover_workspaces(source_tree, TreeRevision::GitBranch(branch))`.

This keeps `last_selected_workspace` as a UI hint. It does not make the branch
owned by that workspace.

### Layer Dependencies

The SDK can load layered workspaces through `extends`. The console data model
currently identifies only the selected workspace path inside a source tree.

The rewrite needs to decide whether `StagedWorkspace` records dependencies:

```rust
pub struct WorkspaceDependencies {
    pub layer_sources: Vec<SourceLayer>,
    pub local_paths: HashSet<RepoRelativePath>,
}
```

Recording dependencies would make branch invalidation more accurate. Without
it, stage should invalidate more broadly when manifests, shared schemas, or
known layer sources change.

### Non-Git Tree Lifecycle

`LocalFolderTree` and `ExtractedArchiveTree` are named, but their lifecycle
rules are incomplete.

Local folders:

- may not need a tempdir or revalidation task;
- can change underneath the console without a commit;
- are read as local filesystem trees; branch and write workflows are not
  implied by being inside a git repository;
- need path canonicalization and symlink escape protections.

Archives:

- own extracted tempdirs;
- refresh by HTTP validator or content hash;
- do not support branch worktrees unless the console writes elsewhere;
- may contain multiple workspace roots under one extracted tree.

These should share the source-tree abstraction, but their capabilities must be
explicit so routes do not offer branch operations a backing cannot support.

### Locking And Async Boundaries

Stage is an async cache around slow operations: git, filesystem scans, archive
fetch/extraction, SDK inspection, lint, runtime loading, and semantic model
construction. The locking contract should make it hard to hold a mutex while
awaiting those operations.

Lock hierarchy:

```text
StageCache.source_trees map lock
  -> TreeSourceSlot lock
       -> BranchSlot lock
       -> WorkspaceSlot lock
```

Rules:

- Acquire `StageCache.source_trees` only long enough to find or insert an
  `Arc<TreeSourceSlot>`.
- Do not hold the `StageCache.source_trees` lock while awaiting any operation.
- Acquire `TreeSourceSlot` only long enough to find or insert branch,
  workspace, discovery, or refresh state.
- Do not hold `TreeSourceSlot` while running git, fetching archives,
  extracting files, scanning workspaces, loading SDK workspaces, linting, or
  building semantic models.
- Do not hold `BranchSlot` while committing files, fetching/probing a branch,
  computing a branch diff, or notifying LSP sessions.
- Do not hold `WorkspaceSlot` while inspecting a workspace, loading a runtime
  workspace, linting, or building a semantic model.
- Never acquire locks in the reverse order. A workspace operation must not
  reacquire the stage map after it has taken a workspace slot.

The implementation should use short locks to clone handles and then do slow
work outside the lock:

```rust
let source_slot = {
    let mut source_trees = self.source_trees.lock().await;
    source_trees
        .entry(source_tree_key.clone())
        .or_insert_with(|| Arc::new(Mutex::new(StagedTreeSource::new(...))))
        .clone()
};

let workspace_slot = {
    let mut source = source_slot.lock().await;
    source
        .workspace_views
        .entry(selector_key.clone())
        .or_insert_with(|| Arc::new(Mutex::new(StagedWorkspace::new(...))))
        .clone()
};

// No StageCache or StagedTreeSource lock is held here.
let inspected = initialize_inspected_workspace(workspace_slot).await?;
```

#### Single-Flight Initialization

Derived data should be initialized once per cache key even under concurrent
requests:

- `WorkspaceDiscovery` is single-flight per
  `(CachedTreeSource, TreeRevision)`.
- `BranchChanges` is single-flight per `(CachedTreeSource, BranchName)` and
  invalidated after branch writes.
- inspected workspace is single-flight per `WorkspaceViewKey`;
- semantic model is single-flight per `WorkspaceViewKey`;
- runtime workspace is single-flight per `WorkspaceViewKey`;
- base-ref refresh is single-flight per `(CachedTreeSource, BaseRef)`.

Use `tokio::sync::OnceCell`, a small in-flight task map, or a local
single-flight helper. The important property is that ten concurrent requests
for the same semantic model wait on one initializer instead of building ten
models.

Single-flight state is process-local. It should not be persisted.

#### Invalidation During In-Flight Work

Invalidation should be generation-based, not cancellation-based. Long-running
requests may finish after a branch save or refresh has invalidated their cache
entry. Those requests can still return their own `Arc<Workspace>` safely, but
they must not republish stale values into the current cache generation.

Each invalidatable entry should carry a generation:

```rust
pub struct CacheGeneration(u64);

pub struct StagedWorkspace {
    pub key: WorkspaceViewKey,
    pub generation: CacheGeneration,
    // ...
}
```

Initialization flow:

1. Read the current generation under a short lock.
2. Clone the physical backing needed to keep files alive.
3. Drop locks and run the slow initializer.
4. Reacquire the slot.
5. Store the result only if the generation is unchanged.
6. If the generation changed, return the freshly built value to the caller but
   do not install it as the cached value.

Branch invalidation increments the generation for affected workspace views and
drops branch-change/discovery entries derived from that branch. Existing
requests and LSP sessions keep their old `Arc<Workspace>` handles alive until
they are dropped.

#### Background Refresh Scheduling

Background refresh should be stale-while-revalidate and single-flight:

- stale entries are returned immediately;
- if no refresh is running, stage records `RevalidationState::Running` under a
  short lock and spawns one refresh task;
- if a refresh is already running, the request does not spawn another one;
- successful refresh installs a new worktree/workspace view only after staging
  and inspection succeed;
- failed refresh records/logs the error and keeps the last known good entry;
- refresh tasks must not hold stage locks while doing remote or filesystem I/O.

Refresh tasks should clone only the data they need: normalized source tree key,
selection, token identity or token handle, and physical backing handles. They
should not borrow route state or hold request-scoped references.

#### LSP And Invalidation

LSP sessions hold inspected workspace handles. Invalidating a branch should:

- remove future cache hits for workspace views derived from that branch;
- ask the LSP session registry to drop sessions for that branch/workspace
  selection;
- avoid waiting for LSP shutdown while holding stage locks.

The order should be: compute affected workspace keys, update stage cache
generations/drop entries under short locks, release locks, then notify LSP.

### Eviction And Disk Limits

The current stage cache is process-lifetime. A source-tree/local-tree model can
retain more state: bare repos, multiple worktrees, branch worktrees, semantic
models, and runtime workspaces.

The rewrite needs an explicit initial policy:

- either no eviction in the first implementation, documented as process-lifetime
  cache behavior;
- or simple idle eviction for source trees/worktrees;
- or explicit invalidation only.

If eviction is deferred, the code should still centralize ownership so adding
eviction later does not require redesigning every handle.

### Capability Model

The model implies different capabilities by backing:

- GitHub-backed trees can fetch refs, create branch checkouts, commit through
  the GitHub API, and open pull requests;
- generic git remotes can fetch refs and inspect workspaces, but should not
  expose writes unless the console write design is reopened;
- local folders can inspect current files;
- archives can inspect and refresh but cannot be edited directly.

These capabilities should be derived close to `LocalTree` so API routes do not
duplicate source-kind checks. They do not need to be durable store fields in
the first design.

Field meanings:

- `can_refresh`: stage can check for newer source contents.
- `can_branch`: console can materialize a branch checkout for this tree.
- `can_write`: console can commit changes back to the source-of-record tree.
- `can_open_pull_request`: console can create or link a PR for a branch.

This should line up with the existing console write policy rather than replace
it.

### Error And Observability Shape

The data model does not yet define how staging errors are attributed:

- source tree fetch/update failure;
- worktree checkout failure;
- workspace inspection failure;
- semantic model failure;
- runtime load/lint failure;
- branch worktree failure.

Errors should carry enough context to log and show useful API messages without
leaking tokens or internal temp paths unnecessarily.

### Resolution Order

Resolve the gaps in this order:

1. Identity normalization.
2. Target store schema.
3. Store-to-stage selector mapping.
4. Source tree selection semantics.
5. Derived workspace discovery.
6. Derived branch changes.
7. Locking and async boundaries.
8. Non-git lifecycle and capabilities.
9. Eviction policy.
10. Error and observability shape.

The first four should be settled before implementing structs. Current
draft-named rows can be migration inputs, but the rewrite should not preserve
their workspace-scoped shape as an internal stage constraint.

## Proposed Console Interface

The current interface available inside `src/console` is `StageCache`, stored on
`ConsoleState` as `state.stage`. Today its methods are named `inspect`,
`semantic_model`, and `runtime`.

A rewrite should use names that say which staged view the caller receives and
should select workspaces by source tree plus workspace path. The runtime case
especially benefits from explicit naming, because it returns the same
`Arc<Workspace>` type as inspection but with a compiled runtime model inside.

```rust
pub struct StageCache;

pub struct WorkspaceSource {
    pub tree: TreeSource,
    pub revision: TreeRevision,
    pub path: WorkspacePath,
}

pub struct CachedWorkspaceSource {
    pub principal_id: String,
    pub token: TokenIdentity,
    pub workspace: WorkspaceSource,
}

pub enum TreeRevision {
    GitRef(GitRefName),
    GitBranch(BranchName),
    GitCommit(GitCommit),
    LocalWorkingTree,
    ArchiveSnapshot(SourceFingerprint),
}

pub struct WorkspaceDiscovery {
    pub cached_tree: CachedTreeSource,
    pub revision: TreeRevision,
    pub workspaces: Vec<WorkspacePath>,
}

pub struct BranchChanges {
    pub branch: BranchName,
    pub base_ref: GitRefName,
    pub changed_files: Vec<RepoRelativePath>,
    pub affected_workspaces: Vec<WorkspacePath>,
}

impl StageCache {
    pub fn new() -> Self;

    pub async fn discover_workspaces(
        &self,
        cached_tree: CachedTreeSource,
        revision: TreeRevision,
    ) -> Result<WorkspaceDiscovery>;

    pub async fn get_branch_changes(
        &self,
        cached_tree: CachedTreeSource,
        branch: BranchName,
    ) -> Result<BranchChanges>;

    pub async fn get_inspected_workspace(
        &self,
        selector: CachedWorkspaceSource,
        source_token: &str,
    ) -> Result<Arc<Workspace>>;

    pub async fn get_semantic_workspace(
        &self,
        selector: CachedWorkspaceSource,
        source_token: &str,
    ) -> Result<SemanticWorkspace>;

    pub async fn get_runtime_workspace(
        &self,
        selector: CachedWorkspaceSource,
        source_token: &str,
    ) -> Result<Arc<Workspace>>;

    pub async fn invalidate_workspace(&self, selector: CachedWorkspaceSource);
    pub async fn invalidate_branch(&self, cached_tree: CachedTreeSource, branch: BranchName);
}
```

The methods are all async because staging may perform filesystem I/O, run git,
fetch archives over the network, extract archives, run lint, or build a
semantic model.

Routes may keep source-string helpers while the code migrates, but those
helpers should normalize the route inputs into `CachedTreeSource`,
`WorkspaceSource`, and `TreeRevision` before touching cache state.

## Method Semantics

### `discover_workspaces`

`discover_workspaces(source_tree, selection)` returns workspace paths found in
one selected source tree checkout. It scans for `rototo-workspace.toml` and
returns a `WorkspaceDiscovery` value. This is the source for workspace lists
and branch fallback choices; it is not durable store state.

Current callers should use it for:

- source tree workspace lists;
- branch screens when `last_selected_workspace` is missing;
- branch change mapping before computing affected workspaces;
- validation that a route-provided `WorkspacePath` still exists.

### `get_branch_changes`

`get_branch_changes(source_tree, branch)` returns a `BranchChanges` value for
one tracked or selected branch. It computes changed files from the branch diff
against the branch's `base_ref`, then maps those files to affected workspaces
using discovered workspace paths.

The name is intentionally not `get_branch_impact`: the method returns a data
structure with concrete changed files and affected workspace paths. Routes can
decide how to present or act on those changes.

Current callers should use it for:

- invalidating workspace views after a branch save;
- choosing a branch screen fallback workspace;
- showing changed files and affected workspaces in branch UI;
- publish checks that need to lint affected workspaces.

### `get_inspected_workspace`

`get_inspected_workspace(selector, source_token)` returns an `Arc<Workspace>`
for one workspace path inside a staged source tree. The workspace has been
inspected, but not lint-gated. Callers can read files through
`workspace.root()` and can call `workspace.lint().await` themselves.

`selector` carries `TokenIdentity` so cache keys stay scoped to the token used
to access a source. `source_token` is the raw bearer token used only when the
stage has to perform a cold source load.

This is the permissive mode used by editing and diagnostics surfaces. Broken
workspaces should still be inspectable when possible so the console can show
lint errors and let users fix files.

Current callers use it for:

- workspace lint routes;
- branch publish lint checks;
- LSP-backed editor update, completion, and hover routes;
- branch reads after selecting the branch worktree.

### `get_semantic_workspace`

`get_semantic_workspace(selector, source_token)` returns the inspected
workspace plus a `WorkspaceSemanticModel`, grouped in `SemanticWorkspace`.

The semantic model should be computed at most once per staged workspace view.
Multiple routes use it for inventory, entity lookup, edit screens, and preview
setup. It is expensive enough that every request should not rebuild it when the
underlying staged files have not changed.

The returned `Workspace` and model must describe the same staged root.

The name is intentionally not `get_semantic_model`: callers almost always need
the staged root and the model together. Returning a named structure makes that
relationship harder to accidentally break than returning a tuple.

### `get_runtime_workspace`

`get_runtime_workspace(selector, source_token)` returns a lint-gated,
runtime-capable `Workspace` for the selected workspace path. This is the
workspace used for resolution previews. If lint fails, this method should fail
the same way `Workspace::load` fails.

Runtime preview has stricter semantics than inspection: applications can only
resolve values from workspaces that pass lint and compile a runtime model. The
console should not preview values from a workspace state an application could
not load.

The runtime workspace must keep any inspected/staged backing files alive. A
common shape is to build the runtime workspace from the inspected root and keep
the inspected `Arc<Workspace>` inside the cached entry.

Current callers use it for:

- saved-context previews on workspace entity pages;
- saved-context previews on branch entity pages;
- fallback from a dirty branch worktree to the base workspace when the branch is
  not lint-clean.

### Invalidation

`invalidate_workspace(selector)` drops cached inspected, semantic, and runtime
state for one workspace path/source-tree selection. It does not need to cancel
requests already holding `Arc<Workspace>` handles; those handles should continue to
keep their temporary files alive until they are dropped.

`invalidate_branch(tree, branch)` drops the branch checkout's workspace caches
and metadata. Branch saves call this after committing to the branch. The LSP
registry is also told to drop sessions for the branch, because an LSP session
owns the old staged workspace handle.

Invalidation should be tree/path/selection-based rather than route-based. A
save does not know which cached view kinds have been created for the branch,
but the branch checkout state knows which repo-relative files and workspace
paths changed.

## Source Handling

The stage layer starts from the source location registered in the console:
usually a repository plus a base ref, but sometimes a local folder or archive.
It also receives a bearer token string when remote reads need auth. Empty token
means no auth.

Cache keys, logs, and diagnostics must not contain raw bearer tokens. The
current implementation uses a short SHA-256-derived token key for cache
identity. A replacement can choose a different representation, but it should be
stable within the process and non-reversible enough for logs and debug output.

The rewrite should share source behavior with `src/source.rs` as much as
possible. The canonical behavior there already covers:

- local paths and `file://` sources;
- `git+file://`, `git+https://`, and `git+ssh://`;
- HTTPS tar archives;
- rejection of plain `http://`;
- git refs and archive subdirectory fragments;
- archive size and extraction limits;
- workspace layering through `extends`;
- source fingerprints and immutable pinned commit detection.

A replacement should not fork source parsing, archive safety, fingerprinting,
immutable-ref detection, or workspace layering inside `src/console/stage/`.
Where the current source module is too workspace-root-oriented for console's
tree use case, factor reusable tree-staging helpers into `src/source.rs`
instead of copying behavior into the console.

### Loader-Backed Staging

Once the console has selected a concrete workspace root inside a staged tree,
inspected workspaces should be loaded with
`Workspace::inspect_with_source_options(root, options)`.

Runtime workspaces should be loaded from the inspected staged root with
`Workspace::load(...)` or the equivalent lint-deny SDK path. Loading from the
staged root avoids fetching the source twice and keeps runtime preview tied to
the same files the console used for inventory and semantic analysis.

This preserves the boundary: console stage owns source-tree manifestation and
workspace-path selection; the SDK owns workspace inspection, lint, runtime
loading, context validation, and resolution.

### Git Sources

Git workspace sources have the form:

```text
git+file://<repo>#<ref>:<subdir>
git+https://<repo>#<ref>:<subdir>
git+ssh://<repo>#<ref>:<subdir>
```

The fragment parts are optional:

- no fragment means `HEAD` at the workspace root;
- `#ref` selects a ref at the workspace root;
- `#ref:subdir` selects a ref and workspace subdirectory;
- `#:subdir` selects `HEAD` and a workspace subdirectory.

The SDK/source loader resolves refs to commit SHAs and exposes the source
fingerprint through `Workspace::source_fingerprint()`. Full 40-character
commit refs are treated as immutable and exposed through
`Workspace::immutable_source()`, so they do not need periodic refresh.

Bearer auth for HTTPS git sources is passed to git through a temporary
`GIT_ASKPASS` helper. Git process environment variables are scrubbed before
running git so ambient `GIT_DIR`, `GIT_WORK_TREE`, and similar variables cannot
change staging behavior.

The current implementation keeps a bare git repository cache per
`(token hash, remote)` and creates per-ref checkouts from that bare repository.
That is an optimization, not the core contract. Do not carry it into the first
rewrite. The core contract is that a request for a git source returns a
workspace rooted at the selected ref and subdirectory, with temporary files
kept alive by the returned handle.

### HTTPS Archives

HTTPS archive sources are treated as gzip-compressed tar archives. The archive
fragment supports only `#:subdir`; archive refs are invalid.

Required behavior:

- use bearer auth when a token is present;
- reject non-success HTTP responses with a useful error;
- enforce configured archive size, decompressed size, and entry count limits;
- reject unsafe archive paths such as absolute paths or `..` components;
- extract only regular files and directories;
- do not allow archive contents to write outside the extraction directory;
- choose the workspace root from the requested subdirectory, an archive root
  containing `rototo-workspace.toml`, or a single wrapper directory containing
  `rototo-workspace.toml`.

The SDK/source loader fingerprints archives with `ETag`, then
`Last-Modified`, then a content SHA-256 when no validator header is present.
The console stage rewrite should rely on that behavior rather than duplicating
it.

## Workspace Root Selection

When a remote artifact contains more than exactly the workspace directory, the
loader must pick the root the console should inspect. A rewrite should inherit
this from `src/source.rs`; this section records the behavior to preserve, not a
separate console implementation to create.

For explicit subdirectories:

- the subdirectory must be a relative path made only of normal path
  components;
- absolute paths, empty paths, `.` and `..` components should be rejected;
- the selected path must exist, be a directory, and remain inside the staged
  artifact after canonicalization;
- if an archive has a single wrapper directory, selecting the subdirectory
  inside that wrapper is allowed.

For implicit roots:

- prefer the artifact root if it contains `rototo-workspace.toml`;
- otherwise, if the artifact has exactly one child directory and that directory
  contains `rototo-workspace.toml`, use that child;
- otherwise use the artifact root and let workspace inspection report any
  missing manifest or parse errors.

## Caching Semantics

The current cache has a 30 second freshness window and serves stale data while
refreshing in the background. A replacement should preserve the user-facing
property: hot requests return quickly, and stale cached content is refreshed
without making every caller block on remote I/O.

The important concepts are:

- staged source tree entries are keyed by user/auth identity and source tree;
- workspaces are keyed by source-tree-relative workspace path and source-tree
  selection;
- semantic models belong to a specific inspected workspace root;
- runtime workspaces belong to a specific inspected workspace root;
- temporary files are owned by cached entries and by any returned `Arc`;
- immutable pinned sources do not need background refresh;
- explicit branch invalidation removes workspace views derived from that
  branch worktree immediately.

The current implementation separates cache state into view entries, artifact
entries, and bare git repository entries. That shape should not survive the
first rewrite. Start with staged source trees that own branch checkout slots and
workspace-view slots.

The useful cache hierarchy is:

- `StageCache`: owns `CachedTreeSource -> StagedTreeSource`;
- `StagedTreeSource`: owns the local tree plus branch checkout and
  workspace-view slots;
- `StagedWorkspace`: owns derived workspace views for one path/source-tree
  selection pair;
- `BranchCheckout`: owns the branch worktree and changed path metadata;
- `SemanticWorkspace`: is only a return object.

Everything else should be a field, helper function, or private enum only after
it proves it makes the code easier to read.

```text
StageCache
  CachedTreeSource -> StagedTreeSource

StagedTreeSource
  WorkspaceViewKey -> StagedWorkspace
  BranchName -> BranchCheckout

StagedWorkspace
  inspected: OnceCell<Arc<Workspace>>
  semantic: OnceCell<Arc<WorkspaceSemanticModel>>
  runtime: OnceCell<Arc<Workspace>>
```

With that shape:

1. `get_inspected_workspace` finds or creates the selected
   `StagedWorkspace`, then initializes `workspace.inspected`.
2. `get_semantic_workspace` initializes `workspace.semantic` from the inspected
   workspace. On a cold cache, it passes the raw source token through to
   inspection; the semantic model is still built from the inspected root.
3. `get_runtime_workspace` initializes `workspace.runtime` from
   `Workspace::load(workspace.inspected.root())` and keeps the inspected
   backing alive.
4. after the freshness window, a request returns the current source
   tree/workspace and starts one background revalidation task if no
   revalidation is already running.
5. a successful revalidation swaps in a new worktree or workspace view; failed
   revalidation keeps the old one.

This is not the only valid design. The point is to make the lifetime and cache
ownership obvious before adding lower-level optimizations.

## Refresh Behavior

Refresh should be stale-while-revalidate:

- a fresh source tree/workspace is returned directly;
- a stale source tree/workspace is returned directly, and one background
  revalidation is scheduled;
- concurrent requests should not start duplicate refreshes for the same cache
  key;
- a revalidation error should not evict the last known good source tree or
  workspace;
- a successful revalidation should replace a worktree or workspace view only
  after the new manifestation stages and inspects successfully;
- semantic and runtime caches must be reset when the staged files change.

Refresh follows `TreeRevision` semantics:

- `GitRef(ref)`: eligible for background refresh. Probe/fetch the ref, compare
  the resolved commit with the cached `RepoWorktree`, and replace the worktree
  only after the new commit stages and inspects successfully.
- `GitBranch(branch)`: not refreshed on a timer in the first design. Console
  writes call `invalidate_branch(...)`; explicit user actions may fetch/probe
  the branch before restaging.
- `GitCommit(commit)`: never refreshed. It is immutable and can only be evicted
  and restaged from the same commit.
- `LocalWorkingTree`: local-folder only. Revalidate by filesystem probing,
  fingerprinting, or short TTL. There is no remote fetch.
- `ArchiveSnapshot(fingerprint)`: never refreshed. Archive URL probing that
  discovers new bytes should produce a new fingerprint and therefore a new
  selection/cache entry.

The current code uses fingerprints to avoid rebuilding views when git refs or
archives are unchanged. A source-tree-first replacement can check a repo ref's
commit or an archive fingerprint before rebuilding workspace views. Prefer shared
`src/source.rs` probe/fingerprint behavior over console-specific parsing.

Refresh is not persistence. Restarting the console drops all stage cache state.

## Invalidation Semantics

Branch invalidation must remove cached workspace views derived from the branch
worktree. It should also make the next LSP request build a new session against
the new staged root.

The current invalidation function matches broad source markers:

- direct source: the original source string;
- git source: `remote#ref`, ignoring subdir and token;
- archive source: archive URL, ignoring subdir and token.

The replacement should preserve the useful property, not the mechanism: one
branch save invalidates every workspace view affected by that branch. It
should avoid substring matching. Prefer explicit keys: `BranchName`,
`WorkspacePath`, and repo-relative changed files.

The git repository fetch cache, if one exists, does not need to be invalidated
on branch saves. It is only a fetch optimization. The next stage should fetch
or probe the branch and observe the new commit.

## Lifetime Invariants

The returned `Arc<Workspace>` is more than parsed metadata. It owns or keeps
alive the staged filesystem root used by API routes and LSP sessions.

A correct implementation must preserve these invariants:

- `workspace.root()` remains valid until the last returned `Arc<Workspace>` is
  dropped;
- a semantic model must not outlive the staged files it describes unless the
  model is fully self-contained and no caller uses it with old paths;
- runtime preview must keep the inspected source root alive when it was built
  from that root;
- LSP sessions must hold the inspected workspace handle for the session
  lifetime;
- invalidating a cache entry must not delete temp files still held by active
  requests or sessions.

Using `Arc<Workspace>` as the public handle is useful because the SDK
`Workspace` already owns its staged source lifetime.

## Current Consumers

Workspace routes use staging as follows:

- workspace lint: `get_inspected_workspace` then `lint`;
- workspace summaries: `get_semantic_workspace` to build inventory counts;
- workspace data: `get_semantic_workspace`, `lint`, and inventory;
- workspace entity: `get_semantic_workspace` to read definitions and contexts,
  then `get_runtime_workspace` for saved-context previews when lint-clean.

Current branch routes, some of which are still draft-named in the API, use
staging as follows:

- branch saves call `invalidate_branch(tree, branch)` after committing;
- branch lint and publish checks call `get_inspected_workspace` on branch
  selectors;
- branch LSP operations call `get_inspected_workspace` and pass the
  handle into `LspSessions`;
- branch screen data calls `get_semantic_workspace` with a branch
  selector;
- branch entity previews call `get_semantic_workspace` with a branch
  selector, then `get_runtime_workspace` on the branch worktree or fall back to
  `get_runtime_workspace` on the base worktree.

Repository registration warms newly discovered workspaces by calling
`get_semantic_workspace` in a background task. Warm-up failures are logged but
do not fail registration.

## Errors

Stage errors cross API boundaries as console API errors. They should be useful
to an engineer looking at a workspace screen.

Important error cases include:

- unsupported or invalid workspace source URI;
- unsupported `http://` source;
- git missing from `PATH`;
- git ref not found;
- git ref beginning with `-`;
- git command timeout or failure;
- archive fetch failure or non-success HTTP status;
- archive too large, too many entries, or too large after decompression;
- unsafe archive paths;
- requested subdirectory missing, not a directory, or escaping the artifact;
- workspace inspection, lint, semantic model, or runtime load failure.

Background refresh errors should be logged and should keep the last good entry.
Foreground first-load errors should be returned to the caller.

## What Not To Carry Forward Blindly

The current implementation has useful behavior but too many concepts at once:
parsed console sources, artifact slots, view slots, git repo slots, artifact
handles, view stages, refresh enums, and duplicated git/archive staging. A
rewrite should remove that conceptual load. Start from the console API
contract, a source-tree cache, and workspace views that delegate semantics to
the SDK. Add lower-level caches only when there is a measured problem and a
small testable abstraction to hold the optimization.

Use these simplifications as design constraints:

- use shared SDK/source code as the implementation authority for source forms,
  archive safety, fingerprints, immutable refs, workspace layering, and
  workspace semantics;
- model one staged source tree per `(principal, source tree, token identity)`;
- store explicit cache keys and invalidation keys instead of searching strings
  with `contains`;
- keep semantic and runtime caches inside `StagedWorkspace` so their
  relationship to the workspace root is obvious;
- avoid introducing separate artifact, view, and git repository stores in the
  first rewrite; use the tree/worktree/workspace hierarchy instead;
- treat git bare-repo sharing across subdirectories as an optional performance
  layer, not a design constraint.

The first replacement should make it easy to answer these questions by reading
one type:

- what source tree does this entry represent?
- which staged root do callers see?
- who owns the temp directory?
- is a refresh already running?
- what happens if refresh fails?
- how are semantic and runtime views tied to the workspace root?

Once those answers are obvious, optimizations can be added deliberately.

## Rewrite Test Checklist

A replacement should have focused tests around behavior rather than current
internal types:

- a source tree can expose multiple discovered workspace paths;
- a workspace path inside a tree loads a root that can be linted, and
  `get_semantic_workspace` reuses the same inspected root;
- a lint-broken workspace can still be inspected, while
  `get_runtime_workspace` fails;
- runtime preview keeps the inspected staged root alive for as long as the
  runtime handle exists;
- repeated `get_semantic_workspace` calls for an unchanged entry compute the
  model once;
- stale source-tree/workspace revalidation returns the old entry immediately and
  replaces it only after a successful restage;
- revalidation failure leaves the last known good source-tree/workspace active;
- explicit branch invalidation makes the next call restage the affected branch
  workspaces and rebuild semantic and runtime state;
- git-backed trees select the requested ref and workspace path;
- full commit git refs are treated as immutable;
- archive sources reject unsafe paths and select wrapper-directory roots;
- cache keys distinguish different bearer tokens without storing the raw token;
- repo-level branch work can mark multiple workspace paths as affected when
  files under those paths change;
- branch invalidation clears every cached workspace view for that branch, not
  just the view kind used by the route that saved the branch;
- deleting the console database/cache and reconnecting to the same repo can
  rediscover existing branch work from git branches and PRs;
- LSP session reuse notices when a new inspected workspace handle or root has
  replaced the old one.
