use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

pub struct SocketApi {
    path: PathBuf,
    next_id: u32,
}

impl SocketApi {
    pub fn from_env() -> Result<Self> {
        let path = std::env::var_os("HERDR_SOCKET_PATH")
            .map(PathBuf::from)
            .context("HERDR_SOCKET_PATH is not set")?;
        Ok(Self { path, next_id: 1 })
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = format!("herdr-layouts:{}", self.next_id);
        self.next_id += 1;
        let request = json!({"id": id, "method": method, "params": params});
        let mut stream = UnixStream::connect(&self.path)
            .with_context(|| format!("failed to connect to {}", self.path.display()))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        serde_json::to_writer(&mut stream, &request)?;
        stream.write_all(b"\n")?;
        stream.flush()?;

        let mut response = String::new();
        BufReader::new(stream).read_line(&mut response)?;
        parse_response(&response)
    }
}

fn parse_response(response: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(response).context("invalid Herdr API response")?;
    if let Some(error) = value.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("api_error");
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Herdr API request failed");
        bail!("{code}: {message}");
    }
    value
        .get("result")
        .cloned()
        .context("Herdr API response has no result")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_success_result() {
        let result = parse_response(r#"{"id":"1","result":{"type":"pong"}}"#).unwrap();

        assert_eq!(result["type"], "pong");
    }

    #[test]
    fn reports_an_api_error() {
        let error =
            parse_response(r#"{"id":"1","error":{"code":"split_not_found","message":"missing"}}"#)
                .unwrap_err();

        assert_eq!(error.to_string(), "split_not_found: missing");
    }
}
