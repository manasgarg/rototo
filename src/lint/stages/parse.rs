use super::super::engine::LintContext;
use super::super::syntax::parse_sources;

pub(super) fn run(ctx: &mut LintContext) {
    ctx.syntax = parse_sources(&ctx.source, &mut ctx.diagnostics);
}
