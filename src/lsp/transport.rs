use serde_json::{Value as JsonValue, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{Result, RototoError};

const JSONRPC_VERSION: &str = "2.0";
const MAX_CONTENT_LENGTH: usize = 4 * 1024 * 1024;

pub(crate) async fn read_message<R>(reader: &mut R) -> Result<Option<JsonValue>>
where
    R: AsyncBufRead + Unpin,
{
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .await
            .map_err(|err| RototoError::new(format!("failed to read LSP header: {err}")))?;
        if bytes == 0 {
            return Ok(None);
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(value.trim().parse::<usize>().map_err(|err| {
                RototoError::new(format!("invalid LSP Content-Length header: {err}"))
            })?);
        }
    }

    let content_length =
        content_length.ok_or_else(|| RototoError::new("missing LSP Content-Length header"))?;
    if content_length > MAX_CONTENT_LENGTH {
        return Err(RototoError::new(format!(
            "LSP Content-Length exceeds maximum of {MAX_CONTENT_LENGTH} bytes"
        )));
    }
    let mut body = vec![0; content_length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|err| RototoError::new(format!("failed to read LSP body: {err}")))?;
    let message = serde_json::from_slice(&body)
        .map_err(|err| RototoError::new(format!("failed to parse LSP JSON body: {err}")))?;
    Ok(Some(message))
}

#[cfg(feature = "console")]
pub(crate) async fn write_notification<W>(
    writer: &mut W,
    method: &str,
    params: JsonValue,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    write_message(
        writer,
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "method": method,
            "params": params,
        }),
    )
    .await
}

#[cfg(feature = "console")]
pub(crate) async fn write_request<W>(
    writer: &mut W,
    id: i64,
    method: &str,
    params: JsonValue,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    write_message(
        writer,
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id,
            "method": method,
            "params": params,
        }),
    )
    .await
}

pub(super) fn response_message(id: JsonValue, result: JsonValue) -> JsonValue {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "result": result,
    })
}

pub(super) fn error_response_message(id: JsonValue, code: i64, message: &str) -> JsonValue {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "error": {
            "code": code,
            "message": message,
        },
    })
}

pub(super) fn notification_message(method: &str, params: JsonValue) -> JsonValue {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "method": method,
        "params": params,
    })
}

pub(super) async fn write_message<W>(writer: &mut W, message: JsonValue) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let body = serde_json::to_vec(&message)
        .map_err(|err| RototoError::new(format!("failed to serialize LSP message: {err}")))?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await
        .map_err(|err| RototoError::new(format!("failed to write LSP header: {err}")))?;
    writer
        .write_all(&body)
        .await
        .map_err(|err| RototoError::new(format!("failed to write LSP body: {err}")))?;
    writer
        .flush()
        .await
        .map_err(|err| RototoError::new(format!("failed to flush LSP output: {err}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tokio::io::BufReader;

    use super::*;

    #[tokio::test]
    async fn read_message_rejects_oversized_content_length_before_allocation() {
        // LSP messages are framed with Content-Length. Rejecting huge lengths
        // before allocation keeps a bad client from forcing a large buffer.
        let input = format!(
            "Content-Length: {}\r\n\r\n",
            MAX_CONTENT_LENGTH.saturating_add(1)
        );
        let err = read_message(&mut BufReader::new(input.as_bytes()))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("exceeds maximum"));
    }

    #[tokio::test]
    async fn read_message_rejects_missing_and_malformed_content_length() {
        // Without a valid Content-Length header, the server cannot know where
        // the JSON body ends, so malformed framing is a protocol error.
        let missing = read_message(&mut BufReader::new("\r\n{}".as_bytes()))
            .await
            .unwrap_err();
        assert!(missing.to_string().contains("missing LSP Content-Length"));

        let malformed = read_message(&mut BufReader::new(
            "Content-Length: nope\r\n\r\n{}".as_bytes(),
        ))
        .await
        .unwrap_err();
        assert!(malformed.to_string().contains("invalid LSP Content-Length"));
    }

    #[tokio::test]
    async fn read_message_accepts_valid_framing() {
        // This is the happy-path framing shape sent by real LSP clients:
        // headers, a blank line, then exactly Content-Length bytes of JSON.
        let body = r#"{"jsonrpc":"2.0","method":"ping"}"#;
        let input = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        let mut input = BufReader::new(input.as_bytes());
        let message = read_message(&mut input).await.unwrap().unwrap();

        assert_eq!(message["method"], "ping");
    }
}
