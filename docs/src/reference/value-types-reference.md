# Value Types Reference

Variable values are TOML values that rototo validates and returns as JSON-like
values during resolution.

Variables declare either a primitive `type` or a JSON `schema`. They must not
declare both.

## Primitive Types

Primitive type variables use:

```toml
type = "int"
```

Supported primitive types:

```text
bool
int
number
string
list
```

## `bool`

TOML boolean.

```toml
[values]
enabled = true
disabled = false
```

## `int`

TOML integer.

```toml
[values]
small = 500
large = 2000
```

## `number`

TOML integer or floating point number.

```toml
[values]
low = 0.1
standard = 1
```

## `string`

TOML string.

```toml
[values]
control = "Welcome back."
premium = "Welcome back, premium member."
```

## `list`

TOML array.

```toml
[values]
default = ["card", "bank_transfer"]
```

The primitive `list` type validates that the value is an array. It does not
declare element types. Use a JSON Schema variable when element shape matters.

## Structured Values

Structured values should use `schema`, not primitive `type`.

```toml
schema = "../schemas/llm-config.schema.json"
```

Each configured value is validated against the referenced JSON Schema during
lint.

Inline object values:

```toml
[values.enterprise]
model = "gpt-5"
gateway = "openai"
max_output_tokens = 5000
temperature = 0.2
```

External object value:

```toml
[value]
model = "gpt-5"
gateway = "openai"
max_output_tokens = 5000
temperature = 0.2
```

## TOML to JSON

Resolution output is JSON. TOML values are converted to JSON-compatible values:

- TOML booleans become JSON booleans.
- TOML integers and floats become JSON numbers.
- TOML strings become JSON strings.
- TOML arrays become JSON arrays.
- TOML tables become JSON objects.

## Choosing `type` Or `schema`

Use primitive `type` for small scalar or list decisions.

Use `schema` when the value is an object, when a list needs element validation,
or when application code expects a precise structured contract.

## Validation

Value validation happens during lint:

- primitive variables check every value against `type`;
- schema-backed variables check every value against the JSON Schema;
- external value files are loaded before validation;
- registered custom value lint runs after values have been expanded.
