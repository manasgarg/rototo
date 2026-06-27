# Introducing Rototo

> Start a thousand engineering teams on a thousand products and each one of them will reinvent a configuration system.

## The humble configuration

The natural inclination is to think of configuration as a bag of settings. This is the stuff that goes into env vars: db URLs, port numbers, secrets, and so on. But that's only partially true. It holds for the values that don't change after deployment, and those are the minority. The vast majority of configuration exists to steer the behavior of a system that's already running.

If code expresses what a system can do, configuration expresses what it should do right now. The two are kept deliberately apart so they can change at different speeds and by different people. Engineers change code when the requirements on capability change. Users change configuration when their intent changes — and intent changes far more often, usually for reasons that have nothing to do with engineering: product launches, customer-specific contracts, model deprecations, production incidents, and more. The whole point of configuration is to steer the system without forcing a rebuild and redeploy.

Software is built, deployed, and steered. Excellence in all three is critical to its success. Yet configuration, the only mechanism for steering a running systemm, rarely gets the attention it deserves.

Rototo's goal is to change that. Just as Terraform is the control plane for deploying software, Rototo is the control plane for steering it.

## What's wrong with configuration systems today?

Configuration systems today share three key problems:
- They're all bespoke implementations of what is really a generalized problem.
- They're built in a way that fails to recognize configuration's dual nature: it is both data and code.
- They were designed for humans to make changes — but that assumption is breaking down as agents operate at far higher velocity and reason across a far wider span.

### Bespoke implementations of a generalized configuration problem

Engineers tend to resist building a configuration system. They'll put behavioral configuration in settings that require a redeploy to change. It's a sound first instinct, because configuration systems are hard to build.

Once they get past that resistance and accept that a configuration system is inevitable, they build it one piece at a time, and only as much as they must. First they externalize the configuration. Then they need somewhere to store it, a way to update it, a way to reload it in the running application. Then it has to be reviewed, which means it has to be diffed. Then come permissions: who can change what. Then audit trails, then provenance. Then they realize every configuration variation has to be tested and bounded by limits. Then users need a release cycle. Then schema upgrades have to be handled while configurations are already deployed in production.

Then they do the whole thing again for pricing, content selection, LLM routing, prompt selection, offer eligibility, operational policy, localization, tenant customization, and every other operational control. In fact, every enterprise application carries a heavy configuration component that touches the entirety of its functionality.

Feature-flagging systems offer a generalized solution, but only for a narrow class of problems. The vast majority of configuration problems still need their own solution.

### Failure to recognize dual nature of configuration: data and code

Configuration mostly looks like data. Even the targeting rules, the one imperative component, can be coerced into the shape of data. So it's natural that engineering teams build configuration around the architectural patterns they use for data: most commonly, a database with CRUD APIs and a UI on top.

But the data-centric view severs configuration from the engineering rigor that goes into code. Configuration needs to be diffed, tested, reviewed, promoted through environments, validated against application contracts, refactored, rolled back, and run through CI. At the same time, it has to stay accessible to users and not just developers, ship independently of the application, and hot-swap into a running system.

In other words, configuration must inherit the rigor of code without inheriting its operational constraints.

### Situation changes with Agents

Every configuration system built today rests on an unstated assumption: that a human makes the change, working at human pace, over a human-sized slice of the system.

That assumption is now breaking, along two axes humans never stressed: velocity and span.
- Velocity: a human changes a handful of values a day. An agent can change thousands, continuously, in response to live signals. Most  configuration systems rely on manual review as a safety mechanism but it does not match the agent speed.
- Span: a human couldn't operate the whole surface even if they wanted to. With no contract or explanation in the system, changing a value safely meant being the expert. Operators were confined to slices not by choice but by the reach of one expert — and the fragmentation of configuration across an org is really the fragmentation of the expertise needed to operate it. An agent has no such ceiling: it reasons across the entire surface at once, including the cross-domain interactions humans avoided by never looking at two together.

Every weakness of the data-centric view was survivable because a person silently compensated for it. The human read the diff the system couldn't render. The human knew the contract the schema didn't encode. The human remembered why a value was set. The human caught GPT-6 before it reached production. The data-view didn't work because it was correct; it worked because someone stood in the gap.

So agents don't introduce a new requirement. They remove the human who was quietly meeting the old one. The rigor has to live in the system rather than human judgement, because the agent will not supply it.

## Rototo's approach

Rototo's core premise is that behavioral configuration should live as files in a git repository.
It should be authored, reviewed, tested, versioned, packaged and released just like code.

To make it practical and avoid endless reinvention of configuration engines, Rototo standardizes a package format for configuration; a common structure that any domain can adopt without rebuilding the machinery around it.
Rototo's package format gives every domain the same building blocks — primitives that compose into its own configuration model rather than dictating one. On top of those primitives, it offers multi-layer validation: schema, semantics, custom lint rules, and application-contract enforcement, each catching a different class of error before release.

These three capabilities answer the three problems we started with.
- Bespoke reinvention is the easy one: by standardizing the package format and shipping reusable primitives, Rototo gives every domain the same control machinery to build on. Be it pricing, routing, eligibility or LLM selection, each models its own behavior on the common foundation.
- The dual nature is answered by keeping both code and data in one model. The package gives configuration the ergonomics of software such as diffs, branches, pull requests, CI, reproducible commits. On the other hand, the runtime SDK gives it the ergonomics of data: configuration is refreshed and applied independent of the code.
- And the agent problem is answered by multi-layer validation. When the human who silently supplied the rigor steps out of the gap, the contract has to live in the package itself. An agent can propose change at any velocity and across any span, but it can only propose inside a typed, linted, explainable boundary that the system checks before anything reaches production.

## Closing thoughts

Rototo is not another feature-flag system with a wider remit, nor is it a generalization of the CRUD APIs that have proliferated everywhere. It is first and foremost a recognition of how much behavioral configuration matters: that it needs its own control plane, just as deployment has one. From that starting point, Rototo solves for the dual nature of configuration, code and data, aiming for generalization while layering safety into config resolution.
