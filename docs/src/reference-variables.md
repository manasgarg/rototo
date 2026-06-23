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

[resolve]
default = 3

[[resolve.rule]]
when = 'qualifier["paid-account"]'
value = 25
```

## Fields

| Field | Required | Type | Meaning |
| --- | --- | --- | --- |
| `schema_version` | Yes | integer | Variable format version. The only supported value is `1`. |
| `description` | No | string | Human description shown by inspect and editor tooling. |
| `type` | Yes | string | Primitive type or `catalog:<catalog-id>`. |
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

Primitive variables put their values directly in `[resolve].default` and
`[[resolve.rule]].value`. Each value must match the declared primitive type.

```toml
type = "string"

[resolve]
default = "standard"

[[resolve.rule]]
when = 'qualifier["priority-account"]'
value = "priority"
```

Variables with `[values]` are invalid. That older indirection is rejected so
the configured value is visible where resolution selects it.

## Catalog-Backed Variables

Catalog-backed variables select named values from the matching catalog value
directory:

```toml
schema_version = 1

description = "Account limit profile"
type = "catalog:account-limit-profile"

[resolve]
default = "growth"

[[resolve.rule]]
when = 'qualifier["enterprise-account"]'
value = "enterprise"
```

The `default` and rule `value` fields name files under:

```text
catalogs/account-limit-profile-entries/
```

Every selected value is validated against the catalog schema before an
application can consume it.

## Resolve Block

Every variable must contain `[resolve]`:

```toml
[resolve]
default = "standard"

[[resolve.rule]]
when = 'qualifier["paid-account"]'
value = "expanded"
```

`default` is required. For primitive variables it is the default value itself.
For catalog-backed variables it names a value in the catalog.

Rules are evaluated in file order. Each rule references one qualifier and one
value. For primitive variables the rule contains the value directly. For
catalog-backed variables the rule names a catalog value. The first rule whose
qualifier resolves to `true` selects its value. If no rule matches, the default
value is selected.

## Invalid Shapes

These shapes are invalid:

```toml
# Missing type
schema_version = 1
```

```toml
# Schema-backed variables are no longer supported
schema = "value.schema.json"
```

```toml
# [values] is no longer supported
type = "string"

[values]
growth = "legacy"
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
