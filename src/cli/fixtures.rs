#![allow(clippy::wildcard_imports)]

use crate::*;

pub(crate) async fn run_fixtures(
    args: FixturesArgs,
    source_options: &SourceOptions,
    json: bool,
    quiet: bool,
) -> Result<ExitCode> {
    let selection = fixture_generate_selection(&args);
    let package = package_source_string_or_current(args.package).await?;
    let invocations =
        rototo::fixtures::generate_resolve_invocations(&package, source_options, selection).await?;
    print_fixture_invocations(
        &package,
        &invocations,
        args.context_form.to_render(),
        json,
        quiet,
    )?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn fixture_generate_selection(
    args: &FixturesArgs,
) -> rototo::fixtures::FixtureGenerateSelection {
    // With no selector flags at all, generate fixtures for the whole package, the
    // same way `lint`, `show`, and `inspect` treat an empty selector set. Without
    // this default, bare `rototo fixtures` would select nothing and print nothing.
    let no_selectors = !args.all_variables && args.variables.is_empty();
    if no_selectors {
        return rototo::fixtures::FixtureGenerateSelection {
            variables: rototo::fixtures::FixtureTargetSelection::All,
        };
    }
    rototo::fixtures::FixtureGenerateSelection {
        variables: fixture_target_selection(args.all_variables, &args.variables),
    }
}

pub(crate) fn fixture_target_selection(
    all: bool,
    values: &[String],
) -> rototo::fixtures::FixtureTargetSelection {
    if all {
        rototo::fixtures::FixtureTargetSelection::All
    } else if values.is_empty() {
        rototo::fixtures::FixtureTargetSelection::None
    } else {
        rototo::fixtures::FixtureTargetSelection::some(values.iter().cloned())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FixtureInvocationJson<'a> {
    target: String,
    case_id: &'a str,
    title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    because: Option<&'a str>,
    command: String,
    context: &'a serde_json::Value,
    expect: &'a rototo::fixtures::ResolveExpectation,
}

pub(crate) fn print_fixture_invocations(
    package: &str,
    invocations: &[rototo::fixtures::ResolveInvocation],
    form: rototo::fixtures::ContextForm,
    json: bool,
    quiet: bool,
) -> Result<()> {
    if json {
        let items = invocations
            .iter()
            .map(|invocation| FixtureInvocationJson {
                target: invocation.target.label(),
                case_id: &invocation.case_id,
                title: &invocation.title,
                because: invocation.because.as_deref(),
                command: rototo::fixtures::render_command(package, invocation, form),
                context: &invocation.context,
                expect: &invocation.expect,
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&items)
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    let mut current_target: Option<String> = None;
    for invocation in invocations {
        let command = rototo::fixtures::render_command(package, invocation, form);
        if quiet {
            println!("{command}");
            continue;
        }
        let label = invocation.target.label();
        if current_target.as_deref() != Some(label.as_str()) {
            if current_target.is_some() {
                println!();
            }
            println!("{}", style::label(&label));
            current_target = Some(label);
        }
        println!(
            "{command}  {}",
            style::dim(&rototo::fixtures::render_comment(invocation))
        );
    }
    Ok(())
}
