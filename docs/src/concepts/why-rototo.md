# Why rototo

Runtime configuration starts small.

A team needs one value to change without rebuilding or redeploying the
application: a token limit, a timeout, a model name, a feature toggle, a queue
name, a support message. The first version is usually a constant, an environment
variable, a flag rule, a JSON file, or a row in a database.

That works while the configuration is simple and changes rarely. It starts to
break when the value becomes a decision: different answers for different
environments, accounts, requests, or rollout states.

rototo exists for that point in the lifecycle. It gives runtime configuration a
single source-controlled place to be described, reviewed, validated, tested,
loaded by applications, and inspected when production behavior is surprising.

## How Runtime Configuration Usually Grows

Consider an LLM summarizer. The first configuration value might be:

```text
max_output_tokens = 2000
```

Then reality arrives:

- `dev` should use a smaller value so local iteration is faster.
- `stage` should be close enough to production for realistic testing.
- `prod` should allow longer output for real users.
- Enterprise accounts may need a different model, prompt, or limit.
- The application expects the returned config to have a specific shape.
- Operators need to know which config a request received and why.

None of those requirements are unusual. They are what happens when a runtime
decision becomes important.

Teams often respond by adding more places where configuration can live:

- constants in application code
- environment variables in deploy manifests
- feature flag rules in a hosted service
- config rows in a database with its own CRUD-like admin service
- JSON or YAML files loaded at startup
- operational overrides documented in a runbook

Each tool can be reasonable on its own. The problem is not that environment
variables, feature flags, config tables, or JSON files are bad. The problem is
that one production decision is now split across several places.

To understand what the application will do, a human or agent has to reconstruct
the answer from code, deployment state, dashboards, database rows, and tribal
knowledge.

## Where This Breaks Down

The failure mode is loss of control over a runtime decision.

Once the decision is scattered, teams can still change values, but they lose the
two controls they rely on for production code: validation before release and
reviewable change history. That is where runtime configuration becomes risky.
The common failure modes are:

- Late validation: a bad model name, malformed JSON object, unknown
  environment, or rule that references a missing value can break production even
  if the application binary did not change.
- Thin review history: important configuration changes lack the same review
  discipline as code changes: diffs, comments, automated checks, release
  history, and rollback.
- Unclear contract: application code asks for loosely named keys that may be
  missing, misspelled, deprecated, or shaped differently from what the code
  expects.
- Scattered inputs: environment variables, flag rules, account attributes,
  request attributes, and operational overrides all influence the final value,
  but no single place shows the whole logic.
- Weak explainability: when a request receives unexpected behavior, operators
  cannot easily see the selected value, the rule that selected it, the runtime
  context, and the configuration version that made the decision.

The point is not that every config value needs a heavy process. The point is
that once a runtime configuration decision affects production behavior, the team
needs a place where that decision can be reviewed and explained as a whole.

## What rototo Changes

rototo addresses those failure modes by putting the runtime decision in one
workspace that the CLI, CI, agents, and application SDK can all load.

- Late validation becomes workspace validation. Lint, type checks, resource
  schema checks, unknown environment checks, missing value checks, and custom
  lint can run before an application depends on the workspace.
- Thin review history becomes a source-controlled workspace. Configuration
  changes can have diffs, code review, automated checks, release history, and
  rollback.
- Unclear contract becomes a variable contract. Application code asks for a
  named variable, and the variable declares a primitive type or a resource type.
- Scattered inputs become explicit resolution inputs. Environment and runtime
  context are passed into resolution instead of being hidden across deployment
  state, dashboards, and ad hoc overrides.
- Weak explainability becomes a selected value name and value. rototo can report
  which branch was selected, such as `large`, and what value it returned, such
  as `2000`.

## The rototo Model

rototo uses a small model to make those changes concrete.

A workspace is the outer boundary. It is a directory tree of configuration files
that can be reviewed, linted, tested, and loaded by applications.

Inside the workspace, variables define what application code can ask for. A
variable such as `max-output-tokens` or `llm-agent-config` is the stable
application-facing name. It also declares whether the value is primitive or
selected from a resource.

Variables are resolved in an environment. The environment is the deployment or
runtime lane, such as `dev`, `stage`, or `prod`. That gives the same variable
different behavior in different lanes without changing application code.

Resolution can also use runtime context. Context is the JSON facts the
application provides for a request, account, tenant, user, or operation. Context
lets configuration depend on real runtime information without hiding those
inputs in unrelated systems.

Qualifiers give names to conditions over that context. A qualifier such as
`enterprise-accounts` can represent "the account is enterprise and has enough
seats." Variables can use qualifiers to select different values.

The selected branch inside a variable has a name. Names such as `standard` or
`large` explain which branch produced the returned value. In CLI and JSON
output, this selected branch name appears as `value_key`.

The entities relate like this:

```text
workspace
  |
  +-- environments
  |     +-- dev
  |     +-- stage
  |     +-- prod
  |
  +-- variables
  |     +-- max-output-tokens
  |           +-- values: small, standard, large
  |           +-- env mapping: dev -> small, prod -> large
  |           +-- rules can select values by qualifier
  |
  +-- qualifiers
  |     +-- enterprise-accounts
  |           +-- predicates over runtime context
  |
  +-- schemas
        +-- context schema
        +-- variable value schemas

runtime request
  |
  +-- variable id: max-output-tokens
  +-- environment: prod
  +-- context: account plan, account seats, request country
```

Those two sides meet during resolution. The application does not read workspace
files directly and it does not reimplement the rules. It sends a variable id,
environment, and context to rototo. rototo uses the workspace to validate the
inputs, evaluate any matching qualifiers, choose a value branch, validate the
selected value, and return the value to the application.

```text
workspace + variable id + environment + context
  -> validate inputs
  -> evaluate qualifiers
  -> select value branch
  -> validate selected value
  -> return value
```

This is why rototo is not just a lookup table.

A lookup asks:

```text
What is the value for max-output-tokens?
```

That question assumes there is one answer. Runtime configuration often has more
than one valid answer, depending on the environment and request context.

Resolution asks:

```text
Given this workspace, environment, and request context,
which reviewed branch applies, and what value should the application receive?
```

In the running example, resolution can return both the selected branch name and
the value:

```text
resolve max-output-tokens for prod given this request context
  -> selected branch: large
  -> value: 2000
```

The selected branch, `large`, tells humans and tools why this path was taken.
The value, `2000`, is what application code uses.

The same model is available through the CLI and the SDK. That means local
debugging, automated checks, agents, and application runtime all use the same
source of truth.

## Runtime Architecture

rototo configuration does not have to be fixed at application deployment time.
The usual production shape is to deploy the configuration workspace separately
from the application binary.

The application is deployed with a workspace source, such as a Git URI. At
startup, the SDK loads that workspace. While the application is running, it can
periodically refresh the workspace from the same source and keep serving the
last successfully loaded version.

```text
configuration repo
  |
  |  pre-push validation, CI, review, merge
  v
Git branch or tag
  |
  |  workspace URI
  |  git+https://github.com/acme/runtime-config.git#prod:config
  v
application deployment
  |
  |  app binary + workspace URI
  v
running application instances
  |
  +-- SDK loads workspace at startup
  +-- SDK refreshes workspace periodically
  +-- request handling resolves variables from latest good workspace
  +-- failed refresh keeps last good workspace active
```

That separation is important. Application deployments decide which workspace
source to trust. Configuration releases update what that source contains. The
application can pick up reviewed configuration changes without rebuilding or
redeploying the application, while still keeping a controlled source, validation
checks, and rollback through Git.

## How the Model Helps

The model is useful because it turns runtime configuration back into something
that can be checked before release.

Validation starts at the workspace boundary. The CLI can lint the manifest,
check variable types, validate schemas, reject unknown environments, and catch
rules that point at missing values. A context schema can reject malformed
runtime context before rules are evaluated, and predicate evaluation fails when
a qualifier reads a missing context field. Custom lint can enforce local policy,
such as allowed model families or token budgets. That moves many configuration
failures from production runtime to review or CI.

Because the workspace is source-controlled, configuration changes also get a
review history. A change to a variable, qualifier, schema, or environment mapping
can be reviewed as a diff, tested before merge, released intentionally, and
rolled back like source code.

Variables give application code a contract. Instead of reaching into a bag of
loosely named keys, the application resolves a stable variable id. The variable
declares what kind of value it can return, so the workspace and the application
can agree on shape before the value is used.

Values can stay inline for small decisions, or move into per-value TOML files
when they are large structured objects. In both cases, resolution sees the same
model: a variable selects a value key, and the selected key points at the value
the application receives.

Environment and context make the inputs explicit. The answer may depend on
`prod`, account plan, request country, tenant tier, or rollout state, but those
inputs are passed into resolution directly. They are no longer hidden across
deployment state, dashboards, database rows, and ad hoc overrides.

Qualifiers and selected branch names make the result explainable. A qualifier
names the condition that matched, and the branch name identifies the selected
path. That gives tests, logs, diagnostics, and agents something better to report
than a raw value: they can explain why the value was returned.

## When rototo Fits

rototo is a good fit when runtime configuration has enough meaning that it
deserves source control and validation.

Use it for:

- environment-specific runtime values
- structured config returned to application code
- account, tenant, user, or request targeting
- LLM model, prompt, gateway, and token settings
- runtime behavior gates that need review and diagnostics
- operational messages or routing decisions
- configuration that agents should be able to inspect and edit

rototo is especially useful when you want local CLI checks, CI validation, and
application runtime to share the same model.

## When rototo Is Not the Right Tool

rototo is not meant to own every kind of runtime state.

Do not use it as:

- a secret store
- a high-frequency counter or metrics database
- a user data store
- an experimentation analytics system
- a replacement for authorization policy engines
- a place for business records that change per transaction

rototo controls reviewed configuration decisions. It should not become the
database for operational facts.

## What to Read Next

Read `quickstart` if you want the shortest working example: create a local
workspace, define one variable, lint it, and resolve it.

Read `production-workflow` if you want to see the same model with Git, context
schemas, qualifiers, tests, application loading, refresh, custom lint, and
observability.

Read the reference pages when you need exact CLI, SDK, file format, or
diagnostic behavior.
