use crate::address::Address;

/// What a registered custom lint rule runs against: an address from the
/// package addressing grammar (`src/address.rs`), depth-checked at
/// registration time by `parse_registered_lint_selector`.
#[derive(Clone)]
pub(in crate::lint) struct RegisteredLintSelector {
    pub(in crate::lint) address: Address,
}
