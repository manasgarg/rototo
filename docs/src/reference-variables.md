# Variables Reference

Applications ask rototo for variables. A variable gives a name to one runtime
configuration decision: which value should this request, account, or service
state receive from this workspace version?

Variable files live under `variables/*.toml`. The file stem is the variable id.

## Minimal Primitive Variable

```toml
schema_version = 1

description = "Maximum active projects for an account"
type = "int"

[values]
standard = 3
expanded = 25

[resolve]
default = "standard"

[[resolve.rule]]
qualifier = "paid-account"
value = "expanded"
```

## Fields

| Field | Required | Type | Meaning |
| --- | --- | --- | --- |
| `schema_version` | Yes | integer | Variable format version. The only supported value is `1`. |
| `description` | No | string | Human description shown by inspect and editor tooling. |
| `type` | Yes | string | Primitive type or `catalog:<catalog-id>`. |
| `values` | Primitive only | table | Inline values keyed by value name. |
| `resolve` | Yes | table | Default value and ordered qualifier rules. |

The older `schema = "...json"` field is rejected. Structured values should use
a [catalog](reference-catalogs.html) and `type = "catalog:<catalog-id>"`.

## Supported Types

Primitive variable types are:

```text
bool
int
number
string
list
```

Catalog-backed variable types use:

```text
catalog:<catalog-id>
```

For example:

```toml
type = "catalog:account-limit-profile"
```

## Primitive Values

Primitive variables must contain a `[values]` table. Each table key is a value
key. Each value must match the declared primitive type.

```toml
type = "string"

[values]
control = "standard"
priority = "priority"
```

Primitive variables without values are invalid. Catalog-backed variables with
`[values]` are invalid.

## Catalog-Backed Variables

Catalog-backed variables select entry keys from the matching catalog entry
directory:

```toml
schema_version = 1

description = "Account limit profile"
type = "catalog:account-limit-profile"

[resolve]
default = "growth"

[[resolve.rule]]
qualifier = "enterprise-account"
value = "enterprise"
```

The `default` and rule `value` fields reference files under:

```text
catalogs/account-limit-profile-entries/
```

Every selected entry is validated against the catalog schema before an
application can consume it.

## Resolve Block

Every variable must contain `[resolve]`:

```toml
[resolve]
default = "standard"

[[resolve.rule]]
qualifier = "paid-account"
value = "expanded"
```

`default` is required. It must reference a known primitive value key or a known
catalog entry key.

Rules are evaluated in file order. Each rule references one qualifier and one
value. The first rule whose qualifier resolves to `true` selects its value. If
no rule matches, the default value is selected.

## Invalid Shapes

These shapes are invalid:

```toml
# Missing type
schema_version = 1
```

```toml
# Schema-backed variables are no longer supported
schema = "../schemas/value.schema.json"
```

```toml
# Catalog-backed variables must not contain [values]
type = "catalog:account-limit-profile"

[values]
growth = {}
```

```toml
# Rules must use [[resolve.rule]] tables
[resolve]
default = "standard"
rule = ["paid-account"]
```

## Related Pages

See [Variable Values](reference-variable-values.html) for value validation, and
[Variable Resolution](reference-variable-resolution.html) for runtime selection
semantics.
