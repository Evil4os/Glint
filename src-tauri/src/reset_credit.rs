//! User-initiated redemptions through the documented Codex App Server protocol.
//! Never called by polling; no model/thread is started. Raw responses are not logged.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::Command,
    time::{timeout, Duration},
};

#[derive(Serialize, Deserialize)]
struct Attempt {
    key: String,
    credit_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Redemption {
    pub outcome: String,
    pub refreshed: bool,
}

fn valid_key(key: &str) -> bool {
    key.len() == 36
        && key.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

async fn rpc<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let mut request = json!({"id":id,"method":method,"params":params}).to_string();
    request.push('\n');
    writer
        .write_all(request.as_bytes())
        .await
        .map_err(|_| "connectionLost")?;
    writer.flush().await.map_err(|_| "connectionLost")?;
    loop {
        let mut line = String::new();
        if reader
            .read_line(&mut line)
            .await
            .map_err(|_| "connectionLost")?
            == 0
        {
            return Err("connectionLost".into());
        }
        let value: Value = serde_json::from_str(&line).map_err(|_| "invalidResponse")?;
        if value.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if value.get("error").is_some() {
            return Err("serverRejected".into());
        }
        return value
            .get("result")
            .cloned()
            .ok_or_else(|| "invalidResponse".into());
    }
}

fn prepare_attempt(path: &Path, key: &str, limits: &Value) -> Result<Option<Attempt>, String> {
    if path.exists() {
        let saved: Attempt =
            serde_json::from_slice(&std::fs::read(path).map_err(|_| "attemptStorage")?)
                .map_err(|_| "attemptStorage")?;
        // A retry always targets the same credit, even after it disappears from the
        // available list. This also prevents an account switch from spending a different card.
        if saved.key == key {
            return Ok(Some(saved));
        }
    }
    let summary = &limits["rateLimitResetCredits"];
    if summary["availableCount"].as_u64() == Some(0) {
        return Ok(None);
    }
    let credit = summary["credits"]
        .as_array()
        .and_then(|rows| {
            rows.iter().find(|row| {
                row["status"] == "available"
                    && row["resetType"] == "codexRateLimits"
                    && row["id"].as_str().is_some_and(|id| !id.is_empty())
            })
        })
        .ok_or("creditDetailsUnavailable")?;
    let attempt = Attempt {
        key: key.into(),
        credit_id: credit["id"].as_str().unwrap().into(),
    };
    // Commit before any request that can consume a card. Failure prevents redemption.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "attemptStorage")?;
    }
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(&attempt).map_err(|_| "attemptStorage")?;
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|_| "attemptStorage")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "attemptStorage")?;
    std::fs::rename(temp, path).map_err(|_| "attemptStorage")?;
    Ok(Some(attempt))
}

async fn exchange<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    key: &str,
    path: &Path,
) -> Result<Redemption, String> {
    let result = timeout(Duration::from_secs(35), async {
        rpc(reader, writer, 1, "initialize", json!({"clientInfo":{"name":"glint","title":"Glint","version":env!("CARGO_PKG_VERSION")}})).await?;
        writer.write_all(b"{\"method\":\"initialized\",\"params\":{}}\n").await.map_err(|_| "connectionLost")?;
        let limits = rpc(reader, writer, 2, "account/rateLimits/read", json!({})).await?;
        let Some(attempt) = prepare_attempt(path, key, &limits)? else {
            return Ok::<Value, String>(json!({"outcome":"noCredit"}));
        };
        rpc(reader, writer, 3, "account/rateLimitResetCredit/consume", json!({"idempotencyKey":attempt.key,"creditId":attempt.credit_id})).await
    }).await.map_err(|_| "requestTimeout")??;
    let outcome = result["outcome"]
        .as_str()
        .filter(|s| ["reset", "alreadyRedeemed", "nothingToReset", "noCredit"].contains(s))
        .ok_or("invalidResponse")?
        .to_string();
    // Once the outcome is known, a subsequent refresh failure must never disguise it
    // as a failed redemption. Re-read instead of predicting reset windows locally.
    let refreshed = timeout(
        Duration::from_secs(10),
        rpc(reader, writer, 4, "account/rateLimits/read", json!({})),
    )
    .await
    .is_ok_and(|v| v.is_ok());
    Ok(Redemption { outcome, refreshed })
}

fn cli_path() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        let local = home.join(".local/bin/codex");
        if local.is_file() {
            return local;
        }
    }
    for path in ["/opt/homebrew/bin/codex", "/usr/local/bin/codex"] {
        if Path::new(path).is_file() {
            return path.into();
        }
    }
    "codex".into()
}

pub async fn consume(key: &str, path: &Path) -> Result<Redemption, String> {
    if !valid_key(key) {
        return Err("invalidKey".into());
    }
    let mut child = Command::new(cli_path())
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "cliUnavailable")?;
    let mut input = child.stdin.take().ok_or("connectionLost")?;
    // Bound the entire protocol transcript, including unsolicited notifications.
    let mut output = BufReader::new(
        child
            .stdout
            .take()
            .ok_or("connectionLost")?
            .take(1024 * 1024),
    );
    let result = exchange(&mut output, &mut input, key, path).await;
    drop(input);
    let _ = child.kill().await;
    let _ = timeout(Duration::from_secs(2), child.wait()).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    const KEY: &str = "12345678-1234-4234-8234-123456789abc";
    fn path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("quota-reset-{}-{label}.json", std::process::id()))
    }
    fn limits(id: &str) -> Value {
        json!({"rateLimitResetCredits":{"availableCount":1,"credits":[{"id":id,"status":"available","resetType":"codexRateLimits"}]}})
    }
    #[test]
    fn retry_preserves_credit_after_available_list_changes() {
        let path = path("retry");
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            prepare_attempt(&path, KEY, &limits("first"))
                .unwrap()
                .unwrap()
                .credit_id,
            "first"
        );
        assert_eq!(
            prepare_attempt(&path, KEY, &limits("second"))
                .unwrap()
                .unwrap()
                .credit_id,
            "first"
        );
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn missing_credit_details_fail_closed_and_zero_is_not_redeemed() {
        let path = path("missing");
        assert!(prepare_attempt(
            &path,
            KEY,
            &json!({"rateLimitResetCredits":{"availableCount":1}})
        )
        .is_err());
        assert!(prepare_attempt(
            &path,
            KEY,
            &json!({"rateLimitResetCredits":{"availableCount":0}})
        )
        .unwrap()
        .is_none());
    }
    #[test]
    fn validates_request_keys() {
        assert!(valid_key(KEY));
        assert!(!valid_key(""));
        assert!(!valid_key("arbitrary-request"));
    }
    #[tokio::test]
    async fn protocol_initializes_consumes_and_reads_again_for_every_outcome() {
        for outcome in ["reset", "alreadyRedeemed", "nothingToReset", "noCredit"] {
            let path = path(outcome);
            let _ = std::fs::remove_file(&path);
            let (client, server) = tokio::io::duplex(8192);
            let (r, mut w) = tokio::io::split(client);
            let expected = outcome.to_string();
            let server = tokio::spawn(async move {
                let (r, mut w) = tokio::io::split(server);
                let mut r = BufReader::new(r);
                let mut requests = Vec::new();
                for step in 0..5 {
                    let mut line = String::new();
                    r.read_line(&mut line).await.unwrap();
                    let v: Value = serde_json::from_str(&line).unwrap();
                    requests.push(v["method"].as_str().unwrap().to_string());
                    if step == 1 {
                        assert_eq!(v["method"], "initialized");
                        continue;
                    }
                    let result = match step {
                        0 => json!({}),
                        2 => limits("credit-one"),
                        3 => {
                            assert_eq!(v["params"]["idempotencyKey"], KEY);
                            assert_eq!(v["params"]["creditId"], "credit-one");
                            json!({"outcome":expected})
                        }
                        _ => json!({"rateLimitResetCredits":{"availableCount":0}}),
                    };
                    // Interleaved notifications must not be mistaken for a response.
                    w.write_all(b"{\"method\":\"account/updated\"}\n")
                        .await
                        .unwrap();
                    w.write_all(format!("{}\n", json!({"id":v["id"],"result":result})).as_bytes())
                        .await
                        .unwrap();
                }
                assert_eq!(
                    requests,
                    [
                        "initialize",
                        "initialized",
                        "account/rateLimits/read",
                        "account/rateLimitResetCredit/consume",
                        "account/rateLimits/read"
                    ]
                );
            });
            let result = exchange(&mut BufReader::new(r), &mut w, KEY, &path)
                .await
                .unwrap();
            assert_eq!(result.outcome, outcome);
            assert!(result.refreshed);
            server.await.unwrap();
            let _ = std::fs::remove_file(path);
        }
    }
    #[tokio::test]
    async fn success_is_preserved_when_followup_read_disconnects() {
        let path = path("disconnect");
        let _ = std::fs::remove_file(&path);
        let (client, server) = tokio::io::duplex(8192);
        let (r, mut w) = tokio::io::split(client);
        let server = tokio::spawn(async move {
            let (r, mut w) = tokio::io::split(server);
            let mut r = BufReader::new(r);
            for step in 0..5 {
                let mut line = String::new();
                r.read_line(&mut line).await.unwrap();
                let v: Value = serde_json::from_str(&line).unwrap();
                if step == 1 {
                    continue;
                }
                if step == 4 {
                    break;
                }
                let result = if step == 0 {
                    json!({})
                } else if step == 2 {
                    limits("credit-one")
                } else {
                    json!({"outcome":"reset"})
                };
                w.write_all(format!("{}\n", json!({"id":v["id"],"result":result})).as_bytes())
                    .await
                    .unwrap();
            }
        });
        let result = exchange(&mut BufReader::new(r), &mut w, KEY, &path)
            .await
            .unwrap();
        assert_eq!(result.outcome, "reset");
        assert!(!result.refreshed);
        server.await.unwrap();
        let _ = std::fs::remove_file(path);
    }
}
