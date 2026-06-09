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
    assert_eq!(
        manifest_version("sdks/typescript/Cargo.toml", &["package", "version"]),
        root_version,
        "sdks/typescript/Cargo.toml should use the canonical rototo version"
    );
    assert_eq!(
        manifest_version("sdks/java/Cargo.toml", &["package", "version"]),
        root_version,
        "sdks/java/Cargo.toml should use the canonical rototo version"
    );
    assert_eq!(
        manifest_version("sdks/go/Cargo.toml", &["package", "version"]),
        root_version,
        "sdks/go/Cargo.toml should use the canonical rototo version"
    );
    assert_eq!(
        xml_project_version("sdks/java/pom.xml"),
        root_version,
        "sdks/java/pom.xml should use the canonical rototo version"
    );
    assert_eq!(
        json_manifest_version("sdks/typescript/package.json", &["version"]),
        root_version,
        "sdks/typescript/package.json should use the canonical rototo version"
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

fn json_manifest_version(path: &str, keys: &[&str]) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(root.join(path)).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
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

fn xml_project_version(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(root.join(path)).unwrap();
    let start = text
        .find("<version>")
        .unwrap_or_else(|| panic!("{path} is missing <version>"))
        + "<version>".len();
    let end = text[start..]
        .find("</version>")
        .unwrap_or_else(|| panic!("{path} is missing </version>"))
        + start;
    text[start..end].to_owned()
}
