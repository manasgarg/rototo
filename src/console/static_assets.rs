use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};

/// Console UI assets built by `just console-build`. Release builds embed the
/// files; debug builds read them from disk so frontend rebuilds show up
/// without recompiling.
#[derive(rust_embed::Embed)]
#[folder = "apps/console/dist"]
struct ConsoleAssets;

const ASSETS_MISSING_PAGE: &str = r#"<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>rototo console</title></head>
<body style="font-family: system-ui; max-width: 38rem; margin: 4rem auto; line-height: 1.5;">
<h1>Console UI assets are not built</h1>
<p>The API is running, but this build of <code>rototo</code> has no console UI
bundle. Build it with:</p>
<pre><code>just console-build</code></pre>
<p>then restart <code>rototo console</code> (release builds embed the bundle
at compile time, so rebuild the binary after building the UI).</p>
</body>
</html>
"#;

/// Serves the embedded single-page app: exact asset paths first, and the SPA
/// shell for any route the client router owns.
pub async fn serve_spa(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(asset) = ConsoleAssets::get(path) {
        return asset_response(path, asset);
    }
    // Client-routed paths (no file extension) fall back to the shell.
    if let Some(index) = ConsoleAssets::get("index.html") {
        return asset_response("index.html", index);
    }
    (StatusCode::SERVICE_UNAVAILABLE, Html(ASSETS_MISSING_PAGE)).into_response()
}

fn asset_response(path: &str, asset: rust_embed::EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    // Vite emits content-hashed file names under assets/; everything else
    // (index.html, favicons) must revalidate so deploys take effect.
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, mime.as_ref().to_owned()),
            (header::CACHE_CONTROL, cache_control.to_owned()),
        ],
        asset.data.into_owned(),
    )
        .into_response()
}
