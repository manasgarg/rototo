use std::path::Path;

use regex::Regex;

use crate::error::{Result, RototoError};

use super::markdown::sdk_snippet_code;
use super::{DEFAULT_DOCS_BASE_URL, DOCS, get_page, page_href};

/// The Rust crate README is the hand-authored, canonical SDK README. Every
/// language SDK README is derived from it: shared prose and the CLI walkthrough
/// are copied verbatim, and only the title plus the per-language runtime example
/// are swapped in.
const ROOT_README: &str = include_str!("../../README.md");

/// Marks the swappable runtime-example region inside `README.md`. The markers
/// are HTML comments, so they stay invisible on crates.io while letting the
/// generator find and replace the region for other languages.
const RUNTIME_START: &str = "<!-- rototo:sdk-quickstart:start -->";
const RUNTIME_END: &str = "<!-- rototo:sdk-quickstart:end -->";

/// The runtime example programs live once, in the quickstart page's snippet
/// group; READMEs reuse them rather than keeping their own copies.
const QUICKSTART_PAGE: &str = "quickstart";
const QUICKSTART_GROUP: &str = "quickstart-app";

struct SdkReadme {
    title: &'static str,
    language_name: &'static str,
    /// Language id inside the quickstart snippet group.
    snippet_language: &'static str,
    /// Markdown for installing the SDK (one or more fenced blocks). A
    /// `{version}` placeholder is filled with the canonical crate version.
    install: &'static str,
    /// File the example program is saved as.
    save_as: &'static str,
    /// Command used to run the example, already wrapped in backticks.
    run: &'static str,
}

fn sdk_readme(sdk: &str) -> Result<SdkReadme> {
    Ok(match sdk {
        "python" => SdkReadme {
            title: "rototo Python SDK",
            language_name: "Python",
            snippet_language: "python",
            install: "```sh\npython -m pip install rototo\n```",
            save_as: "hello-rototo.py",
            run: "`python hello-rototo.py`",
        },
        "typescript" => SdkReadme {
            title: "rototo TypeScript SDK",
            language_name: "TypeScript",
            snippet_language: "typescript",
            install: "```sh\nnpm install rototo\n```",
            save_as: "hello-rototo.ts",
            run: "`npx tsx hello-rototo.ts`",
        },
        "go" => SdkReadme {
            title: "rototo Go SDK",
            language_name: "Go",
            snippet_language: "go",
            install: "```sh\ngo get github.com/manasgarg/rototo/sdks/go@v{version}\n```",
            save_as: "main.go",
            run: "`go run main.go`",
        },
        "java" => SdkReadme {
            title: "rototo Java SDK",
            language_name: "Java",
            snippet_language: "java",
            install: "```gradle\nimplementation(\"dev.rototo:rototo:{version}\")\n```\n\nOr with Maven:\n\n```xml\n<dependency>\n  <groupId>dev.rototo</groupId>\n  <artifactId>rototo</artifactId>\n  <version>{version}</version>\n</dependency>\n```",
            save_as: "HelloRototo.java",
            run: "`java HelloRototo.java`",
        },
        other => {
            return Err(RototoError::new(format!(
                "unsupported package README SDK: {other}"
            )));
        }
    })
}

pub fn render_package_readme(sdk: &str) -> Result<String> {
    render_package_readme_with_base_url(sdk, DEFAULT_DOCS_BASE_URL)
}

pub fn render_package_readme_with_base_url(sdk: &str, docs_base_url: &str) -> Result<String> {
    let docs_base_url = normalize_docs_base_url(docs_base_url)?;
    let meta = sdk_readme(sdk)?;

    let program = sdk_snippet_code(
        get_page(QUICKSTART_PAGE)?.markdown,
        QUICKSTART_GROUP,
        meta.snippet_language,
    )
    .ok_or_else(|| {
        RototoError::new(format!(
            "quickstart snippet group `{QUICKSTART_GROUP}` is missing `{}`",
            meta.snippet_language
        ))
    })?;

    let runtime = build_runtime_section(&meta, &program);
    let readme = replace_runtime_region(ROOT_README, &runtime)?;
    let readme = swap_title(&readme, meta.title);
    let readme = rewrite_package_readme_doc_links(&readme, docs_base_url);

    Ok(format!(
        "<!-- Generated from README.md by `rototo docs --package-readme {sdk} --out sdks/{sdk}/README.md`. Do not edit directly. -->\n\n{readme}",
    ))
}

fn build_runtime_section(meta: &SdkReadme, program: &str) -> String {
    let install = meta.install.replace("{version}", env!("CARGO_PKG_VERSION"));
    format!(
        "### Load the configuration package and resolve the threshold\n\
         \n\
         Now let's read that value from an application. Install the rototo {language} SDK:\n\
         \n\
         {install}\n\
         \n\
         Save this as `{save_as}`. It loads a *refreshing* package (one that re-reads the source in the background) and prints the free-shipping threshold for a standard and a premium account every couple of seconds:\n\
         \n\
         ```{snippet_language}\n\
         {program}```\n\
         \n\
         Run it ({run}) from the directory that holds `app-config`, and it prints:\n\
         \n\
         ```text\n\
         ---\n\
         standard: 50 USD\n\
         premium: 25 USD\n\
         ```\n\
         \n\
         Now edit `free_shipping_threshold.toml`, change the default to 35, and save. Because the package refreshes every second, the next tick shows:\n\
         \n\
         ```text\n\
         ---\n\
         standard: 50 USD\n\
         premium: 35 USD\n\
         ```",
        language = meta.language_name,
        install = install,
        save_as = meta.save_as,
        snippet_language = meta.snippet_language,
        program = program,
        run = meta.run,
    )
}

fn replace_runtime_region(readme: &str, runtime: &str) -> Result<String> {
    let start = readme.find(RUNTIME_START).ok_or_else(|| {
        RototoError::new("README.md is missing the rototo:sdk-quickstart start marker")
    })?;
    let end = readme.find(RUNTIME_END).ok_or_else(|| {
        RototoError::new("README.md is missing the rototo:sdk-quickstart end marker")
    })?;
    if end < start {
        return Err(RototoError::new(
            "README.md rototo:sdk-quickstart markers are out of order",
        ));
    }
    Ok(format!(
        "{before}{runtime}{after}",
        before = &readme[..start],
        after = &readme[end + RUNTIME_END.len()..],
    ))
}

fn swap_title(readme: &str, title: &str) -> String {
    match readme.split_once('\n') {
        Some((_, rest)) => format!("# {title}\n{rest}"),
        None => format!("# {title}\n"),
    }
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
