//! User-initiated redemptions through the documented Codex App Server protocol.
//! Never called by polling; no model/thread is started. Raw responses are not logged.
use crate::companion::CODEX_BUNDLE_ID;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_foundation::NSString;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::ErrorKind,
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

fn saved_attempt(path: &Path, key: &str) -> Result<Option<Attempt>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("attemptStorage".into()),
    };
    let saved: Attempt = serde_json::from_slice(&bytes).map_err(|_| "attemptStorage")?;
    Ok((saved.key == key && !saved.credit_id.is_empty()).then_some(saved))
}

pub fn has_attempt(path: &Path, key: &str) -> Result<bool, String> {
    Ok(valid_key(key) && saved_attempt(path, key)?.is_some())
}

fn prepare_attempt(
    path: &Path,
    key: &str,
    limits: &Value,
    saved: Option<Attempt>,
) -> Result<Option<Attempt>, String> {
    let summary = &limits["rateLimitResetCredits"];
    if summary["availableCount"].as_u64() == Some(0) {
        return Ok(None);
    }
    // A retry always targets the original credit, including when the list is incomplete.
    if saved.is_some() {
        return Ok(saved);
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
        fs::create_dir_all(parent).map_err(|_| "attemptStorage")?;
    }
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(&attempt).map_err(|_| "attemptStorage")?;
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
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
    fs::rename(temp, path).map_err(|_| "attemptStorage")?;
    Ok(Some(attempt))
}

async fn exchange<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    key: &str,
    path: &Path,
    saved: Option<Attempt>,
) -> Result<Redemption, String> {
    let result = timeout(Duration::from_secs(35), async {
        rpc(reader, writer, 1, "initialize", json!({"clientInfo":{"name":"glint","title":"Glint","version":env!("CARGO_PKG_VERSION")}}))
            .await.map_err(|_| "appServerUnavailable")?;
        writer.write_all(b"{\"method\":\"initialized\",\"params\":{}}\n").await.map_err(|_| "appServerUnavailable")?;
        let limits = rpc(reader, writer, 2, "account/rateLimits/read", json!({}))
            .await.map_err(|_| "creditDetailsUnavailable")?;
        let Some(attempt) = prepare_attempt(path, key, &limits, saved)? else {
            return Ok(None);
        };
        rpc(reader, writer, 3, "account/rateLimitResetCredit/consume", json!({"idempotencyKey":attempt.key,"creditId":attempt.credit_id})).await.map(Some)
    }).await.map_err(|_| "requestTimeout")??;
    let Some(result) = result else {
        return Ok(Redemption {
            outcome: "noCredit".into(),
            refreshed: true,
        });
    };
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

fn desktop_cli_path(bundle: &Path) -> Option<PathBuf> {
    let resources = bundle.join("Contents/Resources");
    ["codex-cli/CodexCLI.app/Contents/MacOS/codex", "codex"]
        .into_iter()
        .map(|relative| resources.join(relative))
        .find(|path| path.is_file())
}

fn find_desktop_cli() -> Option<PathBuf> {
    autoreleasepool(|_| {
        let bundle_id = NSString::from_str(CODEX_BUNDLE_ID);
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id)
            .iter()
            .filter(|app| !app.isTerminated())
            .filter_map(|app| app.bundleURL()?.to_file_path())
            .find_map(|bundle| desktop_cli_path(&bundle))
            .or_else(|| {
                let bundle = NSWorkspace::sharedWorkspace()
                    .URLForApplicationWithBundleIdentifier(&bundle_id)?
                    .to_file_path()?;
                desktop_cli_path(&bundle)
            })
    })
}

fn cli_command() -> Command {
    if let Some(cli) = find_desktop_cli() {
        return Command::new(cli);
    }

    let bin_dirs: Vec<_> = dirs::home_dir()
        .map(|home| home.join(".local/bin"))
        .into_iter()
        .chain(["/opt/homebrew/bin".into(), "/usr/local/bin".into()])
        .collect();
    let cli = bin_dirs
        .iter()
        .map(|dir| dir.join("codex"))
        .find(|path| path.is_file())
        .unwrap_or_else(|| "codex".into());
    let mut command = Command::new(cli);
    // npm launchers resolve Node through env, even when Codex was found by absolute path.
    let inherited = env::var_os("PATH").unwrap_or_default();
    if let Ok(path) = env::join_paths(
        env::split_paths(&inherited)
            .filter(|path| !path.as_os_str().is_empty())
            .chain(bin_dirs),
    ) {
        command.env("PATH", path);
    }
    command
}

pub async fn consume(key: &str, path: &Path, retry: bool) -> Result<Redemption, String> {
    if !valid_key(key) {
        return Err("invalidKey".into());
    }
    let saved = saved_attempt(path, key)?;
    if retry && saved.is_none() {
        // Older versions saved the WebView key before finding a credit. An orphaned
        // retry key must never authorize the selection of a new card.
        return Ok(Redemption {
            outcome: "notStarted".into(),
            refreshed: false,
        });
    }
    let mut child = cli_command()
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
    let result = exchange(&mut output, &mut input, key, path, saved).await;
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
            prepare_attempt(&path, KEY, &limits("first"), None)
                .unwrap()
                .unwrap()
                .credit_id,
            "first"
        );
        assert_eq!(
            prepare_attempt(
                &path,
                KEY,
                &limits("second"),
                saved_attempt(&path, KEY).unwrap()
            )
            .unwrap()
            .unwrap()
            .credit_id,
            "first"
        );
        assert!(prepare_attempt(
            &path,
            KEY,
            &json!({"rateLimitResetCredits":{"availableCount":0}}),
            saved_attempt(&path, KEY).unwrap(),
        )
        .unwrap()
        .is_none());
        let _ = std::fs::remove_file(path);
    }
    #[tokio::test]
    async fn retry_for_unsubmitted_key_returns_before_starting_the_cli() {
        let path = path("unsubmitted");
        let _ = fs::remove_file(&path);
        assert!(!has_attempt(&path, KEY).unwrap());
        let other_key = "87654321-1234-4234-8234-123456789abc";
        prepare_attempt(&path, other_key, &limits("previous-credit"), None).unwrap();
        assert!(!has_attempt(&path, KEY).unwrap());
        let result = consume(KEY, &path, true).await.unwrap();
        assert_eq!(result.outcome, "notStarted");
        assert!(has_attempt(&path, other_key).unwrap());
        let _ = fs::remove_file(path);
    }
    #[test]
    fn prefers_native_cli_in_current_bundle_layout() {
        let bundle = path("renamed-desktop.app");
        let resources = bundle.join("Contents/Resources");
        let current_cli = resources.join("codex-cli/CodexCLI.app/Contents/MacOS/codex");
        let old_cli = resources.join("codex");
        fs::create_dir_all(current_cli.parent().unwrap()).unwrap();
        fs::write(&old_cli, "").unwrap();
        assert_eq!(desktop_cli_path(&bundle), Some(old_cli));
        fs::write(&current_cli, "").unwrap();
        assert_eq!(desktop_cli_path(&bundle), Some(current_cli));
        fs::remove_dir_all(bundle).unwrap();
    }
    #[test]
    fn missing_credit_details_fail_closed_and_zero_is_not_redeemed() {
        let path = path("missing");
        assert!(prepare_attempt(
            &path,
            KEY,
            &json!({"rateLimitResetCredits":{"availableCount":1}}),
            None,
        )
        .is_err());
        assert!(prepare_attempt(
            &path,
            KEY,
            &json!({"rateLimitResetCredits":{"availableCount":0}}),
            None,
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
            let result = exchange(&mut BufReader::new(r), &mut w, KEY, &path, None)
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
        let result = exchange(&mut BufReader::new(r), &mut w, KEY, &path, None)
            .await
            .unwrap();
        assert_eq!(result.outcome, "reset");
        assert!(!result.refreshed);
        server.await.unwrap();
        let _ = std::fs::remove_file(path);
    }
}
