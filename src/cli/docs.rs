use std::path::Path;
use std::process::ExitCode;

use regex::Regex;
use serde::Serialize;

use rototo::{Result, RototoError};

use crate::style;
use crate::{DocsArgs, PackageReadmeTarget};

pub(crate) async fn run_docs(args: DocsArgs, json: bool) -> Result<ExitCode> {
    let docs_base_url = args.docs_base_url;
    match (
        args.export,
        args.page,
        args.search,
        args.package_readme,
        args.out,
    ) {
        (Some(out), None, None, None, None) => {
            rototo::docs::export_html(&out).await?;
            print_docs_export(&out, json)?;
            Ok(ExitCode::SUCCESS)
        }
        (None, Some(page), None, None, None) => print_docs_page(&page, json),
        (None, None, Some(search), None, None) => print_docs_search(&search, json),
        (None, None, None, Some(target), Some(out)) => {
            let docs_base_url = docs_base_url
                .as_deref()
                .unwrap_or(rototo::docs::DEFAULT_DOCS_BASE_URL);
            write_package_readme(target, &out, docs_base_url).await?;
            print_package_readme_export(target, &out, json)?;
            Ok(ExitCode::SUCCESS)
        }
        (None, None, None, None, None) => {
            print_docs_index(json)?;
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(RototoError::new(
            "--export, --page, --search, and --package-readme cannot be used together",
        )),
    }
}

#[derive(Serialize)]
struct DocsIndexJson {
    sections: Vec<DocsSectionJson>,
}

#[derive(Serialize)]
struct DocsSectionJson {
    title: &'static str,
    pages: Vec<DocsPageSummaryJson>,
}

#[derive(Serialize)]
struct DocsPageSummaryJson {
    id: &'static str,
    title: &'static str,
}

#[derive(Serialize)]
struct DocsPageJson {
    id: &'static str,
    title: &'static str,
    markdown: String,
}

#[derive(Serialize)]
struct DocsSearchJson {
    query: String,
    matches: Vec<DocsSearchMatch>,
}

#[derive(Serialize)]
struct DocsExportJson {
    out: String,
}

#[derive(Serialize)]
struct PackageReadmeExportJson {
    sdk: &'static str,
    out: String,
}

#[derive(Serialize)]
struct DocsSearchMatch {
    page: &'static str,
    title: &'static str,
    line: usize,
    text: String,
    spans: Vec<DocsSearchSpan>,
}

#[derive(Serialize)]
struct DocsSearchSpan {
    start: usize,
    end: usize,
}

enum PagePrefixMatch {
    One(&'static rototo::docs::DocPage),
    Ambiguous(Vec<&'static rototo::docs::DocPage>),
    None,
}

fn print_docs_index(json: bool) -> Result<()> {
    let sections = rototo::docs::DOC_NAV_SECTIONS
        .iter()
        .map(|section| DocsSectionJson {
            title: section.title,
            pages: section
                .pages
                .iter()
                .filter_map(|id| docs_page_by_id(id))
                .map(|page| DocsPageSummaryJson {
                    id: page.id,
                    title: page.title,
                })
                .collect(),
        })
        .collect::<Vec<_>>();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&DocsIndexJson { sections })
                .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(());
    }

    for section in sections {
        println!("{}", style::label(section.title));
        for page in section.pages {
            // Pad before coloring so ANSI escapes do not break alignment.
            println!(
                "  {} {}",
                style::sea(&format!("{:<42}", page.id)),
                page.title
            );
        }
        println!();
    }
    Ok(())
}

fn print_docs_export(out: &Path, json: bool) -> Result<()> {
    let out = out.display().to_string();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&DocsExportJson { out })
                .map_err(|err| RototoError::new(err.to_string()))?
        );
    } else {
        println!(
            "{}",
            style::ok_line(&format!("exported documentation to {out}"))
        );
    }
    Ok(())
}

async fn write_package_readme(
    target: PackageReadmeTarget,
    out: &Path,
    docs_base_url: &str,
) -> Result<()> {
    let readme = rototo::docs::render_package_readme_with_base_url(target.id(), docs_base_url)?;
    if let Some(parent) = out.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| {
            RototoError::new(format!(
                "failed to create package README directory {}: {err}",
                parent.display()
            ))
        })?;
    }
    tokio::fs::write(out, readme).await.map_err(|err| {
        RototoError::new(format!(
            "failed to write package README {}: {err}",
            out.display()
        ))
    })
}

fn print_package_readme_export(target: PackageReadmeTarget, out: &Path, json: bool) -> Result<()> {
    let out = out.display().to_string();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&PackageReadmeExportJson {
                sdk: target.id(),
                out
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
    } else {
        println!("generated {} package README at {out}", target.id());
    }
    Ok(())
}

fn print_docs_page(prefix: &str, json: bool) -> Result<ExitCode> {
    match docs_page_by_prefix(prefix) {
        PagePrefixMatch::One(page) => {
            let markdown = render_cli_markdown(page.markdown)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&DocsPageJson {
                        id: page.id,
                        title: page.title,
                        markdown,
                    })
                    .map_err(|err| RototoError::new(err.to_string()))?
                );
            } else {
                print!("{}", style::render_markdown(&markdown));
            }
            Ok(ExitCode::SUCCESS)
        }
        PagePrefixMatch::Ambiguous(pages) => {
            println!("multiple documentation pages match \"{prefix}\":\n");
            for page in &pages {
                println!("  {:<42} {}", page.id, page.title);
            }
            println!("\nrun one of:");
            for page in pages {
                println!("  rototo docs -p {}", page.id);
            }
            Ok(ExitCode::FAILURE)
        }
        PagePrefixMatch::None => Err(RototoError::new(format!(
            "documentation page not found for prefix: {prefix}"
        ))),
    }
}

fn print_docs_search(query: &str, json: bool) -> Result<ExitCode> {
    let regex = Regex::new(query)
        .map_err(|err| RototoError::new(format!("invalid documentation search regex: {err}")))?;
    let matches = search_docs(query, &regex);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&DocsSearchJson {
                query: query.to_owned(),
                matches,
            })
            .map_err(|err| RototoError::new(err.to_string()))?
        );
        return Ok(ExitCode::SUCCESS);
    }

    let mut current_page = None;
    for hit in &matches {
        if current_page != Some(hit.page) {
            if current_page.is_some() {
                println!();
            }
            println!("{} - {}", hit.page, hit.title);
            current_page = Some(hit.page);
        }
        println!(
            "  {}: {}",
            hit.line,
            highlight_docs_search(&hit.text, &hit.spans)
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn highlight_docs_search(text: &str, spans: &[DocsSearchSpan]) -> String {
    let mut highlighted = String::new();
    let mut cursor = 0;
    for span in spans {
        if span.start < cursor || span.start == span.end {
            continue;
        }
        highlighted.push_str(&text[cursor..span.start]);
        highlighted.push_str("\x1b[7m");
        highlighted.push_str(&text[span.start..span.end]);
        highlighted.push_str("\x1b[0m");
        cursor = span.end;
    }
    highlighted.push_str(&text[cursor..]);
    highlighted
}

fn search_docs(query: &str, regex: &Regex) -> Vec<DocsSearchMatch> {
    let mut matches = Vec::new();
    for page in rototo::docs::DOCS {
        push_search_match(&mut matches, page, 0, page.id, regex);
        push_search_match(&mut matches, page, 0, page.title, regex);
        for (index, line) in page.markdown.lines().enumerate() {
            push_search_match(&mut matches, page, index + 1, line, regex);
        }
    }
    matches.sort_by_key(|hit| (docs_nav_index(hit.page), hit.line));
    if query.is_empty() {
        matches.clear();
    }
    matches
}

fn push_search_match(
    matches: &mut Vec<DocsSearchMatch>,
    page: &'static rototo::docs::DocPage,
    line: usize,
    text: &str,
    regex: &Regex,
) {
    let spans = regex
        .find_iter(text)
        .filter(|found| found.start() < found.end())
        .map(|found| DocsSearchSpan {
            start: found.start(),
            end: found.end(),
        })
        .collect::<Vec<_>>();
    if spans.is_empty() {
        return;
    }
    matches.push(DocsSearchMatch {
        page: page.id,
        title: page.title,
        line,
        text: text.to_owned(),
        spans,
    });
}

fn docs_page_by_prefix(prefix: &str) -> PagePrefixMatch {
    let prefix = normalize_docs_page_id(prefix);
    if let Some(page) = docs_page_by_id(&prefix) {
        return PagePrefixMatch::One(page);
    }
    let matches = rototo::docs::DOCS
        .iter()
        .filter(|page| page.id.starts_with(&prefix))
        .collect::<Vec<_>>();
    match matches.len() {
        0 => PagePrefixMatch::None,
        1 => PagePrefixMatch::One(matches[0]),
        _ => PagePrefixMatch::Ambiguous(matches),
    }
}

fn docs_page_by_id(id: &str) -> Option<&'static rototo::docs::DocPage> {
    let id = normalize_docs_page_id(id);
    rototo::docs::DOCS.iter().find(|page| page.id == id)
}

fn normalize_docs_page_id(id: &str) -> String {
    match id {
        "" | "/" | "index.html" => "index".to_owned(),
        _ => id.strip_suffix(".html").unwrap_or(id).to_owned(),
    }
}

fn docs_nav_index(page_id: &str) -> usize {
    rototo::docs::DOCS
        .iter()
        .position(|page| page.id == page_id)
        .unwrap_or(usize::MAX)
}

fn render_cli_markdown(markdown: &str) -> Result<String> {
    let link = Regex::new(r"\[([^\]\n]+)\]\(([^)\s]+)\)")
        .map_err(|err| RototoError::new(err.to_string()))?;
    Ok(link
        .replace_all(markdown, |captures: &regex::Captures<'_>| {
            let text = captures.get(1).expect("capture exists").as_str();
            let target = captures.get(2).expect("capture exists").as_str();
            if let Some(page_id) = internal_doc_link_target(target) {
                format!("{text} (rototo docs -p {page_id})")
            } else {
                captures.get(0).expect("capture exists").as_str().to_owned()
            }
        })
        .into_owned())
}

fn internal_doc_link_target(target: &str) -> Option<String> {
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with('#')
    {
        return None;
    }
    let target = target.split('#').next().unwrap_or(target);
    let file_name = Path::new(target).file_name()?.to_str()?;
    let id = file_name
        .strip_suffix(".md")
        .or_else(|| file_name.strip_suffix(".html"))?;
    docs_page_by_id(id).map(|page| page.id.to_owned())
}
