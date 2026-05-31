# Design: Semantic Index and Multi-Stage Lint Pipeline

Status: proposed
Scope: `src/lint.rs`, `src/lua_lint.rs`, `src/diagnostics.rs`,
`src/catalog.rs`, `src/model.rs`, `src/workspace.rs`, `src/output.rs`,
`src/sdk.rs`, `src/main.rs`, lint fixtures, docs, and future LSP support.

The diagnostic contract starts with one stable `rule`, a severity, a location,
a message, and help text. This design says how rototo should produce those
diagnostics from a workspace in a way that supports:

- whole-workspace semantic linting;
- custom lint registration by pipeline stage, entity, and field;
- coherent CLI and SDK lint APIs built around the same LSP-ready result model;
- future Language Server Protocol diagnostics, completion, hover, and
  go-to-definition.

## Problem Statement

Lint is the release gate for a rototo workspace. The CLI uses it before config
is merged. `Workspace::load` uses it before an application accepts a workspace.
A future LSP will use it while an author edits unsaved files.

The current implementation works for the first checkpoint, but its shape is too
local for the next one:

1. Workspace files are parsed repeatedly. `lint_workspace` inspects the
   workspace, then re-reads each qualifier and variable, then context-schema
   validation re-reads qualifiers again.
2. Lint rules inspect ad hoc TOML paths instead of a shared semantic model.
   Reference checks pass small side indexes such as `HashSet<&str>` for
   qualifiers and `&[String]` for environments.
3. Custom Lua lint is variable-scoped and fixed at one execution point. It can
   validate `lint(variable)` and `lint_value(value)`, but it cannot register at
   a particular pipeline stage, cannot target qualifiers or the workspace, and
   cannot attach to a specific field.
4. CLI diagnostics currently point at files. LSP diagnostics need precise
   ranges, related locations, and stable document identities.

The design below replaces the lint internals with one source-aware semantic
index and an ordered lint pipeline.

## Design Principles

1. Prefer the clean lint API for the next model. Backward compatibility is not
   required yet, so `lint_workspace`, `lint_qualifier`, and `lint_variable` may
   return richer LSP-ready diagnostics instead of preserving the current
   path-only structs.
2. Parse once, then lint from the index. Stages after parsing should not read
   files directly.
3. Preserve partial progress. A broken variable should not prevent unrelated
   qualifiers, variables, schemas, or lint files from being checked.
4. Put source spans on every semantic node from the start. Retrofitting ranges
   later would force another rewrite.
5. Treat custom lint as a first-class pipeline participant. Built-in and custom
   checks should use the same stage, target, and diagnostic machinery.
6. Do not add a package model, database, service, or generated state. The
   workspace remains the control-plane boundary.

## High-Level Architecture

The lint engine has four layers:

```text
workspace source
  -> SourceStore       // documents, text, line indexes, overlays
  -> SyntaxIndex       // parsed TOML/JSON/Lua registration input
  -> SemanticIndex     // workspaces, qualifiers, variables, values, schemas
  -> LintPipeline      // built-in checks + custom registered checks
  -> LintResult        // span-rich diagnostics, grouped by document
```

The CLI, SDK, and future LSP should all consume the same lint result model. The
CLI can render a compact human view from that model, but the underlying JSON
and SDK structs should include document identity, spans, related locations,
stage, and entity:

```text
LintResult
  documents: [SourceDocumentSummary]
  diagnostics: [LintDiagnostic { location, related, stage, entity, rule, severity, message, help }]
```

The LSP is not a later adapter over CLI output. It is a first-class consumer of
the lint model, so source stores, spans, per-document diagnostics, and
incremental invalidation are part of the initial design.

This design replaces lint internals only. `inspect_workspace` and resolution
can continue using their existing workspace loading and parsing paths during
the first implementation. "Parse once" means "parse once per lint snapshot";
it does not require the semantic lint index to feed `resolve.rs` immediately.

## Identifiers and Source Types

Use small newtypes internally instead of passing raw strings everywhere. They
make indexes explicit and reduce accidental cross-use of ids.

```rust
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct QualifierId(String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct VariableId(String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct EnvironmentId(String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ValueKey(String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct WorkspacePath(PathBuf);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct DocId(u32);
```

What each identifier means:

- `QualifierId` is the file stem under `qualifiers/`; for
  `qualifiers/premium-users.toml`, the value is `"premium-users"`.
- `VariableId` is the file stem under `variables/`; for
  `variables/checkout-redesign.toml`, the value is `"checkout-redesign"`.
- `EnvironmentId` is a manifest environment such as `"dev"`, `"stage"`, or
  `"prod"`. The fallback environment `"_"` is represented where variable env
  blocks need it, but it is not part of the manifest's declared environment
  set.
- `ValueKey` is a configured branch inside a variable, such as `"standard"` or
  `"enterprise"`.
- `WorkspacePath` is always relative to the workspace root, for example
  `variables/llm-agent-config.toml` or
  `variables/llm-agent-config-values/enterprise.toml`. Absolute paths are only
  used at the source-loading boundary.
- `DocId` is stable within one lint snapshot. A snapshot might assign
  `DocId(0)` to `rototo-workspace.toml`, `DocId(1)` to
  `qualifiers/premium-users.toml`, and `DocId(2)` to
  `variables/checkout-redesign.toml`. Diagnostics and semantic nodes use
  `DocId` instead of repeatedly carrying paths and URIs.

## Source Store

The source store is the boundary between lint and file I/O. CLI lint uses disk
documents. LSP lint supplies overlays for unsaved documents. Every later stage
reads document text through this store.

```rust
pub(crate) struct SourceStore {
    /// Canonical workspace root for path resolution and output.
    pub root: PathBuf,

    /// All documents considered by this lint run.
    pub documents: BTreeMap<DocId, SourceDocument>,

    /// Relative path to document id.
    pub by_path: BTreeMap<WorkspacePath, DocId>,
}

pub(crate) struct SourceDocument {
    pub id: DocId,
    pub path: WorkspacePath,
    pub uri: DocumentUri,
    pub kind: SourceKind,

    /// LSP document version. None for normal CLI/SDK disk lint.
    pub version: Option<i32>,

    /// Current text. This is disk text for CLI and overlay text for LSP.
    pub text: Arc<str>,

    /// Byte offset to line/column conversion.
    pub line_index: LineIndex,
}

pub(crate) struct DocumentUri(String);

pub(crate) struct LineIndex {
    /// Byte offset for the first character of each line.
    pub line_starts: Vec<usize>,
}

pub(crate) enum SourceKind {
    Manifest,
    Qualifier,
    Variable,
    ExternalValue { variable_id: VariableId, value_key: ValueKey },
    Schema,
    CustomLint,
}
```

Why this shape matters:

- `SourceStore.root` is the canonical root used to resolve safe relative
  workspace paths. For `examples/basic`, this is the absolute path to that
  workspace.
- `SourceStore.documents` is the complete document set for the lint run. For
  `examples/basic`, entries include the manifest, each qualifier file, each
  variable file, each external value file, each schema, and each
  auto-discovered lint script.
- `SourceStore.by_path` answers "which document owns this workspace path?" For
  example, `variables/checkout-redesign.toml -> DocId(17)`.
- `SourceDocument.uri` is the editor-facing URI, such as
  `file:///repo/examples/basic/variables/checkout-redesign.toml`.
- `SourceDocument.version` is `Some(42)` for an open LSP document at version
  42 and `None` for CLI lint from disk.
- `SourceDocument.text` is the text lint parses. In CLI mode this is disk text;
  in LSP mode this can be an unsaved overlay from `didChange`.
- `SourceKind` tells later stages which projection to attempt. A TOML document
  with `SourceKind::Qualifier` becomes a `QualifierNode`; a TOML document with
  `SourceKind::ExternalValue` becomes a `ValueNode`.
- Diagnostics can be grouped by document for LSP.
- CLI code can still print paths by looking up `doc -> path`.
- The lint engine does not need to know whether text came from disk or an
  unsaved editor buffer.

Example store entries:

```text
DocId(0) -> SourceDocument {
  path: "rototo-workspace.toml",
  uri: "file:///workspace/rototo-workspace.toml",
  kind: Manifest,
  version: None,
}

DocId(24) -> SourceDocument {
  path: "variables/directory-backed-message-values/premium.toml",
  kind: ExternalValue {
    variable_id: "directory-backed-message",
    value_key: "premium",
  },
}
```

## Spans

Every semantic field that can produce a diagnostic gets a span.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextRange {
    /// Inclusive byte offset from the start of the document.
    pub start: usize,

    /// Exclusive byte offset from the start of the document.
    pub end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceSpan {
    pub doc: DocId,
    pub range: TextRange,
}

#[derive(Clone, Debug)]
pub(crate) struct Spanned<T> {
    pub value: T,
    pub span: SourceSpan,
}
```

Byte ranges are stored internally. The LSP adapter converts byte ranges to
UTF-16 line/character ranges using `LineIndex`. CLI output can ignore inner
ranges and only print the document path.

Field meanings and examples:

- `TextRange.start` and `TextRange.end` are byte offsets in the current
  document text. If `qualifier = "premium-users"` begins at byte 214 and the
  string literal ends at byte 239, the reference span is
  `TextRange { start: 214, end: 239 }`.
- `SourceSpan.doc` ties a range to a document. `SourceSpan { doc: DocId(12),
  range: TextRange { start: 214, end: 239 } }` points at bytes in whatever
  `SourceDocument` owns `DocId(12)`.
- `Spanned<T>` is used for any semantic value that should be addressable in an
  editor. A rule qualifier reference can be stored as
  `Spanned<QualifierId> { value: "premium-users", span: ... }`, so
  go-to-definition and diagnostics both know where the reference text lives.

The TOML parse layer should use `toml-span`. The important requirement is that
TOML keys and values can be mapped back to `SourceSpan`. `toml::Value` alone is
not enough for LSP-quality diagnostics because it drops source positions.
`toml-span` preserves spans for TOML items without routing that information
through Serde.

## Syntax Index

The syntax index is the parsed form of source text before rototo meaning is
assigned.

It answers questions about **text structure**:

- Is this document valid TOML, JSON, or Lua source?
- Which keys, tables, arrays, strings, numbers, and objects appear in it?
- Where is each syntactic item in the source file?

It does not answer semantic questions:

- Is `premium-users` a known qualifier?
- Is `prod` a declared environment?
- Is `"enterprise"` a known value key for this variable?
- Does this value match the variable schema?

Those questions belong to the semantic index and later lint stages.

The syntax index exists because the LSP needs precise locations. Once TOML is
converted directly into a plain `toml::Value`, source positions are gone. The
syntax index keeps the parsed tree and spans so later stages can say "the
unknown value reference is this exact string literal," not only "something is
wrong somewhere in `variables/foo.toml`."

```rust
pub(crate) struct SyntaxIndex {
    pub documents: BTreeMap<DocId, ParsedDocument>,
}

pub(crate) enum ParsedDocument {
    Toml(ParsedToml),
    Json(ParsedJson),
    LuaSource(ParsedLuaSource),
    ParseFailed(ParseFailure),
}

pub(crate) struct ParsedToml {
    pub doc: DocId,

    /// Span-preserving TOML tree returned by toml-span, lightly wrapped so
    /// rototo can convert its spans into SourceSpan.
    pub spanned_root: SpannedTomlValue,
}

pub(crate) struct SpannedTomlValue {
    pub inner: toml_span::Value,
}

impl ParsedToml {
    pub fn to_plain_toml(&self) -> toml::Value {
        // Derived on demand for Lua/schema serialization and compatibility
        // helpers. Projection should use spanned_root directly.
        todo!()
    }
}

pub(crate) struct ParsedJson {
    pub doc: DocId,
    pub value: serde_json::Value,
    pub root_span: SourceSpan,
}

pub(crate) struct ParsedLuaSource {
    pub doc: DocId,
    pub text: Arc<str>,
}

pub(crate) struct ParseFailure {
    pub doc: DocId,
    pub message: String,
    pub span: Option<SourceSpan>,
}
```

Lua is not fully parsed by rototo at this layer. It is stored as source text and
executed later for registration and handlers through `spawn_blocking`.

The key distinction:

```text
SourceStore:
  "variables/llm-agent-config.toml is this text"

SyntaxIndex:
  "that text is valid TOML; it has an [env.prod] table; inside it,
   rule[0].qualifier is the string 'enterprise-accounts' at bytes 214..235"

SemanticIndex:
  "'enterprise-accounts' is a qualifier reference from variable
   llm-agent-config, prod rule 0, and it resolves to
   qualifiers/enterprise-accounts.toml"
```

Field meanings and examples:

- `SyntaxIndex.documents` is keyed by `DocId`, not path. `DocId(0)` might hold
  `ParsedDocument::Toml` for the manifest, while `DocId(31)` holds
  `ParsedDocument::Json` for `schemas/llm-config.schema.json`.
- `ParsedDocument::ParseFailed` preserves a failed parse as data. A malformed
  variable file still appears in the syntax index, which lets lint publish a
  diagnostic for that document and skip only that variable in later stages.
- `ParsedToml.spanned_root` is the single TOML representation for the
  document. It comes from `toml-span`; rototo should not project it into a
  second hand-rolled arena.
- `SpannedTomlValue.inner` preserves TOML values and their source spans.
  Projection reads this tree directly to find tables, arrays, scalar values,
  and exact key/value spans.
- `ParsedToml::to_plain_toml` derives a normal `toml::Value` only when a plain
  value is needed for Lua context serialization, schema conversion, or
  compatibility helpers. Plain TOML values are not the projection source.
- `ParsedJson.root_span` usually covers the whole JSON schema document. If
  later JSON span support is added, schema field diagnostics can become more
  precise without changing `SchemaNode`.
- `ParsedLuaSource.text` is the source loaded from `lint/platform.lua`. The
  registration step executes this text to collect `lint:on(...)` declarations.

Example syntax entries:

```text
DocId(0) -> Toml(ParsedToml {
  spanned_root -> Table {
    "schema_version" -> Integer(1),
    "environments" -> Table {
      "values" -> Array [String("dev"), String("stage"), String("prod")]
    }
  }
})

DocId(8) -> Toml(ParsedToml {
  spanned_root -> Table {
    "predicate" -> Array [
      Table {
        "attribute" -> String("qualifier.premium-users"),
        "op" -> String("eq"),
        "value" -> Boolean(true)
      }
    ]
  }
})

DocId(31) -> Json(ParsedJson {
  value.type: "object",
  root_span: entire schema document,
})
```

## Semantic Index

The semantic index is the central lint model. It owns typed nodes, reference
edges, source spans, custom rule declarations, and coarse gates for entities
that should be skipped by later stages.

```rust
pub(crate) struct SemanticIndex {
    pub root: PathBuf,
    pub source: SourceStore,

    pub manifest: Option<ManifestNode>,
    pub environments: BTreeMap<EnvironmentId, EnvironmentNode>,
    pub context_schema: Option<ContextSchemaNode>,

    pub qualifiers: BTreeMap<QualifierId, QualifierNode>,
    pub variables: BTreeMap<VariableId, VariableNode>,
    pub schemas: BTreeMap<WorkspacePath, SchemaNode>,

    pub references: ReferenceIndex,
    pub custom_lints: CustomLintRegistry,

    /// Tracks which entities should be skipped by later stages.
    pub gates: GateIndex,
}
```

`SemanticIndex` is crate-private at first. It is deliberately shaped so a
future LSP crate can use it as a symbol model without reparsing workspace
files.

Field meanings and examples:

- `root` and `source` connect semantic nodes back to their text and paths.
- `manifest` is `None` only when the manifest is missing or cannot be parsed.
  When present, it contains declared environments, context schema refs, and
  custom lint declarations.
- `environments` is the validated manifest environment set. In
  `examples/basic`, it contains `"dev"`, `"stage"`, and `"prod"`.
- `context_schema` points at the workspace context schema when `[context]` is
  declared. For `examples/basic`, it points at
  `schemas/context.schema.json`.
- `qualifiers` maps ids such as `"premium-users"` to `QualifierNode`s.
- `variables` maps ids such as `"checkout-redesign"` to `VariableNode`s.
- `schemas` maps workspace paths such as `schemas/llm-config.schema.json` to
  parsed and compiled `SchemaNode`s.
- `references` is the cross-entity graph: qualifier refs, variable rule refs,
  value refs, schema refs, and context attribute refs.
- `custom_lints` contains rule metadata from the manifest and registrations
  collected from Lua lint files.
- `gates` records which nodes are blocked by earlier errors, so later stages
  can avoid duplicate noise.

Example index summary:

```text
SemanticIndex {
  environments: ["dev", "stage", "prod"],
  context_schema: Some("schemas/context.schema.json"),
  qualifiers: {
    "premium-users": QualifierNode(...),
    "premium-beta-users": QualifierNode(...),
  },
  variables: {
    "checkout-redesign": VariableNode(...),
    "llm-agent-config": VariableNode(...),
  },
  schemas: {
    "schemas/checkout-page.schema.json": SchemaNode(...),
  },
}
```

### Gate Index

Lint should continue after local failures while avoiding noisy downstream
diagnostics for invalid entities.

```rust
pub(crate) struct GateIndex {
    pub entity_state: BTreeMap<GateEntity, GateState>,
}

pub(crate) struct GateState {
    pub blocked_at: LintStage,
    pub diagnostic: Option<DiagnosticRule>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum GateEntity {
    Manifest,
    Qualifier(QualifierId),
    Variable(VariableId),
    ExternalValue { variable: VariableId, key: ValueKey },
    Schema(WorkspacePath),
    CustomLintFile(WorkspacePath),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum EntityId {
    Workspace,
    Manifest,
    Qualifier(QualifierId),
    Predicate { qualifier: QualifierId, index: usize },
    Variable(VariableId),
    Value { variable: VariableId, key: ValueKey },
    EnvironmentBlock { variable: VariableId, environment: EnvironmentId },
    Rule { variable: VariableId, environment: EnvironmentId, index: usize },
    Schema(WorkspacePath),
    CustomLintFile(WorkspacePath),
    CustomRule(CustomRuleId),
}
```

Example: if `variables/foo.toml` fails parse, `GateEntity::Variable(foo)` is
blocked at `LintStage::Parse`. Reference, value, graph, and custom policy
stages skip it. Other variables still run.

Field meanings and examples:

- `GateIndex.entity_state` has entries only for coarse source-backed entities
  that later stages should skip. Most valid entities have no entry.
- `GateState.blocked_at = Parse` means "the entity exists as a source
  document, but later semantic stages must not assume it parsed or projected
  cleanly."
- `GateEntity` deliberately stops at manifest, qualifier, variable, external
  value, schema, and custom lint file. It does not gate individual predicates,
  rules, or fields.
- Intra-file problems remain in the node model. Examples:
  `PredicateNode.attribute = None`, `PredicateOp::Unknown("contains")`, or
  `TypeSourceNode::Conflict` represent malformed local shape and let Project
  emit precise diagnostics without marking the whole variable unavailable.
- `EntityId::Predicate { qualifier: "premium-beta-users", index: 1 }`
  identifies the second `[[predicate]]` table in that qualifier.
- `EntityId::Rule { variable: "checkout-redesign", environment: "prod",
  index: 0 }` identifies the first rule under `[env.prod]`.
- `EntityId::Value { variable: "llm-agent-config", key: "enterprise" }`
  identifies the `enterprise` value whether it came from the variable file or a
  sibling external value file.
- `EntityId` remains fine-grained for diagnostics, references, custom targets,
  and LSP symbols. `GateEntity` is the coarser validity boundary.

## Manifest Nodes

The manifest declares workspace-level facts and custom rule metadata.

```rust
pub(crate) struct ManifestNode {
    pub doc: DocId,
    pub path: WorkspacePath,

    pub schema_version: Option<Spanned<i64>>,
    pub environments: Vec<Spanned<EnvironmentId>>,
    pub context_schema_ref: Option<SchemaReferenceNode>,

    /// Reviewable rule metadata. Lua registrations may only reference these.
    pub custom_rule_declarations: Vec<CustomRuleDeclarationNode>,

    pub span: SourceSpan,
}

pub(crate) struct EnvironmentNode {
    pub id: Spanned<EnvironmentId>,
}

pub(crate) struct ContextSchemaNode {
    pub reference: SchemaReferenceNode,
    pub schema: Option<SchemaNode>,
}
```

Proposed manifest shape:

```toml
schema_version = 1

[environments]
values = ["dev", "stage", "prod"]

[context]
schema = "schemas/context.schema.json"

[[lint.rule]]
id = "payments/max-token-budget"
title = "Token budget exceeds payments policy"
help = "Lower max_output_tokens or update the payments policy."
severity = "error"
```

This moves rule metadata out of variables and into a workspace-level catalog.
The metadata remains reviewable TOML. Lua files register where and how the rules
run.

Field meanings and examples:

- `ManifestNode.doc` points at the manifest document, usually `DocId(0)`.
- `path` is normally `rototo-workspace.toml`.
- `schema_version` is `Some(1)` for the supported format. If the field is
  absent, this is `None` and the Project stage emits
  `workspace-manifest-schema-failed`.
- `environments` stores each declared environment with its source span. For the
  example above, it contains spanned values for `"dev"`, `"stage"`, and
  `"prod"`.
- `context_schema_ref` stores the literal schema reference
  `"schemas/context.schema.json"` plus the resolved `WorkspacePath` if it is
  safe and inside the workspace.
- `custom_rule_declarations` stores TOML-owned metadata for custom rules. A
  Lua registration can use `payments/max-token-budget` only if that id appears
  here.
- `EnvironmentNode` exists so environment symbols have spans for hover,
  completion, and diagnostics.
- `ContextSchemaNode.schema` points at the parsed schema node after the schema
  file is loaded. It remains `None` when the schema reference is malformed or
  unreadable.

## Qualifier Nodes

Qualifiers become typed predicates plus precomputed reference edges.

```rust
pub(crate) struct QualifierNode {
    pub id: Spanned<QualifierId>,
    pub uri: String,
    pub doc: DocId,
    pub path: WorkspacePath,

    pub schema_version: Option<Spanned<i64>>,
    pub description: Option<Spanned<String>>,
    pub predicates: Vec<PredicateNode>,

    /// Whole-file or root table span.
    pub span: SourceSpan,
}

pub(crate) struct PredicateNode {
    pub entity: EntityId,
    pub index: usize,
    pub span: SourceSpan,

    pub attribute: Option<Spanned<PredicateAttribute>>,
    pub op: Option<Spanned<PredicateOp>>,
    pub value: Option<Spanned<toml::Value>>,

    /// Only present for bucket predicates.
    pub bucket: Option<BucketPredicateNode>,
}

pub(crate) enum PredicateAttribute {
    ContextPath(ContextPath),
    QualifierRef(QualifierReferenceNode),
}

pub(crate) enum PredicateOp {
    Eq,
    Neq,
    In,
    NotIn,
    Gt,
    Gte,
    Lt,
    Lte,
    Bucket,
    Unknown(String),
}

pub(crate) struct BucketPredicateNode {
    pub salt: Option<Spanned<String>>,
    pub range: Option<Spanned<BucketRange>>,
}

pub(crate) struct BucketRange {
    pub start: i64,
    pub end: i64,
}
```

Why predicates are modeled this way:

- `QualifierNode.id` is the file stem with a span over the source id when the
  id is represented in text. For discovered files, the value comes from the
  path, so the span can fall back to the whole document.
- `uri` is the SDK/CLI identifier, for example
  `qualifier://premium-beta-users`.
- `doc` and `path` identify the source file, such as
  `qualifiers/premium-beta-users.toml`.
- `schema_version` is `Some(1)` for a valid qualifier file. Missing or
  unsupported versions produce `qualifier-schema-version`.
- `description` carries the optional human explanation.
- `predicates` preserves source order because qualifiers are ANDed and because
  diagnostics should report predicate indexes predictably.
- `PredicateNode.entity` is the stable semantic id for this predicate.
- `PredicateNode.attribute` is either a context path, such as
  `user.tier`, or a qualifier reference, such as
  `qualifier.premium-users`.
- `PredicateNode.op` is normalized into `PredicateOp`, with
  `Unknown("contains")` preserving the bad text for diagnostics.
- `PredicateNode.value` stores the comparison value for non-bucket predicates,
  such as `"premium"` or `true`.
- `BucketPredicateNode` stores `salt` and `range` for bucket rules, for
  example `salt = "checkout-redesign"` and `range = [0, 2500]`.
- Shape errors can point at the missing or malformed field.
- Reference lint does not need to reparse `attribute = "qualifier.foo"`.
- Graph lint can read qualifier-to-qualifier edges directly from the reference
  index.

Example node for `qualifiers/premium-beta-users.toml`:

```text
QualifierNode {
  id: "premium-beta-users",
  uri: "qualifier://premium-beta-users",
  path: "qualifiers/premium-beta-users.toml",
  predicates: [
    PredicateNode {
      attribute: QualifierRef("premium-users"),
      op: Eq,
      value: true,
    },
    PredicateNode {
      attribute: QualifierRef("beta-rollout-bucket"),
      op: Eq,
      value: true,
    },
  ],
}
```

## Variable Nodes

Variables are the richest entity because they own values, environment defaults,
rules, type/schema contracts, and custom policy targets.

```rust
pub(crate) struct VariableNode {
    pub id: Spanned<VariableId>,
    pub uri: String,
    pub doc: DocId,
    pub path: WorkspacePath,

    pub schema_version: Option<Spanned<i64>>,
    pub description: Option<Spanned<String>>,
    pub type_source: TypeSourceNode,

    /// Inline and external values projected into one map. Each value keeps its
    /// origin, so diagnostics can point to the correct file.
    pub values: BTreeMap<ValueKey, ValueNode>,

    /// Includes the required "_" fallback block when present.
    pub environments: BTreeMap<EnvironmentId, EnvironmentBlockNode>,

    pub span: SourceSpan,
}

pub(crate) enum TypeSourceNode {
    Primitive(Spanned<PrimitiveType>),
    Schema(SchemaReferenceNode),
    Missing { span: SourceSpan },
    Conflict {
        type_span: SourceSpan,
        schema_span: SourceSpan,
    },
    Unknown(Spanned<String>),
}

pub(crate) enum PrimitiveType {
    Bool,
    Int,
    Number,
    String,
    List,
}

pub(crate) struct SchemaReferenceNode {
    pub raw: Spanned<String>,
    pub resolved_path: Option<WorkspacePath>,
}
```

`TypeSourceNode` intentionally represents invalid states. Lint needs to emit a
diagnostic for those states while still keeping enough information to avoid
panics and continue elsewhere.

Field meanings and examples:

- `VariableNode.id` is the application-facing variable id, such as
  `"llm-agent-config"`.
- `uri` is the SDK/CLI identifier, such as
  `variable://llm-agent-config`.
- `doc` and `path` point at the variable definition file.
- `schema_version` captures `schema_version = 1`; if absent, the Project stage
  emits `variable-schema-version`.
- `description` is optional but addressable for hover and policy checks.
- `type_source` is exactly one of primitive type or schema when the variable is
  valid. Invalid states are represented explicitly:
  - `Primitive(Int)` for `type = "int"`;
  - `Schema("../schemas/llm-config.schema.json")` for schema-backed values;
  - `Missing` when neither `type` nor `schema` is present;
  - `Conflict` when both are present;
  - `Unknown("currency")` for an unsupported primitive type.
- `values` contains both inline values and external sibling value files after
  projection.
- `environments` stores `[env._]`, `[env.dev]`, `[env.prod]`, and any other
  blocks exactly as authored, including unknown environments so lint can point
  at them.
- `SchemaReferenceNode.raw` is the literal text from TOML, while
  `resolved_path` is the normalized workspace path when the reference is safe.

Example node for `variables/llm-agent-config.toml`:

```text
VariableNode {
  id: "llm-agent-config",
  uri: "variable://llm-agent-config",
  path: "variables/llm-agent-config.toml",
  type_source: Schema("../schemas/llm-config.schema.json"),
  values: {
    "local": ValueNode(...),
    "standard": ValueNode(...),
    "enterprise": ValueNode(...),
  },
  environments: {
    "_": EnvironmentBlockNode { value: "standard", rules: [] },
    "dev": EnvironmentBlockNode { value: "local", rules: [] },
    "prod": EnvironmentBlockNode {
      value: "standard",
      rules: [
        VariableRuleNode {
          qualifier: "enterprise-accounts",
          value: "enterprise",
        },
      ],
    },
  },
}
```

### Values

```rust
pub(crate) struct ValueNode {
    pub entity: EntityId,
    pub variable_id: VariableId,
    pub key: Spanned<ValueKey>,
    pub value: Spanned<toml::Value>,
    pub origin: ValueOrigin,
}

pub(crate) enum ValueOrigin {
    Inline {
        variable_doc: DocId,
    },
    External {
        doc: DocId,
        path: WorkspacePath,
    },
}
```

Today many value diagnostics point at the variable file. With `ValueOrigin`, a
schema mismatch in `variables/foo-values/enterprise.toml` can point at the
external value file.

Field meanings and examples:

- `ValueNode.entity` is usually
  `EntityId::Value { variable: "directory-backed-message", key: "premium" }`.
- `variable_id` links the value back to its owning variable.
- `key` is the branch name used by environment blocks and rules. For
  `variables/directory-backed-message-values/premium.toml`, the key is
  `"premium"`.
- `value` is the TOML value after unwrapping external files that use
  `value = ...` or `[value]`.
- `origin` distinguishes inline and external values. Inline values point at the
  variable document. External values point at the sibling value document.

Example values:

```text
ValueNode {
  variable_id: "max-output-tokens",
  key: "large",
  value: 2000,
  origin: Inline { variable_doc: DocId(18) },
}

ValueNode {
  variable_id: "directory-backed-message",
  key: "premium",
  value: "Welcome back, premium member.",
  origin: External {
    doc: DocId(24),
    path: "variables/directory-backed-message-values/premium.toml",
  },
}
```

### Environment Blocks and Rules

```rust
pub(crate) struct EnvironmentBlockNode {
    pub entity: EntityId,
    pub environment: Spanned<EnvironmentId>,
    pub value: Option<ValueReferenceNode>,
    pub rules: Vec<VariableRuleNode>,
    pub span: SourceSpan,
}

pub(crate) struct VariableRuleNode {
    pub entity: EntityId,
    pub index: usize,
    pub qualifier: Option<QualifierReferenceNode>,
    pub value: Option<ValueReferenceNode>,
    pub description: Option<Spanned<String>>,
    pub span: SourceSpan,
}

pub(crate) struct QualifierReferenceNode {
    pub id: Spanned<QualifierId>,
}

pub(crate) struct ValueReferenceNode {
    pub key: Spanned<ValueKey>,
}
```

This supports:

- `EnvironmentBlockNode.environment` is the env block key. Examples are
  `"_"`, `"dev"`, and `"prod"`.
- `EnvironmentBlockNode.value` is the default value reference for the block.
  For `[env.prod] value = "standard"`, this stores `"standard"` with the span
  of the string literal.
- `rules` preserves source order because the first matching rule wins during
  resolution.
- `VariableRuleNode.index` is the rule's position within one environment block.
  The first `[[env.prod.rule]]` has `index = 0`.
- `VariableRuleNode.qualifier` stores the referenced qualifier id with its
  source span.
- `VariableRuleNode.value` stores the selected value key with its source span.
- `description` is optional rule-level prose.
- Unknown environment, unknown qualifier, unknown value, shadowed rule,
  go-to-definition, and find-references all read this structure.

Example block:

```text
EnvironmentBlockNode {
  environment: "prod",
  value: "standard",
  rules: [
    VariableRuleNode {
      index: 0,
      qualifier: "enterprise-accounts",
      value: "enterprise",
      description: "Enterprise accounts get the larger reasoning model",
    },
  ],
}
```

## Schema Nodes

Schemas are first-class because they can be standalone lint targets, context
contracts, or variable value contracts.

```rust
pub(crate) struct SchemaNode {
    pub entity: EntityId,
    pub doc: DocId,
    pub path: WorkspacePath,
    pub json: Option<serde_json::Value>,
    pub validator: Option<Arc<jsonschema::Validator>>,
    pub span: SourceSpan,
}
```

Standalone schema lint (`schemas/*.json`) and variable schema refs both use
`SchemaNode`. A schema file can parse but fail compilation; in that case
`json` is present and `validator` is absent.

Field meanings and examples:

- `entity` is `EntityId::Schema("schemas/llm-config.schema.json")`.
- `doc` and `path` identify the JSON schema source document.
- `json` is `Some(serde_json::Value)` when JSON parsing succeeds. It is `None`
  for invalid JSON.
- `validator` is `Some(Arc<jsonschema::Validator>)` when the parsed JSON
  compiles as JSON Schema. It is `None` when compilation fails.
- `span` initially points at the full schema document. If JSON span support is
  added later, individual schema keywords can have more precise spans.

Example:

```text
SchemaNode {
  path: "schemas/llm-config.schema.json",
  json: Some({ "type": "object", "properties": { ... } }),
  validator: Some(...),
}
```

### Path Resolution and Containment

Every path read by lint must normalize to a `WorkspacePath` inside the
canonical workspace root before it is opened. Absolute paths, URLs inside
workspace files, and paths that escape the workspace are invalid.

Path rules differ by source:

- Conventional files are discovered from fixed root-relative directories:
  `qualifiers/*.toml`, `variables/*.toml`, `schemas/*.json`, and
  `lint/*.lua`.
- `lint/*.lua` custom files are auto-discovered directly under the root
  `lint/` directory. Lint files are not supplied by user path fields, and
  discovery must reject files whose canonical path escapes the workspace.
- `[context].schema` is resolved relative to the workspace root because the
  manifest lives at the root. It should be written as a root-relative path such
  as `schemas/context.schema.json`; parent-directory traversal is invalid.
- Variable `schema` is resolved relative to the variable file's directory.
  `../schemas/llm-config.schema.json` is valid for a file under `variables/`
  because the normalized path remains inside the workspace.
- External values are discovered by convention from the sibling
  `<variable-id>-values/*.toml` directory. There is no path field to resolve.

For any path that can contain `..`, validation is based on the normalized and
canonicalized result, not on string prefix checks alone. The accepted result is
always stored as a normalized `WorkspacePath`, for example
`schemas/llm-config.schema.json`.

### Schema Normalization and Reporting

Schema files are deduplicated by normalized `WorkspacePath`. A schema referenced
as `../schemas/llm-config.schema.json` from a variable and discovered as
`schemas/llm-config.schema.json` is one `SchemaNode`, compiled at most once.

Reporting rules:

- `rototo/variable-schema-ref` fires when a variable schema path is malformed,
  escapes the workspace, cannot be read, or does not resolve to a schema
  document.
- `rototo/schema-parse-failed` fires once per schema document when JSON parsing
  fails.
- `rototo/schema-invalid` fires once per schema document when parsed JSON does
  not compile as JSON Schema.
- If a variable points at a schema document that exists but fails parse or
  compilation, emit the schema diagnostic on the schema document and do not also
  emit `rototo/variable-schema-ref` for the variable. Add a related location
  from the schema diagnostic back to referencing variables when practical.
- Value validation is skipped for variables whose schema node has no compiled
  validator, to avoid cascading schema mismatch diagnostics after a schema parse
  or compile failure.

## Reference Index

The reference index stores all semantic edges and their source locations.

```rust
pub(crate) struct ReferenceIndex {
    pub qualifier_refs: Vec<QualifierReferenceEdge>,
    pub variable_qualifier_refs: Vec<VariableQualifierReferenceEdge>,
    pub variable_value_refs: Vec<VariableValueReferenceEdge>,
    pub variable_schema_refs: Vec<VariableSchemaReferenceEdge>,
    pub context_attribute_refs: Vec<ContextAttributeReferenceEdge>,

    /// Reverse indexes used by graph lint and future LSP find-references.
    pub qualifier_referenced_by: BTreeMap<QualifierId, Vec<ReferenceSite>>,
    pub value_referenced_by: BTreeMap<(VariableId, ValueKey), Vec<ReferenceSite>>,
}

pub(crate) struct ReferenceSite {
    pub from: EntityId,
    pub span: SourceSpan,
}

pub(crate) struct QualifierReferenceEdge {
    pub from: QualifierId,
    pub to: QualifierId,
    pub site: ReferenceSite,
}

pub(crate) struct VariableQualifierReferenceEdge {
    pub variable: VariableId,
    pub environment: EnvironmentId,
    pub rule_index: usize,
    pub to: QualifierId,
    pub site: ReferenceSite,
}

pub(crate) struct VariableValueReferenceEdge {
    pub variable: VariableId,
    pub environment: EnvironmentId,
    pub rule_index: Option<usize>,
    pub to: ValueKey,
    pub site: ReferenceSite,
}

pub(crate) struct VariableSchemaReferenceEdge {
    pub variable: VariableId,
    pub to: WorkspacePath,
    pub site: ReferenceSite,
}

pub(crate) struct ContextAttributeReferenceEdge {
    pub qualifier: QualifierId,
    pub attribute: Spanned<ContextPath>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ContextPath(Vec<String>);
```

Reference checks read this index:

- `qualifier_refs` stores qualifier-to-qualifier predicate edges. Example:
  `premium-beta-users -> premium-users`.
- `variable_qualifier_refs` stores variable rule edges. Example:
  `llm-agent-config/prod/rule[0] -> enterprise-accounts`.
- `variable_value_refs` stores default and rule value references. Example:
  `llm-agent-config/prod/default -> standard` and
  `llm-agent-config/prod/rule[0] -> enterprise`.
- `variable_schema_refs` stores schema-backed variable refs. Example:
  `llm-agent-config -> schemas/llm-config.schema.json`.
- `context_attribute_refs` stores qualifier context paths. Example:
  `premium-users -> user.tier`.
- `ReferenceSite.from` identifies the source entity that made the reference.
- `ReferenceSite.span` points at the reference text, not the declaration.
- `qualifier_referenced_by` and `value_referenced_by` are reverse indexes used
  by graph lint and LSP find-references.
- Reference checks use this index to prove that qualifier references exist,
  variable rule qualifiers exist, environment names are declared, value keys
  exist, schema refs normalize to readable in-workspace schema documents, and
  context schema declares qualifier context attributes.

Graph checks also read this index:

- qualifier cycles;
- unreferenced qualifiers;
- unused variable values;
- rule shadowing.

Example reference index fragment:

```text
qualifier_refs: [
  { from: "premium-beta-users", to: "premium-users", site: predicate[0].attribute },
  { from: "premium-beta-users", to: "beta-rollout-bucket", site: predicate[1].attribute },
]

variable_qualifier_refs: [
  {
    variable: "llm-agent-config",
    environment: "prod",
    rule_index: 0,
    to: "enterprise-accounts",
  },
]

value_referenced_by[("llm-agent-config", "enterprise")] = [
  Rule { variable: "llm-agent-config", environment: "prod", index: 0 },
]
```

## Custom Lint Registry

Custom lint has two phases:

1. Rule metadata is declared in reviewable TOML.
2. Lua lint files auto-discovered from `lint/*.lua` register handlers that
   bind a declared rule to a pipeline stage, entity, and optional field
   selector.

```rust
pub(crate) struct CustomLintRegistry {
    /// Rule metadata declared in TOML.
    pub rules: BTreeMap<CustomRuleId, CustomRuleDeclarationNode>,

    /// Lua files auto-discovered from lint/*.lua.
    pub files: BTreeMap<WorkspacePath, CustomLintFileNode>,

    /// Handler bindings collected by executing register(lint).
    pub registrations: Vec<CustomLintRegistration>,
}

pub(crate) struct CustomRuleDeclarationNode {
    pub id: Spanned<CustomRuleId>,
    pub title: Spanned<String>,
    pub help: Spanned<String>,
    pub severity: Spanned<Severity>,
    pub span: SourceSpan,
}

pub(crate) struct CustomLintFileNode {
    pub path: WorkspacePath,
    pub doc: DocId,
    pub span: SourceSpan,
}

pub(crate) struct CustomLintRegistration {
    pub file: WorkspacePath,
    pub rule: CustomRuleId,
    pub stage: LintStage,
    pub target: LintTargetSelector,
    pub handler: LuaHandlerName,

    /// Best-effort span. If Lua cannot provide a precise source range for the
    /// registration call, use the whole lint file.
    pub span: SourceSpan,
}

pub(crate) struct LuaHandlerName(String);
```

Field meanings and examples:

- `rules` contains TOML-declared custom rule metadata. Example:
  `payments/max-token-budget -> { title, help, severity }`.
- `files` contains auto-discovered Lua files such as `lint/payments.lua`.
- `registrations` contains the bindings produced when those files run
  `lint:on(...)`.
- `CustomRuleDeclarationNode.severity` defaults to `Error` when omitted in
  TOML.
- `CustomLintRegistration.stage` says when the handler runs. A token budget
  check normally uses `LintStage::Value` because it needs projected and
  type/schema-validated values.
- `CustomLintRegistration.target` says what the handler receives. A value
  budget check uses `entity = Value` and `field = value.max_output_tokens`.
- `handler` is the Lua function name, such as `"check_token_budget"`.
- `span` points at the registration or, if the Lua source cannot provide that,
  the whole lint file.

Example registry:

```text
CustomLintRegistry {
  rules: {
    "payments/max-token-budget": {
      title: "Token budget exceeds payments policy",
      help: "Lower max_output_tokens or update the payments policy.",
      severity: Error,
    },
  },
  files: {
    "lint/payments.lua": CustomLintFileNode { doc: DocId(40) },
  },
  registrations: [
    CustomLintRegistration {
      file: "lint/payments.lua",
      rule: "payments/max-token-budget",
      stage: Value,
      target: Value(JsonPath(["max_output_tokens"])),
      handler: "check_token_budget",
    },
  ],
}
```

### Target Selectors

```rust
pub(crate) struct LintTargetSelector {
    pub entity: LintEntity,
    pub field: Option<FieldSelector>,
}

pub(crate) enum LintEntity {
    Workspace,
    Qualifier,
    Predicate,
    Variable,
    Value,
    EnvironmentBlock,
    Rule,
    Schema,
}

pub(crate) enum FieldSelector {
    Workspace(WorkspaceField),
    Qualifier(QualifierField),
    Predicate(PredicateField),
    Variable(VariableField),
    Value(ValueField),
    EnvironmentBlock(EnvironmentBlockField),
    Rule(RuleField),
    Schema(SchemaField),
}

pub(crate) enum WorkspaceField {
    Environments,
    ContextSchema,
    Lint,
}

pub(crate) enum QualifierField {
    Id,
    Description,
    Predicates,
}

pub(crate) enum PredicateField {
    Attribute,
    Op,
    Value,
    BucketSalt,
    BucketRange,
}

pub(crate) enum VariableField {
    Id,
    Description,
    Type,
    Schema,
    Values,
    Environments,
}

pub(crate) enum ValueField {
    Key,
    Value,
    JsonPath(JsonPathSelector),
}

pub(crate) enum EnvironmentBlockField {
    Environment,
    Value,
    Rules,
}

pub(crate) enum RuleField {
    Qualifier,
    Value,
    Description,
}

pub(crate) enum SchemaField {
    Json,
    JsonPath(JsonPathSelector),
}

pub(crate) struct JsonPathSelector(Vec<String>);
```

The first implementation can support only a conservative subset:

- `entity = "workspace"` with fields `environments`, `context_schema`;
- `entity = "qualifier"` with fields `description`, `predicates`;
- `entity = "variable"` with fields `description`, `type`, `schema`,
  `values`, `environments`;
- `entity = "value"` with `field = "value"` or `field = "value.<path>"`;
- `entity = "schema"` with `field = "json.<path>"`.

Lua registration fields are strings. The host validates those strings during
Register and converts them into the enum-backed selectors above.

| Lua `entity` | Accepted `field` strings | Internal selector |
|--------------|--------------------------|-------------------|
| `workspace` | omitted, `environments`, `context_schema` | `None`, `Workspace(Environments)`, `Workspace(ContextSchema)` |
| `qualifier` | omitted, `id`, `description`, `predicates` | `None`, `Qualifier(...)` |
| `predicate` | omitted, `attribute`, `op`, `value`, `bucket.salt`, `bucket.range` | `None`, `Predicate(...)` |
| `variable` | omitted, `id`, `description`, `type`, `schema`, `values`, `environments` | `None`, `Variable(...)` |
| `value` | omitted, `key`, `value`, `value.<json-path>` | `None`, `Value(Key)`, `Value(Value)`, `Value(JsonPath(...))` |
| `environment` | omitted, `environment`, `value`, `rules` | `None`, `EnvironmentBlock(...)` |
| `rule` | omitted, `qualifier`, `value`, `description` | `None`, `Rule(...)` |
| `schema` | omitted, `json`, `json.<json-path>` | `None`, `Schema(Json)`, `Schema(JsonPath(...))` |

`<json-path>` is a dot-separated sequence of object keys. Each segment must use
ASCII letters, digits, underscore, or hyphen and must not be empty. Array
indexes, wildcards, recursive descent, quoted segments, and escaping are not
part of the first implementation. If a registration uses a field string that is
not allowed for its entity, has an invalid JSON path segment, or references an
unknown entity/stage/rule/handler, Register emits
`rototo/custom-lint-registration-invalid`.

The enum leaves room for finer targeting without inventing a stringly typed
contract later.

Field meanings and examples:

- `LintTargetSelector.entity` chooses the semantic entity set. `Value` means
  "run once per `ValueNode`".
- `field` optionally narrows that entity. `Value(JsonPath(["max_output_tokens"]))`
  means "inside each value object, focus on `max_output_tokens` when it
  exists."
- `WorkspaceField::Environments` targets the manifest environment list.
- `QualifierField::Predicates` targets the predicate list as a whole, while
  `PredicateField::Attribute` targets each predicate attribute.
- `VariableField::Values` targets the value map as a whole. `LintEntity::Value`
  is better when a handler should run once per value key.
- `RuleField::Qualifier` targets the qualifier reference inside each variable
  rule.
- `SchemaField::Json` targets the schema document as a whole.
- `SchemaField::JsonPath(["properties", "model"])` targets part of a JSON
  schema when JSON path spans become available.

Example selectors:

```text
LintTargetSelector {
  entity: Workspace,
  field: Some(Workspace(Environments)),
}

LintTargetSelector {
  entity: Value,
  field: Some(Value(JsonPath(["max_output_tokens"]))),
}

LintTargetSelector {
  entity: Rule,
  field: Some(Rule(Qualifier)),
}
```

### Lua Registration API

Proposed Lua shape:

```lua
function register(lint)
  lint:on({
    stage = "value",
    entity = "value",
    field = "value.max_output_tokens",
    rule = "payments/max-token-budget",
    handler = "check_token_budget",
  })
end

function check_token_budget(ctx)
  local budget = ctx.target.value.max_output_tokens
  if budget ~= nil and budget > 5000 then
    return {
      {
        message = ctx.target.variable.id .. "." .. ctx.target.name
          .. " exceeds 5000 output tokens"
      }
    }
  end
  return {}
end
```

Important rules:

- `register(lint)` is called once during the registration step.
- A registration must name a declared custom rule id.
- The handler returns diagnostics without rule metadata. The registration owns
  the rule id, severity, title, and help.
- A handler may optionally return a more specific field path. If it does not,
  the diagnostic falls back to the target span.
- A returned diagnostic `field` uses the same field grammar as the
  registration target and is interpreted relative to the handler target. Invalid
  returned fields fall back to the target span rather than failing the whole
  lint run.
- Custom Lua still runs through `spawn_blocking` because `mlua` execution is
  synchronous.

## Lint Diagnostics

Diagnostics are LSP-ready at the lint boundary. The CLI can print a path-only
view for humans, but the SDK and JSON output should expose the richer location
model directly.

```rust
pub struct LintDiagnostic {
    pub rule: DiagnosticRule,
    pub severity: Severity,
    pub stage: LintStage,
    pub entity: EntityId,

    pub message: String,
    pub help: String,

    /// Primary location for CLI path output and LSP range output.
    pub primary: DiagnosticLocation,

    /// Optional secondary locations. Example: a duplicate rule declaration can
    /// point at both declarations.
    pub related: Vec<RelatedLocation>,
}

pub enum DiagnosticLocation {
    Span(SourceSpan),
    Document(DocId),
    WorkspaceRoot,
}

pub struct RelatedLocation {
    pub location: DiagnosticLocation,
    pub message: String,
}

impl DiagnosticLocation {
    pub fn doc(&self) -> Option<DocId> {
        match self {
            DiagnosticLocation::Span(span) => Some(span.doc),
            DiagnosticLocation::Document(doc) => Some(*doc),
            DiagnosticLocation::WorkspaceRoot => None,
        }
    }
}
```

Field meanings and examples:

- `rule` is the stable diagnostic identity, such as
  `rototo/variable-unknown-value` or `payments/max-token-budget`.
- `severity` is `Error` or `Warning`.
- `stage` tells callers where the diagnostic was produced. An unknown
  qualifier reference is a `Reference` diagnostic; a schema mismatch is a
  `Value` diagnostic.
- `entity` identifies the semantic owner. For an unknown value inside
  `variables/llm-agent-config.toml`, the entity can be
  `Rule { variable: "llm-agent-config", environment: "prod", index: 0 }`.
- `message` is concrete and local to the failing input.
- `help` comes from built-in rule metadata or custom rule metadata.
- `primary` is the main location. For LSP, `Span` maps to an exact range. For
  CLI, the same location maps to a path plus optional line/column.
- `Document(DocId)` is used when lint knows the document but cannot point to a
  precise range. For LSP it maps to a whole-document diagnostic range or the
  first line, depending on client behavior.
- `WorkspaceRoot` is reserved for genuinely document-less diagnostics such as
  a missing manifest or missing workspace. It is not included in per-document
  LSP diagnostic groups.
- There is no path-only diagnostic location. Once a file is discovered,
  `SourceStore.by_path` must resolve it to a `DocId`, and diagnostics should
  use either `Span` or `Document`.
- `related` stores secondary locations. A duplicate custom rule declaration can
  use one primary location and one related location for the earlier
  declaration.
- JSON diagnostics expose locations as path plus LSP-style line/character
  ranges when a precise span exists. Document-only diagnostics serialize with
  `path` and no `range`. Workspace-root diagnostics serialize with the
  workspace path and no document range. Byte offsets stay internal. A precise
  diagnostic location serializes as:

```json
{
  "path": "variables/checkout-redesign.toml",
  "range": {
    "start": { "line": 14, "character": 10 },
    "end": { "line": 14, "character": 22 }
  }
}
```

Example diagnostic:

```text
LintDiagnostic {
  rule: rototo/variable-unknown-value,
  severity: Error,
  stage: Reference,
  entity: Rule {
    variable: "checkout-redesign",
    environment: "prod",
    index: 0,
  },
  primary: Span("variables/checkout-redesign.toml", bytes 284..296),
  message: "rule references unknown value: enterprise",
  help: "Create the referenced value under [values] or update the reference.",
}
```

### Diagnostic Ordering

Before returning `WorkspaceLint`, diagnostics should be sorted into a canonical
order independent of stage execution order:

```text
(
  primary document path,          // WorkspaceRoot sorts before documents
  primary range start byte,       // Document and WorkspaceRoot use 0
  rule id,
  message
)
```

Document path ordering uses normalized `WorkspacePath`, not absolute paths.
For `Span` locations, the range start is the byte offset in the current
document text. For `Document(DocId)`, the start is `0`. This keeps CLI JSON,
human output, tests, and LSP publishing stable even if stage internals change.

## Lint Result

`WorkspaceLint` should become the public, serializable lint result. It can
still offer a flat iterator for CLI rendering, but the primary model should
group diagnostics by document for editor integrations.

```rust
pub struct WorkspaceLint {
    pub root: PathBuf,
    pub documents: Vec<SourceDocumentSummary>,
    pub diagnostics: Vec<LintDiagnostic>,
}

pub struct SourceDocumentSummary {
    pub id: DocId,
    pub path: WorkspacePath,
    pub uri: DocumentUri,
    pub version: Option<i32>,
    pub kind: SourceKind,
}

impl WorkspaceLint {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }

    pub fn diagnostics_for_doc(&self, doc: DocId) -> impl Iterator<Item = &LintDiagnostic> {
        self.diagnostics
            .iter()
            .filter(move |diagnostic| diagnostic.primary.doc() == Some(doc))
    }
}

pub(crate) struct WorkspaceLintSnapshot {
    pub lint: WorkspaceLint,
    pub index: SemanticIndex,
}
```

Field meanings and examples:

- `root` is the canonical workspace root.
- `documents` includes every document considered by lint, even documents with
  zero diagnostics. This is required so an LSP client can clear stale
  diagnostics by publishing an empty list for a document.
- `diagnostics` is the flat list for the workspace. It can be grouped by
  `diagnostics_for_doc`. Every file-owned diagnostic has a `DocId`, so
  per-document grouping is total for discovered documents.
- `WorkspaceLintSnapshot` is crate-private. It keeps the `SemanticIndex` next
  to the public lint result for LSP features such as hover, completion, and
  go-to-definition.
- `SemanticIndex` is not part of the public or JSON lint result. It owns full
  source text through `SourceStore` and non-serializable schema validators, so
  exposing it would break `--json` and leak implementation detail.
- A future LSP API can expose a narrow `WorkspaceSymbols` view derived from
  `WorkspaceLintSnapshot.index`.
- `SourceDocumentSummary` is safe to serialize. It omits full document text but
  keeps enough identity for diagnostics and editor integration.

Example result:

```text
WorkspaceLint {
  root: "/repo/examples/basic",
  documents: [
    { id: DocId(0), path: "rototo-workspace.toml", kind: Manifest },
    { id: DocId(12), path: "variables/checkout-redesign.toml", kind: Variable },
  ],
  diagnostics: [
    LintDiagnostic { rule: rototo/variable-unknown-value, ... },
  ],
}
```

## Lint Stages

Stages are ordered. Every stage receives the current index and diagnostics so
far. A stage can enrich the index, gate invalid entities, and emit diagnostics.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum LintStage {
    Discover,
    Parse,
    Project,
    Register,
    Reference,
    Value,
    Graph,
    Policy,
}
```

User-facing custom registrations can target `Project`, `Reference`, `Value`,
`Graph`, and `Policy`. They cannot target `Discover`, `Parse`, or `Register`,
because the semantic target set does not exist yet.

### Stage Responsibilities

| Stage | Purpose | Examples of built-in diagnostics |
|-------|---------|-----------------------------------|
| `Discover` | Resolve root, find manifest, and enumerate conventional workspace files. | `workspace-not-found`, `workspace-manifest-missing` |
| `Parse` | Parse TOML and JSON, load Lua text, create syntax documents. | `workspace-manifest-parse-failed`, `qualifier-parse-failed`, `variable-parse-failed`, `schema-parse-failed` |
| `Project` | Convert syntax into typed semantic nodes and validate local shape. Merge external values. Compile schemas. | `workspace-manifest-schema-failed`, `qualifier-schema-version`, `qualifier-predicate-shape`, `variable-type-or-schema`, `variable-env-shape`, `variable-external-value-*`, `schema-invalid`, `custom-lint-rule-shape`, `custom-lint-invalid-rule`, `custom-lint-rule-conflict` |
| `Register` | Execute `register(lint)` in auto-discovered `lint/*.lua` files and validate registrations against declared rule metadata. | `custom-lint-failed`, `custom-lint-registration-invalid`, `custom-lint-unknown-rule` |
| `Reference` | Resolve cross-entity references using the completed index. | `qualifier-predicate-unknown-qualifier`, `variable-rule-unknown-qualifier`, `variable-unknown-value`, `variable-unknown-environment`, `variable-schema-ref`, `workspace-context-schema-attribute` |
| `Value` | Validate variable values against primitive types or schemas. | `variable-unknown-type`, `variable-value-type-mismatch`, `variable-value-schema-mismatch` |
| `Graph` | Validate whole-workspace graph properties. | `qualifier-cycle`, `qualifier-unreferenced`, `variable-rule-shadowed`, `variable-value-unused` |
| `Policy` | Final workspace policies that need the full checked index. | custom policy checks first; built-ins can be added later |

Existing built-in rule ids should stay where the user-facing failure still
exists. `rototo/variable-lint-shape` is the exception because variable-scoped
`[lint]` is being removed.

## Pipeline Execution

Concrete engine sketch:

```rust
pub(crate) struct LintInput {
    pub root: PathBuf,
    pub overlays: BTreeMap<WorkspacePath, OverlayDocument>,
}

pub(crate) struct OverlayDocument {
    pub version: Option<i32>,
    pub text: Arc<str>,
}

pub(crate) struct LintContext {
    pub input: LintInput,
    pub syntax: SyntaxIndex,
    pub index: SemanticIndex,
    pub diagnostics: Vec<LintDiagnostic>,
}

pub(crate) struct LintEngine {
    custom_runner: CustomLintRunner,
}

impl LintEngine {
    pub async fn lint_workspace(
        &self,
        input: LintInput,
    ) -> Result<WorkspaceLint> {
        let mut ctx = LintContext::new(input);

        self.run_discover(&mut ctx).await?;
        self.run_parse(&mut ctx).await?;
        self.build_projection(&mut ctx).await?;
        self.run_register(&mut ctx).await?;

        for stage in [
            LintStage::Project,
            LintStage::Reference,
            LintStage::Value,
            LintStage::Graph,
            LintStage::Policy,
        ] {
            self.run_builtin_stage(stage, &mut ctx)?;
            self.run_custom_stage(stage, &mut ctx).await?;
        }

        Ok(ctx.finish())
    }
}
```

### Error Boundary

`lint_workspace` should reserve `Result::Err` for failures outside the
workspace-content contract:

- source staging failures before a workspace root is available, such as git,
  archive, network, or temporary-directory failures;
- process-level failures such as cancellation, task join failures, or
  unexpected Lua host setup failures before they can be tied to a lint file;
- internal invariant violations that indicate a rototo bug rather than an
  invalid workspace.

Everything about authored workspace content should become a diagnostic instead
of an `Err`: missing manifest, invalid TOML or JSON, unreadable discovered
files when a workspace path is known, unsafe paths, invalid schemas, unknown
references, invalid custom registrations, and custom handler failures. This
keeps CLI lint able to report as many workspace problems as possible in one
run.

`build_projection` creates typed semantic nodes but does not run project-stage
lint checks. `Project` then runs once in the normal stage loop:

1. `build_projection` creates `ManifestNode`, `QualifierNode`, `VariableNode`,
   `ValueNode`, and `SchemaNode` data.
2. `run_register` executes Lua registration after the manifest and custom rule
   declarations are known.
3. `run_builtin_stage(Project)` emits built-in shape diagnostics.
4. `run_custom_stage(Project)` runs custom checks against the completed typed
   projection.

That means a custom lint can register for `stage = "project"` and validate
local entity shape after rototo has parsed and projected the entity.

Field meanings and examples:

- `LintInput.root` is the staged workspace path. For a remote source, this is
  the temporary staged checkout/archive root, not the original URI.
- `LintInput.overlays` is empty for CLI/SDK disk lint. LSP lint populates it
  with unsaved document text keyed by `WorkspacePath`.
- `OverlayDocument.version` is the LSP version sent by the editor. A CLI run
  never sets it.
- `LintContext.syntax` starts empty and is filled by `run_parse`.
- `LintContext.index` starts with source/discovery data and is filled by
  `build_projection`, reference construction, and custom registration.
- `LintContext.diagnostics` accumulates diagnostics from every stage in stable
  order.
- `LintEngine.custom_runner` owns Lua execution and marshaling.

Example LSP input:

```text
LintInput {
  root: "/repo/config",
  overlays: {
    "variables/checkout-redesign.toml": OverlayDocument {
      version: 12,
      text: "<unsaved TOML from editor>",
    },
  },
}
```

## Built-In Stage Functions

Built-in checks should be ordinary stage functions. They are not dynamically
installed, reordered, or selected by user input, so they do not need a registry,
target enum, or function pointer table.

```rust
fn run_builtin_stage(stage: LintStage, ctx: &mut LintContext) -> Result<()> {
    match stage {
        LintStage::Project => builtins::run_project(ctx),
        LintStage::Reference => builtins::run_reference(ctx),
        LintStage::Value => builtins::run_value(ctx),
        LintStage::Graph => builtins::run_graph(ctx),
        LintStage::Policy => builtins::run_policy(ctx),
        LintStage::Discover | LintStage::Parse | LintStage::Register => Ok(()),
    }
}
```

Each built-in stage function can call smaller domain functions, for example
`builtins::qualifier::check_predicate_shape(ctx)` or
`builtins::variable::check_env_references(ctx)`. Those functions inspect
`SemanticIndex` directly and emit diagnostics through `LintStageContext`.

## Stage Context and Custom Target Resolution

Every built-in stage function receives a bounded view of the index and a way to
emit diagnostics. Built-ins inspect the index directly. Data-driven target
resolution is reserved for custom lint registrations.

```rust
pub(crate) struct LintStageContext<'a> {
    pub stage: LintStage,
    pub index: &'a SemanticIndex,
    pub diagnostics: &'a mut Vec<LintDiagnostic>,
}

pub(crate) struct LintTarget<'a> {
    pub entity: EntityId,
    pub span: SourceSpan,
    pub data: LintTargetData<'a>,
}

pub(crate) enum LintTargetData<'a> {
    Workspace(&'a SemanticIndex),
    Qualifier(&'a QualifierNode),
    Predicate(&'a PredicateNode),
    Variable(&'a VariableNode),
    Value(&'a ValueNode),
    EnvironmentBlock(&'a EnvironmentBlockNode),
    Rule(&'a VariableRuleNode),
    Schema(&'a SchemaNode),
}
```

Custom registration execution uses target resolution:

```rust
impl SemanticIndex {
    pub fn targets_for<'a>(
        &'a self,
        selector: &LintTargetSelector,
        stage: LintStage,
    ) -> Vec<LintTarget<'a>> {
        // 1. Select entities of the requested kind.
        // 2. Drop targets whose coarse GateEntity is blocked before this stage.
        // 3. If a field selector is present, narrow the target span/data.
        // 4. Return stable sorted targets for deterministic output.
    }
}
```

Deterministic target order is important because CLI JSON output and tests
should be stable.

`targets_for` maps each fine-grained `EntityId` to its owning `GateEntity`
before applying gates. For example, a `Rule` target is skipped only when its
owning `Variable` is gated; a malformed sibling rule in the same file does not
automatically hide it.

## Full Data Flow

### CLI and SDK Workspace Lint

```text
rototo workspace lint <source>
  -> stage_workspace_source(source)
  -> LintInput::from_workspace_root(staged.path())
  -> LintEngine::lint_workspace(input)
     -> Discover
        - canonicalize root
        - find rototo-workspace.toml
        - discover qualifiers/*.toml
        - discover variables/*.toml
        - discover schemas/*.json
     -> Parse
        - read and parse the manifest first
        - auto-discover lint/*.lua files
        - read text through SourceStore
        - parse TOML/JSON
        - keep Lua text
     -> Project
        - build ManifestNode
        - build QualifierNode values
        - build VariableNode values and envs
        - merge external values into ValueNode map
        - build SchemaNode validators
        - emit local shape diagnostics
     -> Register
        - execute register(lint) in each lint file
        - validate stage/entity/field/rule/handler
        - emit registration diagnostics
     -> Reference
        - build and validate ReferenceIndex
        - emit unknown-reference diagnostics
     -> Value
        - validate primitive and schema-backed values
     -> Graph
        - evaluate reference graph rules
     -> Policy
        - run custom final policies
  -> return WorkspaceLint
  -> print output
  -> exit failure only if at least one error diagnostic exists
```

`Workspace::load` uses the same engine. It rejects the workspace only when the
`WorkspaceLint` result has at least one error. Warnings do not block loading
once warning severity exists.

### Qualifier and Variable Lint

`lint_qualifier(root, id)` and `lint_variable(root, id)` still run the full
workspace pipeline. They then filter diagnostics by entity:

```rust
fn diagnostic_belongs_to_entity(diagnostic: &LintDiagnostic, entity: &EntityId) -> bool {
    diagnostic.entity == *entity
        || diagnostic.related.iter().any(|related| related_mentions_entity(related, entity))
}
```

Path filtering should not be the primary mechanism. Diagnostics carry
`EntityId`, so noun-specific lint can select diagnostics by semantic owner and
then render those diagnostics through the same document/span model.

Full-pipeline execution is important: a variable lint needs to know whether its
rule references an unknown qualifier, and that requires a workspace index.

### LSP Lint

The future LSP should use the same engine with overlays:

```text
initialize/open workspace
  -> build full WorkspaceSnapshot
  -> publish diagnostics per document

didOpen/didChange document
  -> update SourceStore overlay
  -> rebuild syntax for changed document
  -> re-project affected semantic entity
  -> rebuild ReferenceIndex and graph summaries
  -> rerun Reference, Value, Graph, Policy as needed
  -> publish diagnostics for every affected document, including empty lists

didSave document
  -> remove or refresh overlay
  -> rerun as above
```

The first implementation can rebuild the full snapshot on every LSP change.
The data structures should still preserve the information needed for
incremental rebuild.

## Incremental Rebuild Model

Defer a dedicated `DependencyGraph` until the LSP actually narrows rebuilds.
The first LSP implementation should rebuild broadly:

1. Reparse and reproject entities owned by the changed document.
2. Rebuild all references.
3. Re-run reference, value, graph, and policy stages workspace-wide.

That is enough for correctness and keeps the first implementation smaller. The
reverse edges already stored in `ReferenceIndex` are enough for
go-to-definition and find-references. Add a separate dependency graph only when
the server starts skipping unaffected stage work.

When that time comes, the dependency graph can track facts such as:

- `variables/foo.toml` defines `Variable(foo)`, its env blocks, rules, and
  inline values.
- `variables/foo-values/enterprise.toml` defines the external value
  `enterprise` and affects `Variable(foo)` value validation.
- `qualifiers/premium.toml` defines `Qualifier(premium)` and affects variables
  that reference it.
- `schemas/foo.schema.json` affects variables whose `schema` points at it.
- `lint/platform.lua` affects every target selected by its registrations.

## LSP Features Enabled by the Index

The same index supports more than diagnostics:

- Go-to-definition:
  - rule `qualifier = "premium-users"` -> `QualifierNode.id.span`;
  - predicate `attribute = "qualifier.foo"` -> `QualifierNode.id.span`;
  - env/rule `value = "enterprise"` -> `ValueNode.key.span`;
  - `schema = "../schemas/foo.json"` -> `SchemaNode.span`.
- Find references:
  - use `qualifier_referenced_by`;
  - use `value_referenced_by`;
  - use schema and context reference edges.
- Hover:
  - variable description, type/schema, and value keys;
  - qualifier description and predicate summary;
  - diagnostic rule title/help;
  - custom rule metadata.
- Completion:
  - environment names from `ManifestNode`;
  - qualifier ids from `qualifiers`;
  - value keys from the current `VariableNode`;
  - predicate operators from `PredicateOp`;
  - field selectors from the custom-lint target vocabulary.
- Document symbols:
  - workspace environments;
  - qualifier ids and predicates;
  - variable ids, values, env blocks, rules.

This is why source spans and reverse references are part of the first index
design instead of an LSP add-on.

## Custom Lint Execution Details

Custom lint is run after built-ins for the same stage:

```text
for stage in [Project, Reference, Value, Graph, Policy] {
  run_builtin_stage(stage)
  run_custom_stage(stage)
}
```

`Register` collects custom registrations first. A malformed registration emits
a built-in diagnostic and is ignored.

Custom execution algorithm:

```rust
async fn run_custom_stage(ctx: &mut LintContext<'_>, stage: LintStage) {
    let registrations = ctx.index.custom_lints.registrations_for(stage);

    for registration in registrations {
        let targets = ctx.index.targets_for(&registration.target, stage);

        for target in targets {
            let lua_ctx = CustomLuaContext::from_target(&ctx.index, &target);
            let returned = custom_runner
                .call_handler(&registration, lua_ctx)
                .await;

            match returned {
                Ok(items) => {
                    emit_custom_diagnostics(registration, target, items);
                }
                Err(err) => {
                    emit_custom_lint_failed(registration, target, err);
                }
            }
        }
    }
}
```

Lua handler context should be stable and small:

```rust
pub(crate) struct CustomLuaContext<'a> {
    pub stage: LintStage,
    pub target: CustomLuaTarget<'a>,
    pub workspace: CustomLuaWorkspaceView<'a>,
}

pub(crate) struct CustomLuaWorkspaceView<'a> {
    pub root: &'a Path,
    pub environments: Vec<&'a str>,
    pub qualifiers: Vec<&'a str>,
    pub variables: Vec<&'a str>,
}

pub(crate) enum CustomLuaTarget<'a> {
    Workspace { environments: Vec<&'a str> },
    Qualifier { id: &'a str, path: &'a str, predicates: serde_json::Value },
    Variable { id: &'a str, path: &'a str, toml: serde_json::Value },
    Value {
        variable_id: &'a str,
        name: &'a str,
        value: serde_json::Value,
        origin_path: &'a str,
    },
    Schema { path: &'a str, json: serde_json::Value },
}
```

This preserves the current Lua ergonomics while making the target explicit.

Returned Lua diagnostics:

```lua
return {
  {
    message = "enterprise.max_output_tokens exceeds 5000",
    field = "value.max_output_tokens" -- optional, relative to the target
  }
}
```

The host fills in:

- rule id from `CustomLintRegistration.rule`;
- severity/title/help from `CustomRuleDeclarationNode`;
- span from the selected target or returned field;
- entity from the target.

## Built-In Rule Set

The built-in linter owns `rototo/*` rules. Custom policy findings use declared
custom ids such as `payments/max-token-budget`; the built-in linter only
validates that custom rules are declared, registered, and executed correctly.

This table is the target rule set for the new pipeline. Existing rule ids are
kept where they still describe the same user-facing failure. Variable-scoped
`[lint]` is removed, so `rototo/variable-lint-shape` is retired and replaced by
workspace-level custom rule and registration diagnostics.

### Discover Rules

| Rule | Severity | Entity | Checks |
|------|----------|--------|--------|
| `rototo/workspace-not-found` | error | Workspace | The workspace root exists and is a directory after source staging. |
| `rototo/workspace-manifest-missing` | error | Workspace | `rototo-workspace.toml` exists at the workspace root. |

### Parse Rules

| Rule | Severity | Entity | Checks |
|------|----------|--------|--------|
| `rototo/workspace-manifest-parse-failed` | error | Workspace | The manifest is valid TOML. |
| `rototo/qualifier-parse-failed` | error | Qualifier | A `qualifiers/*.toml` file is valid TOML. |
| `rototo/variable-parse-failed` | error | Variable | A `variables/*.toml` file is valid TOML. |
| `rototo/variable-external-value-parse-failed` | error | Value | A `<variable-id>-values/*.toml` external value file is valid TOML. |
| `rototo/schema-parse-failed` | error | Schema | A `schemas/*.json` file is valid JSON. |

Lua files are loaded as source during Parse, but registration execution happens
in Register. Lua syntax or runtime failures are reported as
`rototo/custom-lint-failed`.

### Project Rules

| Rule | Severity | Entity | Checks |
|------|----------|--------|--------|
| `rototo/workspace-manifest-schema-failed` | error | Manifest | The manifest declares `schema_version = 1` and non-empty `[environments].values`. |
| `rototo/workspace-context-schema-ref` | error | Manifest | `[context].schema`, when present, is root-relative, stays inside the workspace, and points at a readable JSON Schema. |
| `rototo/qualifier-schema-version` | error | Qualifier | A qualifier declares `schema_version = 1`. |
| `rototo/qualifier-predicate-missing` | error | Qualifier | A qualifier has at least one `[[predicate]]`. |
| `rototo/qualifier-predicate-shape` | error | Predicate | Each predicate is a table with the required `attribute`, `op`, and `value` shape. |
| `rototo/qualifier-predicate-unknown-op` | error | Predicate | Predicate `op` is one of `eq`, `neq`, `in`, `not_in`, `gt`, `gte`, `lt`, `lte`, or `bucket`. |
| `rototo/qualifier-predicate-bucket` | error | Predicate | Bucket predicates include valid `salt` and `range = [start, end]` with `0 <= start < end <= 10000`. |
| `rototo/qualifier-predicate-value` | error | Predicate | Predicate `value` has the shape required by its operator. |
| `rototo/variable-schema-version` | error | Variable | A variable declares `schema_version = 1`. |
| `rototo/variable-type-or-schema` | error | Variable | A variable declares exactly one of `type` or `schema`. |
| `rototo/variable-values-missing` | error | Variable | A variable has at least one inline or external value. |
| `rototo/variable-env-missing-default` | error | Variable | A variable declares `[env._]`. |
| `rototo/variable-env-shape` | error | EnvironmentBlock | Each environment block is a table with a value reference and optional rules. |
| `rototo/variable-rule-shape` | error | Rule | Each rule has valid `qualifier` and `value` reference fields. |
| `rototo/variable-external-values-load-failed` | error | Variable | A sibling external values directory can be read and contains valid value files. |
| `rototo/variable-external-value-duplicate` | error | Value | Inline and external values do not declare the same value key more than once. |
| `rototo/schema-invalid` | error | Schema | A standalone schema file compiles as JSON Schema. |
| `rototo/custom-lint-rule-shape` | error | CustomRule | Each `[[lint.rule]]` declaration has `id`, `title`, `help`, and optional valid `severity`. |
| `rototo/custom-lint-invalid-rule` | error | CustomRule | A custom rule id uses `<authority>/<rule-id>`, does not use `rototo`, and uses lowercase ASCII letters, digits, and hyphens. |
| `rototo/custom-lint-rule-conflict` | error | CustomRule | Repeated declarations for the same custom rule id have identical metadata. |

### Register Rules

| Rule | Severity | Entity | Checks |
|------|----------|--------|--------|
| `rototo/custom-lint-failed` | error | CustomLintFile | A `lint/*.lua` file can execute `register(lint)` and later registered handlers without host or Lua errors. |
| `rototo/custom-lint-registration-invalid` | error | CustomLintFile | A registration names an allowed stage, entity, field selector, declared rule id, and callable handler. |
| `rototo/custom-lint-unknown-rule` | error | CustomLintFile | A registration references only a rule declared in workspace TOML. |

`rototo/custom-lint-failed` is emitted at `Register` when `register(lint)`
itself fails. If a registered handler fails while running at `Project`,
`Reference`, `Value`, `Graph`, or `Policy`, the same rule is emitted at that
handler's target stage, with a related location pointing back to the
registration site when available.

### Reference Rules

| Rule | Severity | Entity | Checks |
|------|----------|--------|--------|
| `rototo/workspace-context-schema-attribute` | error | Predicate | A context attribute used by a qualifier is declared by the workspace context schema when one exists. |
| `rototo/qualifier-predicate-unknown-qualifier` | error | Predicate | `attribute = "qualifier.<id>"` references an existing qualifier. |
| `rototo/variable-unknown-environment` | error | EnvironmentBlock | A variable environment block references `_` or an environment declared in the manifest. |
| `rototo/variable-unknown-value` | error | EnvironmentBlock or Rule | Environment defaults and rules reference known value keys for the variable. |
| `rototo/variable-rule-unknown-qualifier` | error | Rule | A variable rule references an existing qualifier. |
| `rototo/variable-schema-ref` | error | Variable | A schema-backed variable's file-relative schema path normalizes inside the workspace and resolves to a readable schema document. |

### Value Rules

| Rule | Severity | Entity | Checks |
|------|----------|--------|--------|
| `rototo/variable-unknown-type` | error | Variable | Primitive `type` is one of `bool`, `int`, `number`, `string`, or `list`. |
| `rototo/variable-value-type-mismatch` | error | Value | Every value of a primitive variable matches the declared primitive type. |
| `rototo/variable-value-schema-mismatch` | error | Value | Every value of a schema-backed variable validates against the referenced JSON Schema. |

### Graph Rules

| Rule | Severity | Entity | Checks |
|------|----------|--------|--------|
| `rototo/qualifier-cycle` | error | Qualifier | Qualifier references do not form cycles. |
| `rototo/qualifier-unreferenced` | warning | Qualifier | A qualifier is not referenced by another qualifier or by any variable rule. |
| `rototo/variable-rule-shadowed` | warning | Rule | The same qualifier id appears in an earlier rule in the same variable environment block, so the later rule can never win under the current first-match rule order. |
| `rototo/variable-value-unused` | warning | Value | A variable value is not referenced by any environment default or rule. |

`rototo/qualifier-cycle` is reported from strongly connected components in the
qualifier reference graph:

- A self-reference is a cycle and emits one diagnostic.
- A multi-qualifier cycle emits one diagnostic per participating qualifier.
- The primary span is the first source-ordered `qualifier.<id>` reference in
  that qualifier that points to another qualifier in the same component.
- Related locations should point to the other references that form the cycle
  when available.
- The graph walk must be cycle-safe, for example Tarjan/Kosaraju SCC traversal
  or an equivalent iterative algorithm. Lint reports cycles and continues; it
  must not use resolve-time recursion that errors out on the first cycle.

`rototo/variable-rule-shadowed` is intentionally narrow. It does not attempt to
prove general qualifier overlap, because qualifier predicates can overlap in
ways lint cannot decide statically. The first implementation should only flag
duplicate qualifier ids within one `[env.<id>]` rule list.

### Policy Rules

Policy is primarily for custom lint. The built-in linter should not start with
workspace-specific policy rules beyond custom execution diagnostics. Built-in
Policy rules can be added later when rototo has product-level policies that
need the complete checked graph.

### Retired Rules

| Rule | Replacement |
|------|-------------|
| `rototo/variable-lint-shape` | Removed with variable-scoped `[lint]`. Workspace-level `[[lint.rule]]` shape uses `rototo/custom-lint-rule-shape`; Lua registration shape uses `rototo/custom-lint-registration-invalid`. |
| `rototo/qualifier-missing-table` | Retired with the flattened qualifier TOML shape. Qualifier local shape is now checked by `rototo/qualifier-schema-version`, `rototo/qualifier-predicate-missing`, and `rototo/qualifier-predicate-shape`. |
| `rototo/variable-missing-table` | Retired with the flattened variable TOML shape. Variable local shape is now checked by `rototo/variable-schema-version`, `rototo/variable-type-or-schema`, `rototo/variable-values-missing`, `rototo/variable-env-shape`, and `rototo/variable-rule-shape`. |

Rule coverage tests should track retired ids explicitly so a removed rule is a
reviewed decision, not an accidental gap in the built-in catalog.

Adding warnings requires `Severity` to expand:

```rust
pub enum Severity {
    Error,
    Warning,
}
```

Pass/fail semantics:

- CLI lint exits non-zero only when at least one error exists.
- `Workspace::load` rejects only on errors.
- Warnings are still printed and serialized.
- The diagnostic catalog includes warning severity.

Warning suppression is deliberately out of scope for the first implementation.
Warnings such as `qualifier-unreferenced` and `variable-value-unused` will
likely create demand for workspace-scoped suppression, but adding a suppression
syntax before the warning set settles would make the public contract larger
than necessary. Treat suppression as a near-term follow-up design decision.

## Code Organization

The implementation should live under `src/lint/` with a small public facade and
private modules behind it. The goal is to make each module own one kind of
question:

- source modules answer "what text is in the workspace?"
- syntax modules answer "what did the text parse into?"
- projection modules answer "what rototo entities did the text define?"
- reference modules answer "what points at what?"
- stage modules answer "which checks run when?"
- custom modules answer "what user-defined handlers are registered and how are
  they called?"

Proposed layout:

```text
src/lint/
  mod.rs                 // public facade for lint_workspace/lint_qualifier/lint_variable
  engine.rs              // LintEngine, LintContext, stage orchestration
  input.rs               // LintInput, LintOptions, overlays
  output.rs              // WorkspaceLint construction and CLI/SDK helpers

  source/
    mod.rs               // SourceStore and SourceDocument
    discover.rs          // conventional workspace file discovery
    span.rs              // SourceSpan, TextRange, Spanned<T>
    line_index.rs        // byte offset <-> LSP line/character conversion

  syntax/
    mod.rs               // SyntaxIndex and ParsedDocument
    toml.rs              // toml-span adapter
    json.rs              // JSON parser and schema source spans
    lua.rs               // Lua source loading metadata

  index/
    mod.rs               // SemanticIndex
    ids.rs               // QualifierId, VariableId, EntityId, etc.
    nodes.rs             // ManifestNode, QualifierNode, VariableNode, ValueNode
    gates.rs             // coarse GateIndex and blocked entity state
    targets.rs           // LintTargetSelector and targets_for

  project/
    mod.rs               // projection entry point
    manifest.rs          // manifest projection and custom rule declarations
    qualifier.rs         // qualifier and predicate projection
    variable.rs          // variable, env, rule, inline value projection
    external_value.rs    // sibling <variable-id>-values/*.toml projection
    schema.rs            // schema node and validator projection

  references.rs          // ReferenceIndex construction and reverse edges

  stages/
    mod.rs               // LintStage and stage runner traits/helpers
    discover.rs          // Discover stage
    parse.rs             // Parse stage
    project.rs           // Project checks
    register.rs          // Register checks and Lua register(lint)
    reference.rs         // Reference checks
    value.rs             // Value checks
    graph.rs             // Graph checks
    policy.rs            // Policy checks and custom stage execution

  builtins/
    mod.rs               // built-in stage entry points
    workspace.rs         // workspace and manifest built-ins
    qualifier.rs         // qualifier and predicate built-ins
    variable.rs          // variable, env, rule, value built-ins
    schema.rs            // schema built-ins
    graph.rs             // graph built-ins

  custom/
    mod.rs               // CustomLintRegistry and public custom execution API
    registry.rs          // registration validation and storage
    lua_runner.rs        // async Lua execution boundary
    marshal.rs           // Rust <-> Lua target/diagnostic conversion
```

`src/diagnostics.rs` remains the public diagnostic catalog and rule-id home.
The lint module should use those rule definitions rather than defining a second
catalog.

### Dependency Rules

Keep dependencies pointed in one direction:

```text
source -> syntax -> project -> index -> references -> stages -> output
                                      \-> builtins
                                      \-> custom
```

Practical rules:

- `source`, `syntax`, `index`, and `project` do not call CLI, SDK, Lua, or
  output rendering code.
- `builtins` do not read files, parse TOML, execute Lua, or format CLI output.
  They inspect `SemanticIndex` through `LintStageContext`.
- `custom::lua_runner` is the only module that executes Lua.
- `stages` orchestrate work but do not contain detailed rule logic.
- `output` serializes and renders `WorkspaceLint`; it does not run checks.

This prevents the old pattern where lint logic, file I/O, parsing, and output
shape are coupled in one procedural flow.

### Mutation Boundaries

`LintContext` should be the only mutable state passed through the pipeline:

```rust
pub(crate) struct LintContext {
    pub input: LintInput,
    pub syntax: SyntaxIndex,
    pub index: SemanticIndex,
    pub diagnostics: Vec<LintDiagnostic>,
}
```

Each stage gets a narrow mutable capability:

- Discover can add `SourceDocument`s.
- Parse can add `ParsedDocument`s.
- Project can add semantic nodes and gates.
- Register can add custom registrations.
- Reference can rebuild `ReferenceIndex`.
- Value, Graph, and Policy can emit diagnostics. They should update gates only
  for fatal source-backed failures, not for ordinary rule failures.

Checks should not mutate arbitrary index state. If a check needs to block later
work, it should do that through a small gate API such as
`ctx.gate(gate_entity, stage, diagnostic_index)`. Gate targets should be
coarse `GateEntity` values, not individual predicates, rules, or fields.

### Public API Boundary

The public crate API should stay small:

```rust
pub async fn lint_workspace(root: &Path) -> Result<WorkspaceLint>;
pub async fn lint_qualifier(root: &Path, id: &str) -> Result<QualifierLint>;
pub async fn lint_variable(root: &Path, id: &str) -> Result<VariableLint>;
```

New internal entry points can exist for LSP and tests:

```rust
pub(crate) async fn lint_workspace_with_input(input: LintInput) -> Result<WorkspaceLint>;
pub(crate) async fn lint_workspace_snapshot(input: LintInput) -> Result<WorkspaceLintSnapshot>;
pub(crate) async fn lint_workspace_until(input: LintInput, stage: LintStage) -> Result<LintContext>;
```

`lint_workspace_with_input` returns the same public result shape as
`lint_workspace`, with overlay support. `lint_workspace_snapshot` additionally
keeps the private `SemanticIndex` for LSP symbol queries.

`lint_workspace_until` is a test helper. It lets tests stop after Parse,
Project, Reference, or Value and inspect the data structures directly. It
should stay crate-private.

### Test Organization

Tests should mirror the module boundaries:

```text
tests/lint_source.rs
tests/lint_syntax.rs
tests/lint_semantic_index.rs
tests/lint_references.rs
tests/lint_stages.rs
tests/lint_custom.rs
tests/lint_lsp_overlay.rs
tests/workspace_lint.rs        // CLI-facing integration behavior
```

Module-local unit tests are appropriate for small parsing and span helpers.
Workspace fixture tests belong in `tests/` so they exercise the same public or
crate-private entry points the CLI, SDK, and future LSP will use.

For the first slice, these modules can be private and some files can be folded
together if they are still small. The important part is preserving the
boundaries: file I/O, parsing, projection, rule execution, custom Lua, and
output should stay separable.

## Migration Plan

### Phase 1: Span-Aware Source and Index, Behavior Preserving

- Add `SourceStore`, `SourceSpan`, `SyntaxIndex`, and `SemanticIndex`.
- Build the index from disk for `lint_workspace`.
- Port existing built-in lint checks onto index data.
- Replace path-only public lint diagnostics with the LSP-ready
  `LintDiagnostic` and `WorkspaceLint` model.
- Add tests that guard against repeated file reads if practical.

Exit criteria: `just check` passes, existing fixtures still assert the same
rules, and tests assert document/span/entity/stage fields for representative
diagnostics.

### Phase 2: LSP Snapshot API

- Add a lint entry point that accepts overlay documents.
- Group diagnostics by document, including documents with zero diagnostics.
- Add line-index conversion helpers for LSP ranges.
- Rebuild broadly on document changes; defer dedicated dependency tracking
  until incremental narrowing is implemented.

Exit criteria: an unsaved document can be linted through an overlay without
writing it to disk, and the result can publish/clear diagnostics per document.

### Phase 3: Custom Lint Registration

- Move custom rule metadata to workspace-level `[lint]` declarations.
- Auto-discover `lint/*.lua` files.
- Add `register(lint)` collection.
- Add stage/entity/field registration validation.
- Run custom checks through `targets_for`.
- Remove variable-scoped `[lint]`. Custom lint is workspace-scoped through
  auto-discovered lint files and workspace-level rule declarations.

Exit criteria: custom checks can target workspace, qualifier, variable, value,
and schema entities.

### Phase 4: Severity and Graph Rules

- Add warning severity.
- Add graph rules for qualifier cycles, unreferenced qualifiers, shadowed rules,
  and unused values.
- Keep existing examples lint-clean, and begin splitting `examples/basic` into
  curated `quickstart`, `production`, and `custom-lint` examples.

Exit criteria: warnings do not fail lint; error graph rules block load.

### Phase 5: LSP Server

- Build the LSP server on top of the snapshot API.
- Start with diagnostics and document symbols.
- Add completion, hover, go-to-definition, and find-references once the symbol
  queries are stable.

Exit criteria: editing an unsaved workspace file produces diagnostics from the
same lint engine used by the CLI and SDK.

## Testing Strategy and Fixture Plan

The new lint engine should be developed under tests from the beginning. The
goal is not only to prove that diagnostics appear, but to prove that the
pipeline populated the right source, syntax, semantic, reference, and
diagnostic structures before each rule ran.

The current tests provide a useful migration baseline, not the final fixture
shape:

- `examples/basic` is the broad lint-clean integration workspace, but it grew
  organically and should be treated as transitional rather than as the final
  product example.
- `tests/fixtures/workspaces/lint-failures` is a compact failure workspace for
  broad CLI output checks.
- `tests/fixtures/workspaces/rule-coverage` makes current rule-id coverage
  explicit, but it does not isolate stage/entity/range behavior well enough for
  the new engine.
- `tests/workspace_lint.rs` already has a `covers_every_rototo_rule...` table
  that prevents adding a built-in rule without a fixture.

The new engine should keep the rule coverage discipline while replacing the
fixture structure with smaller canonical rule fixtures. Each rule fixture
should assert:

```rust
pub(crate) struct ExpectedDiagnostic {
    pub rule: &'static str,
    pub stage: LintStage,
    pub entity: ExpectedEntity,
    pub path: &'static str,
    pub range: ExpectedRange,
    pub related: &'static [ExpectedRelatedLocation],
}

pub(crate) struct ExpectedRange {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

pub(crate) enum ExpectedEntity {
    Workspace,
    Manifest,
    Qualifier(&'static str),
    Predicate { qualifier: &'static str, index: usize },
    Variable(&'static str),
    Value { variable: &'static str, key: &'static str },
    EnvironmentBlock { variable: &'static str, environment: &'static str },
    Rule { variable: &'static str, environment: &'static str, index: usize },
    Schema(&'static str),
    CustomRule(&'static str),
}
```

Field meanings and examples:

- `rule` is the stable diagnostic id, such as
  `rototo/variable-rule-unknown-qualifier`.
- `stage` proves the diagnostic came from the expected pipeline phase. An
  unknown qualifier in a variable rule should be `LintStage::Reference`, not
  `Project`.
- `entity` proves noun-specific lint and LSP symbol ownership work. Example:
  `Rule { variable: "checkout-redesign", environment: "prod", index: 0 }`.
- `path` and `range` prove spans survived source loading, TOML parsing,
  projection, and diagnostic rendering.
- `related` is used for diagnostics that need a secondary location, such as
  duplicate custom rule metadata or duplicate external values.

### Test Layers

1. Source and span tests.

   These are small Rust unit tests for `SourceStore`, `LineIndex`,
   `SourceSpan`, and `toml-span` adapters. They assert byte-to-LSP-range
   conversion, UTF-16 character handling, overlay precedence, workspace-path
   normalization, and path escape rejection.

2. Syntax projection tests.

   These parse a small TOML/JSON document and assert that the `SyntaxIndex`
   contains the expected root node, key spans, value spans, and parse failure
   locations. They should not involve semantic rules.

3. Semantic index tests.

   These build `SemanticIndex` from fixture workspaces and assert entity
   population: environments, qualifiers, predicates, variables, inline values,
   external values, schemas, custom rule declarations, gates, and source spans.
   They answer "did the index contain what the user wrote?"

4. Reference index tests.

   These assert exact edges: qualifier-to-qualifier, variable-rule-to-qualifier,
   variable-env-to-value, variable-schema-to-schema, and context-attribute
   references. They should include both resolved and unresolved edges so
   diagnostics and go-to-definition use the same data.

5. Stage tests.

   Each stage should be callable through a test harness. Tests can run
   `Discover -> Parse -> Project` and stop, or run through `Reference` and
   inspect gates and diagnostics. This keeps failures local when the pipeline
   changes.

6. Whole-workspace behavior tests.

   These are the current CLI/SDK style tests. They assert user-visible JSON,
   exit status, lint-clean examples, `Workspace::load` rejection on errors, and
   noun-specific `lint_qualifier` / `lint_variable` filtering.

7. Custom lint tests.

   These use workspace-level rule declarations plus auto-discovered
   `lint/*.lua`. They cover valid registration, invalid stage, invalid entity,
   invalid field, undeclared rule, duplicate/conflicting rule metadata, handler
   failure, handler skip, and custom diagnostics on workspace, qualifier,
   variable, value, and schema targets.

8. LSP overlay tests.

   These call the lint snapshot API directly with overlay text. They prove that
   unsaved text wins over disk, diagnostics are grouped by all considered
   documents, and stale diagnostics can be cleared with empty publish sets.

### Fixture Development

Fixtures should be hand-authored, reviewable workspaces. Generated fixtures are
less useful here because rototo's value is that engineers can inspect the
control-plane files directly.

Do not treat the current `rule-coverage` and `lint-failures` directories as
the final fixture architecture. They are useful, but they were built for the
current path-only linter. The new engine needs fixtures that make one expected
stage, entity, field span, or reference edge obvious.

### Example Workspace Strategy

`examples/basic` should also be rethought. It currently covers many useful
features, but the name and shape are misleading for the new lint engine:

- it is not basic; it includes many variables, many qualifiers, several
  schemas, external values, and custom Lua lint;
- it mixes several domains: checkout, payments, search, support, tenant limits,
  admin UI, and LLM configuration;
- it still demonstrates variable-scoped `[lint]`, which is being removed;
- many tests use it as a convenient stable workspace, which makes it harder to
  reshape into a clear teaching example.

Keep the current `examples/basic` during the migration so existing CLI, SDK,
docs, and lint-clean checks have a stable target. Do not use it as the primary
fixture for semantic index, LSP, or individual rule behavior.

The end state should split examples by role:

```text
examples/quickstart/
examples/production/
examples/custom-lint/
examples/basic/        # optional compatibility alias or removed after docs move
```

Recommended roles:

- `examples/quickstart` is the smallest complete workspace: one manifest, one
  primitive variable, one default environment block, and no custom lint. It is
  for first success and docs smoke tests.
- `examples/production` is the representative workspace. It should tell one
  coherent operational story, for example LLM agent configuration for
  enterprise accounts. It should include a context schema, qualifier
  composition, one schema-backed variable, external values, environment
  defaults, production rules, and observability-friendly descriptions.
- `examples/custom-lint` shows workspace-level `[[lint.rule]]` declarations and
  auto-discovered `lint/*.lua` registration with one or two custom policies.
- `examples/basic`, if kept, should become either a compatibility alias to the
  quickstart-style workspace or a deliberately small example. It should not be
  the broad coverage workspace.

The representative production example should be curated, not exhaustive. It
needs enough surface area to prove the model is real, but not every feature
rototo supports. Broad feature coverage belongs in fixtures and tests, not in
the introductory example.

### Fixture Families

Use these fixture families:

1. Curated examples

   `examples/quickstart`, `examples/production`, and `examples/custom-lint`
   should stay lint-clean. Tests can use them for smoke coverage and public
   examples, but exact rule assertions should live in canonical rule fixtures.

2. Legacy rule coverage: `tests/fixtures/workspaces/rule-coverage`

   This is the current `rule-coverage` workspace, kept only as a migration
   parity fixture. It proves the new engine still emits the same existing rule
   ids while old behavior is being ported. It should not become the main
   source of span, stage, or entity assertions because several failures share
   the same manifest and can influence each other.

3. Legacy lint failures: `tests/fixtures/workspaces/lint-failures`

   This is the current `lint-failures` workspace, kept as a broad integration
   fixture. It is valuable because it exercises many diagnostics in one CLI JSON
   run, including qualifiers, variables, external values, schemas, and custom
   lint. It is not suitable as the primary fixture for subtle behavior because
   the expected output is intentionally crowded.

The legacy directories do not need to be renamed during the first slice. Their
role should change: they guard migration parity and broad CLI rendering, while
new canonical fixtures become the source of exact rule expectations.

4. Canonical one-rule fixtures.

   Add one small workspace per built-in rule, grouped by pipeline stage:

   ```text
   tests/fixtures/workspaces/rules/discover/workspace-manifest-missing/
   tests/fixtures/workspaces/rules/parse/variable-parse-failed/
   tests/fixtures/workspaces/rules/project/variable-type-or-schema/
   tests/fixtures/workspaces/rules/reference/variable-rule-unknown-qualifier/
   tests/fixtures/workspaces/rules/value/variable-value-schema-mismatch/
   tests/fixtures/workspaces/rules/graph/qualifier-cycle/
   tests/fixtures/workspaces/rules/register/custom-lint-registration-invalid/
   ```

   Each of these fixtures should be as close to single-failure as possible. If
   a rule requires supporting valid files, those files should be present and
   lint-clean. The test table for built-in rules should point to these
   canonical fixtures, not to the legacy integration workspaces.

   Example expectation:

   ```rust
   ExpectedDiagnostic {
       rule: "rototo/variable-rule-unknown-qualifier",
       stage: LintStage::Reference,
       entity: ExpectedEntity::Rule {
           variable: "checkout-redesign",
           environment: "prod",
           index: 0,
       },
       path: "variables/checkout-redesign.toml",
       range: ExpectedRange {
           start_line: 14,
           start_character: 16,
           end_line: 14,
           end_character: 35,
       },
       related: &[],
   }
   ```

5. Focused contract fixtures.

   Add small directories when a behavior needs isolation:

   ```text
   tests/fixtures/workspaces/source-contract/
   tests/fixtures/workspaces/syntax-contract/
   tests/fixtures/workspaces/semantic-index-contract/
   tests/fixtures/workspaces/reference-index-contract/
   tests/fixtures/workspaces/custom-registration-contract/
   tests/fixtures/workspaces/lsp-overlay-contract/
   ```

   These should be minimal. For example,
   `reference-index-contract` can have one qualifier, one variable, one known
   value, one unknown value, one known qualifier reference, and one unknown
   qualifier reference. That gives resolved and unresolved edges without hiding
   the intent in a large workspace.

Fixture naming should describe the failure mode, not the implementation detail:

```text
variables/rule-unknown-qualifier.toml
variables/rule-unknown-value.toml
variables/schema-value-mismatch.toml
qualifiers/predicate-unknown-qualifier.toml
lint/invalid-stage.lua
lint/value-field-policy.lua
```

### Snapshot Policy

Avoid broad snapshots of full CLI JSON as the primary contract. They become
noisy whenever ordering or helpful metadata changes. Prefer targeted JSON
assertions for stable facts: rule id, severity, stage, entity, path, range, and
related locations.

Full JSON snapshots can be added later for one or two high-value examples, but
they should supplement the targeted assertions rather than replace them.

### Migration Testing

Because the lint internals are being replaced, keep a behavior parity harness
until the old implementation is removed:

1. Run existing fixtures through the old lint path and the new engine.
2. Compare the set of rule ids, severities, messages, and paths.
3. Allow the new engine to add ranges, stages, entities, related locations, and
   more precise locations.
4. Switch public APIs to the new engine only after parity passes for existing
   built-in rules.
5. Delete the old lint path after the new engine owns all existing fixtures.

This keeps the rewrite honest without forcing the new diagnostic model to mimic
the old path-only shape.

## Test Contract

Extend the current lint test contract:

- Existing rule ids still emit from existing fixtures after Phase 1.
- Every built-in rule has a canonical one-rule fixture under
  `tests/fixtures/workspaces/rules/<stage>/<rule-id>/`.
- Legacy `rule-coverage` and `lint-failures` fixtures remain migration and
  integration checks, not the source of exact stage/entity/range expectations.
- Diagnostic ordering tests prove output is sorted by document path, range
  start, rule id, and message rather than by incidental stage execution order.
- Error-boundary tests prove invalid workspace content becomes diagnostics,
  while source staging failures and internal invariant failures return `Err`.
- Warning fixtures prove lint output contains warnings while CLI exit status is
  success when no errors exist.
- Warning suppression is not implemented in this phase; tests should assert
  that warnings are emitted rather than suppressed.
- Graph fixtures cover qualifier self-cycles, multi-qualifier cycles, duplicate
  qualifier rule shadowing, unreferenced qualifiers, and unused values.
- Path-safety fixtures cover context schema refs, variable schema refs, and
  auto-discovered lint files with both valid normalized paths and escaping
  paths.
- Schema fixtures prove schema documents are deduplicated by normalized
  `WorkspacePath`, malformed referenced schemas emit one schema diagnostic, and
  value validation is skipped when the schema has no validator.
- Retired-rule tests explicitly document `variable-lint-shape`,
  `qualifier-missing-table`, and `variable-missing-table` as removed rules.
- Custom lint fixtures cover:
  - workspace-targeted registration;
  - qualifier-targeted registration;
  - value field registration;
  - field selector string grammar, including invalid field strings;
  - invalid stage/entity/field;
  - registration referencing an undeclared rule;
  - custom warning severity.
- LSP-oriented internal tests cover:
  - diagnostic span points at an offending qualifier reference;
  - diagnostic span points at an external value file;
  - diagnostics are grouped by all considered documents, including empty sets;
  - overlay text is linted without touching disk.
- Curated examples remain lint-clean. During migration this includes
  `examples/basic`; after docs move, it includes `examples/quickstart`,
  `examples/production`, and `examples/custom-lint`.

`just check` remains the release gate.

## Settled Decisions

1. TOML parser: use `toml-span` for the span-preserving TOML parse layer.
2. Custom lint discovery: auto-discover Lua registration files from
   `lint/*.lua`.
3. Field selector grammar: use a conservative enum-backed target registry, with
   limited JSON-path selectors only inside value and schema payloads.
4. Public diagnostic serialization is path plus LSP-style line/character
   ranges under `location` and `related[].location`. Byte offsets stay
   internal.
5. Variable-scoped `[lint]` is removed. Custom lint uses workspace-level rule
   declarations and auto-discovered `lint/*.lua` registration files.
