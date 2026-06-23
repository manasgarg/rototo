pub(in crate::lint) const CATALOG_SCHEMA_URI_PREFIX: &str = "rototo://catalogs/";

pub(in crate::lint) fn catalog_schema_uri(id: &str) -> String {
    format!("{CATALOG_SCHEMA_URI_PREFIX}{id}.schema.json")
}
