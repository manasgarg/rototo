use std::fs;
use std::path::Path;

use toml::Value;

#[test]
fn sdk_package_versions_match_root_package() {
    let root_version = env!("CARGO_PKG_VERSION");

    assert_eq!(
        manifest_version("sdks/python/Cargo.toml", &["package", "version"]),
        root_version,
        "sdks/python/Cargo.toml should use the canonical rototo version"
    );
    assert_eq!(
        manifest_version("sdks/python/pyproject.toml", &["project", "version"]),
        root_version,
        "sdks/python/pyproject.toml should use the canonical rototo version"
    );
}

fn manifest_version(path: &str, keys: &[&str]) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(root.join(path)).unwrap();
    let value = text.parse::<Value>().unwrap();
    let mut current = &value;
    for key in keys {
        current = current
            .get(*key)
            .unwrap_or_else(|| panic!("{path} is missing `{}`", keys.join(".")));
    }
    current
        .as_str()
        .unwrap_or_else(|| panic!("{path} `{}` must be a string", keys.join(".")))
        .to_owned()
}
