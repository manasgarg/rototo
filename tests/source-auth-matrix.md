# Source loading and auth test matrix

Package sources are rototo's security boundary: every load starts with a
source string, and everything the loader does with it (parsing, staging,
extends resolution, credentials) decides what bytes end up trusted as
configuration. This file is the executable inventory of those promises, in
the same Given / When / Then form as `tests/composition-matrix.md`. A GAP row
is a promise we currently keep only by reading the code; a "pinned by"
row is a deliberate non-test with its verification note.

The source of truth is `src/source/`: `uri.rs` (grammar), `load.rs`
(dispatch), `local.rs` / `git.rs` / `archive.rs` (staging), `auth.rs`
(credentials), and `layer/graph.rs` (the extends graph). Merge semantics
after staging belong to `tests/composition-matrix.md`, not here.

## 1. Source grammar

| # | Given a source string... | Then... | Coverage |
|---|---|---|---|
| U1 | with no `://` | it is a local path, not a URI | `source_uri_rejects_malformed_uris` (`src/source/uri.rs`) |
| U2 | `://host`, `https://`, `https://#main` | parsing fails as an invalid URI | `source_uri_rejects_malformed_uris` |
| U3 | any supported form (`file://`, `git+file://`, `git+https://`, `git+ssh://`, `https://`), with `#ref:subdir` and `#:subdir` fragments | parsing yields scheme, base, ref, and subdir | `source_uri_accepts_supported_source_forms` (`src/source/uri.rs`) |
| U4 | `http://` or `git+http://` | staging is refused with a pointer to `https://`: unencrypted config transport is off on purpose | `stage_package_source_rejects_http`, `stage_package_source_rejects_git_http` (`src/source/load.rs`) |
| U5 | `file://` with any `#` fragment | staging is refused: `file://` is just a folder and takes no extras | `file_sources_reject_fragments` (`src/source/load.rs`) |
| U6 | a git ref that begins with `-` | refused before git ever runs (argument-injection guard) | `stage_package_source_rejects_leading_dash_git_refs_before_running_git` (`src/source/load.rs`) |
| U7 | a git ref of forty hex characters | treated as an immutable commit; anything shorter is a moving ref | `full_git_commit_detection_requires_forty_hex_characters` (`src/source/git.rs`) |
| U8 | `git+file://` with `#ref:subdir` | the SDK stages the named ref and reads the package from the subdirectory | `package_sdk_loads_git_file_source_with_ref_and_subdir` (`tests/sdk.rs`) |

## 2. Archive staging

Archives arrive as untrusted bytes; extraction is where a hostile tarball
would strike.

| # | When the archive... | Then... | Coverage |
|---|---|---|---|
| A1 | contains `..` or absolute entry paths | extraction is refused | `extract_archive_rejects_unsafe_paths` (`src/source/archive.rs`) |
| A2 | contains device nodes, fifos, or other special entries | extraction is refused | `extract_archive_rejects_special_entries` |
| A3 | contains symlink or hardlink entries | the entries are skipped, never followed | `extract_archive_skips_link_entries` |
| A4 | contains pax/metadata entries | they are skipped silently | `extract_archive_skips_metadata_entries` |
| A5 | decompresses past the size limit | extraction is refused (zip-bomb guard) | `extract_archive_rejects_decompressed_size_over_limit` |
| A6 | has more entries than the limit | extraction is refused | `extract_archive_rejects_entry_count_over_limit` |
| A7 | wraps the package in a single top-level directory | subdir selection falls back into the wrapper | `select_archive_subdir_falls_back_to_single_wrapper_directory` |
| A8 | is fingerprinted for change probes | content hashing uses a stable sha256; HTTP validators prefer ETag, then Last-Modified | `content_hash_fingerprint_uses_stable_sha256_digest`, `http_validator_fingerprint_prefers_etag`, `http_validator_fingerprint_uses_last_modified_without_etag` |

## 3. Bearer tokens: one grammar, scoped delivery

Entry grammar (shared verbatim between the repeatable `--package-token` flag
and whitespace-separated `ROTOTO_PACKAGE_TOKEN`):

| # | Given the entries... | Then... | Coverage |
|---|---|---|---|
| G1 | none | requests are anonymous | `no_entries_is_anonymous` (`src/source/auth.rs`) |
| G2 | one bare token | single-origin sugar: `SourceAuth::Bearer` | `one_bare_entry_is_the_single_origin_sugar` |
| G3 | a bare token ending in `=` base64 padding | still one bare token: only a leading `https://` makes an entry scoped, never the `=` sign | `base64_padding_never_makes_a_bare_token_scoped` (`src/source/auth.rs`), `package_token_accepts_base64_padded_bare_tokens` (`tests/cli.rs`) |
| G4 | `https://...=TOKEN` entries | each splits at the first `=` into prefix and token | `https_entries_split_at_the_first_equals` |
| G5 | a prefix with no `=TOKEN` | hard error | `scoped_prefix_without_token_is_an_error` (`src/source/auth.rs`), `package_token_prefix_without_a_token_is_rejected` (`tests/cli.rs`) |
| G6 | a bare token mixed with scoped entries | hard error: no unambiguous recipient | `mixing_bare_and_scoped_entries_is_an_error` (`src/source/auth.rs`), `package_token_rejects_mixing_bare_and_scoped_entries` (`tests/cli.rs`) |
| G7 | two bare tokens | hard error | `two_bare_entries_are_an_error` |
| G8 | duplicate prefixes | hard error | `duplicate_prefixes_are_rejected` |
| G9 | a malformed prefix (non-https scheme, empty host, userinfo, wildcard) | hard error naming the entry | `prefix_validation_rejects_bad_shapes` |
| G10 | the same entries via flags and via the environment variable | one grammar: flag and env produce identical auth | `package_token_entries_share_one_grammar_across_flag_and_env`, `package_token_env_takes_whitespace_separated_scoped_entries` (`tests/cli.rs`) |

Prefix matching:

| # | Given scoped tokens, when a request URL... | Then... | Coverage |
|---|---|---|---|
| M1 | matches several prefixes | the longest prefix wins | `longest_matching_prefix_wins` |
| M2 | continues a prefix without a path boundary (`/team` vs `/teammate`) | no match: prefixes match whole path segments | `prefixes_match_whole_path_segments` |
| M3 | matches no prefix | the request is anonymous: a secret never travels to a host you did not name | `no_matching_prefix_means_anonymous`, `scoped_tokens_attach_on_match_and_stay_silent_otherwise` (`src/source/archive.rs`) |
| M4 | differs only in host case or an explicit `:443` | it still matches: scheme/host lowercase, default port elided | `matching_normalizes_host_case_and_default_port` |
| M5 | uses a non-default port | it is a distinct origin and needs its own entry | `non_default_ports_are_distinct_origins` |

Delivery on the wire:

| # | When a request goes out... | Then... | Coverage |
|---|---|---|---|
| W1 | with no auth configured | no `Authorization` header | `anonymous_requests_carry_no_authorization_header` (`src/source/archive.rs`) |
| W2 | with a bare token | the header is attached and the token binds to that first archive origin; a later request to a second origin fails the load naming it | `a_bare_token_attaches_and_binds_to_the_first_origin` (`src/source/archive.rs`), `bare_token_binds_to_the_first_origin`, `clones_share_the_origin_binding` (`src/source/auth.rs`) |
| W3 | with scoped tokens | the matching token attaches; an unmatched URL sends nothing | `scoped_tokens_attach_on_match_and_stay_silent_otherwise` |
| W4 | and the server answers 401/403 | the error names the credential that was sent (bare, scoped-to-prefix, none, or no-entry-matched) and suggests the entry to add; non-auth statuses get no hint | `auth_failure_hints_name_the_credential_that_was_sent` |
| W5 | and the server redirects cross-origin | the `Authorization` header does not follow | pinned by verified reqwest 0.12.28 source (`remove_sensitive_headers` via `TowerRedirectPolicy::on_request`), recorded in the `apply_archive_auth` doc comment; an end-to-end test needs two local TLS servers and plain `http://` is refused by design |
| W6 | and the server redirects to a non-https URL, or more than 10 times | the redirect is refused | pinned by `https_only_redirect_policy` (a private closure; same TLS-harness limitation as W5) |
| W7 | to a git source | no token is ever attached: git authenticates itself (SSH keys, credential helpers) | by construction: `apply_archive_auth` is called only from the two archive request sites in `src/source/archive.rs`; `src/source/git.rs` has no auth path |

## 4. The extends graph

Staging composes a graph of sources before any merge happens. The merge rows
live in `tests/composition-matrix.md`; these rows are about which sources the
graph will follow at all.

| # | Given / When | Then | Coverage |
|---|---|---|---|
| E1 | a local package extends a git source | the parent stages through the same pipeline and composes | `a_local_package_can_extend_a_git_source` (`tests/composition.rs`) |
| E2 | a staged (fetched) package extends `file://` or `git+file://`, an absolute path, or a `../` that climbs out of its checkout | the load fails with "escapes a staged package": a remote package must not read the loading machine's disk | `a_staged_package_may_not_extend_a_local_git_source` (`tests/composition.rs`), `staged_extend_base_rejects_local_filesystem_escape_sources` (`src/source/layer/mod.rs`) |
| E3 | two packages extend each other, directly or through a chain | the load fails with the cycle spelled out | `extends_cycles_fail_the_load`, `a_package_extending_itself_fails_the_load` (`tests/composition.rs`) |
| E4 | an extends chain deeper than 32 | the load fails naming the depth limit | `extends_chains_deeper_than_the_limit_fail_the_load` (`tests/composition.rs`) |
| E5 | relative extends entries | they resolve against the extending package, not the working directory | `a_three_deep_chain_composes_bottom_up` and the rest of `tests/composition.rs`, which build every fixture this way |

Note for the review pass: the package-sources reference says "every format
works" in `extends` and lists only `file://`, absolute paths, and escaping
`../` as staged-package escapes. E2 shows `git+file://` is also (correctly)
refused from a staged package; the docs bullet should name it. Remote
parents over real `git+https://` / `git+ssh://` are exercised only at the
unit level (E2's pass-through arm); a true end-to-end needs a git server.

## Current gap tally

0 GAP rows. Two deliberate non-tests, W5 and W6, carry their verification
notes inline; one docs follow-up is recorded above for the review pass.

When you add or change source or auth behavior, add the row and the test
together; an empty Coverage cell is a regression in this file's contract.
