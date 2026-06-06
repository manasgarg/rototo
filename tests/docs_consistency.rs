use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use rototo::docs::{DOC_NAV_SECTIONS, DOCS};

#[test]
fn every_public_docs_source_is_bundled() {
    let registered = registered_page_ids();
    let docs_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/src");
    for path in markdown_files(&docs_dir) {
        let id = path.file_stem().unwrap().to_string_lossy().into_owned();
        assert!(
            registered.contains(&id),
            "{} exists but is not registered in rototo::docs::DOCS",
            path.display()
        );
    }
}

#[test]
fn docs_navigation_is_grouped_and_complete() {
    let titles = DOC_NAV_SECTIONS
        .iter()
        .map(|section| section.title)
        .collect::<Vec<_>>();
    assert_eq!(
        titles,
        vec!["Start", "Learn", "Reference", "Adopt"],
        "documentation navigation should match the active docs scaffold"
    );

    let registered = registered_page_ids();
    let registered_order = DOCS.iter().map(|page| page.id).collect::<Vec<_>>();
    let mut nav_order = Vec::new();
    let mut seen = BTreeSet::new();
    for section in DOC_NAV_SECTIONS {
        assert!(
            !section.pages.is_empty(),
            "navigation section `{}` should not be empty",
            section.title
        );
        for id in section.pages {
            assert!(
                registered.contains(*id),
                "navigation section `{}` lists unknown page `{id}`",
                section.title
            );
            assert!(
                seen.insert((*id).to_owned()),
                "navigation lists page `{id}` more than once"
            );
            nav_order.push(*id);
        }
    }
    assert_eq!(
        seen, registered,
        "navigation should list every bundled page exactly once"
    );
    assert_eq!(
        nav_order, registered_order,
        "DOCS should use the same order as the rendered navigation"
    );
}

#[test]
fn bundled_docs_do_not_contain_review_comments() {
    for page in DOCS {
        assert!(
            !page.markdown.contains("[Manas]") && !page.markdown.contains("Manas"),
            "bundled docs page `{}` contains a review comment marker",
            page.id
        );
    }
}

#[test]
fn bundled_docs_avoid_ambiguous_audience_terms() {
    for page in DOCS {
        let markdown = page.markdown.to_lowercase();
        for term in ["segment", "segmented", "segmentation", "cohort"] {
            assert!(
                !markdown.contains(term),
                "bundled docs page `{}` contains `{}`; prefer condition, account class, or bucket terminology",
                page.id,
                term
            );
        }
    }
}

fn markdown_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    collect_markdown_files(dir, &mut files);
    files
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("md") {
            files.push(path);
        }
    }
}

fn registered_page_ids() -> BTreeSet<String> {
    DOCS.iter().map(|page| page.id.to_owned()).collect()
}
