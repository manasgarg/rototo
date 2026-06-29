use std::path::Path;

use regex::Regex;

use crate::error::{Result, RototoError};

use super::{DEFAULT_DOCS_BASE_URL, DOCS, get_page, page_href};

pub fn render_package_readme(sdk: &str) -> Result<String> {
    render_package_readme_with_base_url(sdk, DEFAULT_DOCS_BASE_URL)
}

pub fn render_package_readme_with_base_url(sdk: &str, docs_base_url: &str) -> Result<String> {
    let docs_base_url = normalize_docs_base_url(docs_base_url)?;
    let page = match sdk {
        "python" => get_page("reference-sdk-python")?,
        "typescript" => get_page("reference-sdk-typescript")?,
        "java" => get_page("reference-sdk-java")?,
        "go" => get_page("reference-sdk-go")?,
        other => {
            return Err(RototoError::new(format!(
                "unsupported package README SDK: {other}"
            )));
        }
    };
    let mut markdown = page.markdown.to_owned();
    let readme_title = match sdk {
        "python" => "rototo Python SDK",
        "typescript" => "rototo TypeScript SDK",
        "java" => "rototo Java SDK",
        "go" => "rototo Go SDK",
        _ => unreachable!("unsupported SDK was rejected above"),
    };
    if let Some(rest) = markdown.strip_prefix(&format!("# {} Reference\n", page.title)) {
        markdown = format!("# {readme_title}\n{rest}");
    } else if let Some(rest) = markdown.strip_prefix(&format!("# {}\n", page.title)) {
        markdown = format!("# {readme_title}\n{rest}");
    }
    markdown = rewrite_package_readme_doc_links(&markdown, docs_base_url);
    Ok(format!(
        "<!-- Generated from docs/src/{page_id}.md by `rototo docs --package-readme {sdk} --out sdks/{sdk}/README.md`. Do not edit directly. -->\n\n{markdown}",
        page_id = page.id,
    ))
}

fn normalize_docs_base_url(docs_base_url: &str) -> Result<&str> {
    let docs_base_url = docs_base_url.trim().trim_end_matches('/');
    if docs_base_url.is_empty() {
        return Err(RototoError::new("docs base URL must not be blank"));
    }
    Ok(docs_base_url)
}

fn rewrite_package_readme_doc_links(markdown: &str, docs_base_url: &str) -> String {
    let link =
        Regex::new(r"\[([^\]\n]+)\]\(([^)\s]+)\)").expect("documentation link regex is valid");
    link.replace_all(markdown, |captures: &regex::Captures<'_>| {
        let text = captures.get(1).expect("capture exists").as_str();
        let target = captures.get(2).expect("capture exists").as_str();
        if let Some((page_id, anchor)) = internal_doc_link_target(target) {
            format!(
                "[{text}]({docs_base_url}/{}{})",
                page_href(page_id),
                anchor.unwrap_or("")
            )
        } else {
            captures.get(0).expect("capture exists").as_str().to_owned()
        }
    })
    .into_owned()
}

fn internal_doc_link_target(target: &str) -> Option<(&'static str, Option<&str>)> {
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with('#')
    {
        return None;
    }
    let (target, anchor) = match target.find('#') {
        Some(index) => (&target[..index], Some(&target[index..])),
        None => (target, None),
    };
    let file_name = Path::new(target).file_name()?.to_str()?;
    let id = file_name
        .strip_suffix(".md")
        .or_else(|| file_name.strip_suffix(".html"))?;
    DOCS.iter()
        .find(|page| page.id == id)
        .map(|page| (page.id, anchor))
}
