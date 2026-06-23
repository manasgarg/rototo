#[derive(Clone)]
pub(in crate::lint) struct RegisteredLintSelector {
    pub(in crate::lint) address: RegisteredLintAddress,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::lint) enum RegisteredLintAddress {
    Workspace,
    Qualifiers,
    Qualifier {
        id: String,
    },
    Variables,
    Variable {
        id: String,
    },
    VariableValues {
        variable: String,
    },
    VariableValue {
        variable: String,
        key: String,
    },
    VariableRules {
        variable: String,
    },
    VariableRule {
        variable: String,
        index: usize,
    },
    Catalogs,
    Catalog {
        id: String,
    },
    CatalogEntries {
        catalog: String,
    },
    CatalogEntry {
        catalog: String,
        key: String,
    },
    RequestContexts,
    RequestContext {
        id: String,
    },
    RequestContextEntries {
        request_context: String,
    },
    RequestContextEntry {
        request_context: String,
        key: String,
    },
}
