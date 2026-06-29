# Introducing Rototo

> Start a thousand engineering teams on a thousand products and each one of them will reinvent a configuration system.

## What configuration really is

It's tempting to think of configuration as a bag of settings - the stuff that goes in env vars: database URLs, port numbers, secrets. And some of it is exactly that. But those are the values that never change after you deploy, and they're the minority. Most configuration is there to steer a system that's already running.

Here's the split that matters. Code says what your system *can* do. Configuration says what it should do *right now*. We keep the two apart on purpose, because they change at different speeds and by different people. Engineers change code when the system needs a new capability. People change configuration when their intent changes - and intent changes constantly, usually for reasons that have nothing to do with engineering: a product launch, a one-off customer contract, a model getting deprecated, an incident at 2am. The whole point of configuration is to steer the system without a rebuild and a redeploy.

So software gets built, deployed, and steered. All three matter. But configuration - the one knob you have for steering a live system - almost never gets the care the other two do. That's what we want to fix. Terraform gave deployment a control plane; Rototo is the control plane for steering.

## What's wrong with how we do configuration today

Three things, really:

- Everyone builds their own, over and over, for what is actually one general problem.
- The systems we build forget that configuration is two things at once: data *and* code.
- They were all designed assuming a human makes the change - and that assumption is falling apart now that agents are in the loop.

Let's take them one at a time.

### Everyone rebuilds the same thing

Engineers resist building a configuration system, and honestly that's a good instinct - they're hard to build. So at first they dodge it: put the behavioral setting somewhere that needs a redeploy to change, and move on.

That holds until it doesn't. Once you accept you need a real configuration system, you build it one painful piece at a time, only ever adding the next thing you can't live without. First you pull the config out of the code. Then you need somewhere to keep it, a way to change it, a way to reload it in the running app. Then someone has to review changes, which means you need diffs. Then permissions: who's allowed to change what. Then an audit trail. Then provenance. Then you realize every variation needs to be tested and bounded by limits. Then your users want a release cycle. Then you have to handle schema upgrades while old config is still live in production.

And then you do the whole thing *again* for pricing. And again for content selection, LLM routing, prompt selection, offer eligibility, operational policy, localization, tenant customization. Every serious application carries a heavy configuration load that touches nearly everything it does.

Feature-flag systems are the one general solution we have, but they only cover a narrow slice. The rest of the configuration problem still gets reinvented from scratch.

### Configuration is both data and code

Most of the time, configuration looks like data. Even targeting rules - the one part that's really logic - can be squeezed into a data shape. So it feels natural to build configuration the way you'd build any data feature: a database, some CRUD APIs, a UI on top.

The trouble is that the data-shaped view cuts configuration off from everything that makes code trustworthy. Configuration needs to be diffed, tested, reviewed, promoted across environments, checked against what the app actually expects, refactored, rolled back, run through CI. And at the same time it has to stay usable by people who aren't developers, ship on its own schedule, and drop into a running system without a restart.

Put plainly: configuration should inherit the rigor of code without inheriting code's operational constraints.

### Then agents showed up

Every configuration system built so far quietly assumes one thing: a human makes the change, at human pace, over a human-sized chunk of the system.

That assumption is breaking on two fronts it was never tested against - speed and reach.

- **Speed.** A person changes a handful of values a day. An agent can change thousands, continuously, reacting to live signals. Most systems lean on manual review as the safety net, and manual review simply can't keep up with that.
- **Reach.** A person couldn't operate the whole surface even if they wanted to. With no contract and no explanation built into the system, changing a value safely meant *being the expert*. So people stuck to their slice - not by choice, but because that's as far as one expert's knowledge reached. An agent has no such limit. It reasons across the entire surface at once, including the cross-domain interactions people avoided precisely because they never looked at two of them together.

Every weakness of the data-shaped view used to be survivable, because a person quietly made up for it. The human read the diff the system couldn't render. The human knew the contract the schema didn't capture. The human remembered why a value was set the way it was. The human caught the bad change before it shipped. The data view didn't work because it was sound - it worked because someone was standing in the gap.

So agents don't add a new requirement. They remove the person who was silently meeting the old one. The rigor has to live in the system now, because the agent won't bring it for you.

## Rototo's approach

Rototo's core premise is that behavioral configuration should live as files in a git repository. It should be authored, reviewed, tested, versioned, packaged and released just like code.

To make that practical - and to stop everyone from reinventing the engine - Rototo standardizes a package format for configuration. It's a common structure any domain can pick up without rebuilding the machinery around it. The format hands you a small set of building blocks that compose into *your* configuration model; it doesn't force one on you. And on top of those blocks it layers validation: schema, semantics, custom lint rules, and checks against the app's own contract - each catching a different kind of mistake before release.

Those three capabilities line up with the three problems we started with:

- **Reinvention** is the easy one. Standardize the format, ship reusable building blocks, and every domain gets the same control machinery to build on - pricing, routing, eligibility, LLM selection, each modeling its own behavior on the same foundation.
- **The dual nature** is handled by keeping code and data in one model. The package gives configuration the ergonomics of software: diffs, branches, pull requests, CI, reproducible commits. The runtime SDK gives it the ergonomics of data: config refreshes and applies independently of the code.
- **The agent problem** is handled by that layered validation. When the human who quietly supplied the rigor steps out of the gap, the contract has to live in the package itself. An agent can propose change at any speed and across any reach - but only inside a typed, linted, explainable boundary the system checks before anything reaches production.

## One last thought

Rototo isn't a feature-flag system with a bigger remit, and it isn't a fancier version of the CRUD APIs that have sprung up everywhere. It starts from taking behavioral configuration seriously - seriously enough to say it deserves its own control plane, the same way deployment got one. From there, it solves for configuration's split nature, code and data, aiming to generalize while building safety into how config gets resolved.
