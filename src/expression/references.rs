use super::*;

pub(super) fn references_from_cel(expr: &IdedExpr) -> ExpressionReferences {
    let mut references = ExpressionReferences::default();
    collect_cel(expr, &mut references, &mut Vec::new());
    references
}

/// One pass over the cel AST: record references (context/entry paths and
/// variable ids) and, per context path, the scalar family the surrounding
/// operator or function requires of it. `bound` carries the identifiers a
/// surrounding comprehension binds (macros such as `exists(a, ...)` expand to
/// comprehensions), so chains rooted at them are not misread as unknown roots.
pub(super) fn collect_cel(
    expr: &IdedExpr,
    references: &mut ExpressionReferences,
    bound: &mut Vec<String>,
) {
    cel_constraints(expr, &mut references.context_path_types);
    cel_list_memberships(expr, &mut references.context_path_lists);

    if let Some((root, _)) = cel_path(expr)
        && bound.contains(&root)
    {
        return;
    }

    match cel_reference(expr) {
        Some(Reference::Context(path)) => {
            references.context_paths.insert(path);
            return;
        }
        Some(Reference::Entry(path)) => {
            references.entry_paths.insert(path);
            return;
        }
        Some(Reference::Variable(id)) => {
            references.variables.insert(id);
            return;
        }
        Some(Reference::List(id)) => {
            references.lists.insert(id);
            return;
        }
        Some(Reference::EnvNow) => {
            return;
        }
        Some(Reference::ResolvingVariable) => {
            references.uses_resolving = true;
            return;
        }
        None => {
            if let Some(issue) = cel_root_issue(expr) {
                references.invalid_roots.insert(issue);
                return;
            }
        }
    }

    if let Expr::Comprehension(comprehension) = &expr.expr {
        collect_cel(&comprehension.iter_range, references, bound);
        collect_cel(&comprehension.accu_init, references, bound);
        let outer = bound.len();
        bound.push(comprehension.iter_var.clone());
        bound.extend(comprehension.iter_var2.clone());
        bound.push(comprehension.accu_var.clone());
        collect_cel(&comprehension.loop_cond, references, bound);
        collect_cel(&comprehension.loop_step, references, bound);
        collect_cel(&comprehension.result, references, bound);
        bound.truncate(outer);
        return;
    }

    for child in cel_children(expr) {
        collect_cel(child, references, bound);
    }
}

pub(super) fn cel_children(expr: &IdedExpr) -> Vec<&IdedExpr> {
    match &expr.expr {
        Expr::Call(call) => call
            .target
            .as_deref()
            .into_iter()
            .chain(call.args.iter())
            .collect(),
        Expr::Comprehension(comprehension) => vec![
            &comprehension.iter_range,
            &comprehension.accu_init,
            &comprehension.loop_cond,
            &comprehension.loop_step,
            &comprehension.result,
        ],
        Expr::List(list) => list.elements.iter().collect(),
        Expr::Map(map) => map
            .entries
            .iter()
            .filter_map(|entry| match &entry.expr {
                EntryExpr::MapEntry(entry) => Some([&entry.key, &entry.value]),
                EntryExpr::StructField(_) => None,
            })
            .flatten()
            .collect(),
        Expr::Select(select) => vec![&select.operand],
        Expr::Struct(structure) => structure
            .entries
            .iter()
            .filter_map(|entry| match &entry.expr {
                EntryExpr::StructField(field) => Some(&field.value),
                EntryExpr::MapEntry(_) => None,
            })
            .collect(),
        Expr::Ident(_) | Expr::Literal(_) | Expr::Unspecified => Vec::new(),
    }
}

pub(super) enum Reference {
    Context(String),
    Entry(String),
    /// `variables.<id>` / `variables["<id>"]`; extra trailing segments select
    /// into the referenced variable's resolved value.
    Variable(String),
    /// `lists.<id>` / `lists["<id>"]`; binds the list's member list, so
    /// membership tests read `context.path in lists.<id>`.
    List(String),
    EnvNow,
    ResolvingVariable,
}

pub(super) fn cel_reference(expr: &IdedExpr) -> Option<Reference> {
    let (root, segments) = cel_path(expr)?;
    if segments.is_empty() {
        return None;
    }
    match root.as_str() {
        "context" => Some(Reference::Context(segments.join("."))),
        "entry" => Some(Reference::Entry(segments.join("."))),
        "variables" => Some(Reference::Variable(segments[0].clone())),
        "lists" => Some(Reference::List(segments[0].clone())),
        "env" => match segments.as_slice() {
            [member] if member == "now" => Some(Reference::EnvNow),
            [first, second] if first == "resolving" && second == "variable" => {
                Some(Reference::ResolvingVariable)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Record `context.<path> in lists.<id>` memberships. The parse layer cannot
/// know the list's member type; lint later refines the context path's expected
/// scalar family from the declared list, so the membership itself is what gets
/// recorded here.
pub(super) fn cel_list_memberships(
    expr: &IdedExpr,
    memberships: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let Expr::Call(call) = &expr.expr else {
        return;
    };
    if call.func_name != operators::IN || call.args.len() != 2 {
        return;
    }
    if let Some(path) = cel_context_path(&call.args[0])
        && let Some(Reference::List(id)) = cel_reference(&call.args[1])
    {
        memberships.entry(path).or_default().insert(id);
    }
}

/// Classify a root chain that [`cel_reference`] did not recognize. Only chains
/// with at least one segment are considered, so bare identifiers (such as
/// comprehension variables) are never flagged.
pub(super) fn cel_root_issue(expr: &IdedExpr) -> Option<ExpressionRootIssue> {
    let (root, segments) = cel_path(expr)?;
    if segments.is_empty() {
        return None;
    }
    match root.as_str() {
        "context" | "entry" | "variables" | "lists" => None,
        "env" => match segments.as_slice() {
            [member] if member == "now" => None,
            [first, _] if first == "qualifier" => Some(ExpressionRootIssue::LegacyQualifier),
            [first, second] if first == "resolving" && second == "variable" => None,
            _ => Some(ExpressionRootIssue::UnknownEnvMember(segments.join("."))),
        },
        "qualifier" => Some(ExpressionRootIssue::LegacyQualifier),
        other => Some(ExpressionRootIssue::UnknownRoot(other.to_owned())),
    }
}

/// Unwrap a `root.a.b` / `root["a"]["b"]` chain into its root identifier and
/// dotted segments. Returns `None` for anything that is not such a chain.
pub(super) fn cel_path(expr: &IdedExpr) -> Option<(String, Vec<String>)> {
    match &expr.expr {
        Expr::Ident(name) => Some((name.clone(), Vec::new())),
        Expr::Select(select) => {
            let (root, mut segments) = cel_path(&select.operand)?;
            segments.push(select.field.clone());
            Some((root, segments))
        }
        Expr::Call(call)
            if call.func_name == operators::INDEX
                && call.target.is_none()
                && call.args.len() == 2 =>
        {
            let Expr::Literal(LiteralValue::String(key)) = &call.args[1].expr else {
                return None;
            };
            let (root, mut segments) = cel_path(&call.args[0])?;
            segments.push(key.to_string());
            Some((root, segments))
        }
        _ => None,
    }
}
