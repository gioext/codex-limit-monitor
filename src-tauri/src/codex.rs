use chrono::DateTime;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

const APP_SERVER_TIMEOUT_SECS: u64 = 12;
const HTTP_TIMEOUT_SECS: u64 = 12;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexLimitData {
    pub remaining_percent: i64,
    pub used_percent: i64,
    pub weekly_reset_at: i64,
    pub window_duration_mins: i64,
    pub reset_available_count: i64,
    pub reset_expiries: Vec<i64>,
    pub expiry_details_available: bool,
    pub plan_type: Option<String>,
    pub last_updated_at: i64,
}

#[derive(Debug, Deserialize)]
struct AuthFile {
    tokens: Option<AuthTokens>,
}

#[derive(Debug, Deserialize)]
struct AuthTokens {
    access_token: String,
    account_id: String,
}

#[derive(Debug, Deserialize)]
struct ResetCreditsResponse {
    available_count: i64,
    #[serde(default)]
    credits: Vec<ResetCreditResponse>,
}

#[derive(Debug, Deserialize)]
struct ResetCreditResponse {
    status: String,
    expires_at: Option<String>,
}

pub async fn fetch() -> Result<CodexLimitData, String> {
    tokio::task::spawn_blocking(fetch_blocking)
        .await
        .map_err(|error| format!("利用枠取得処理が終了しました: {error}"))?
}

fn fetch_blocking() -> Result<CodexLimitData, String> {
    let result = fetch_rate_limits_from_app_server()?;
    parse_limit_data(&result)
}

fn fetch_rate_limits_from_app_server() -> Result<Value, String> {
    let codex_binary = find_codex_binary().ok_or_else(|| {
        "Codex CLIが見つかりません。Codexをインストールまたは更新してください。".to_string()
    })?;

    let mut child = Command::new(&codex_binary)
        .args(["app-server", "--stdio"])
        .current_dir(dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Codex app-serverを起動できません: {error}"))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Codex app-serverの入力を開けません".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Codex app-serverの出力を開けません".to_string())?;

    let requests = [
        serde_json::json!({
            "method": "initialize",
            "id": 0,
            "params": {
                "clientInfo": {
                    "name": "codex_limit_monitor",
                    "title": "Codex Limit Monitor",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": { "experimentalApi": true }
            }
        }),
        serde_json::json!({ "method": "initialized", "params": {} }),
        serde_json::json!({ "method": "account/rateLimits/read", "id": 1, "params": null }),
    ];

    for request in requests {
        writeln!(stdin, "{request}")
            .map_err(|error| format!("Codex app-serverへ要求を送れません: {error}"))?;
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message.get("id").and_then(Value::as_i64) == Some(1) {
                let _ = sender.send(message);
                break;
            }
        }
    });

    let response = receiver
        .recv_timeout(Duration::from_secs(APP_SERVER_TIMEOUT_SECS))
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => {
                "Codex app-serverからの応答がタイムアウトしました".to_string()
            }
            mpsc::RecvTimeoutError::Disconnected => {
                "Codex app-serverが応答前に終了しました".to_string()
            }
        });

    let _ = child.kill();
    let _ = child.wait();

    let response = response?;
    if let Some(error) = response.get("error") {
        return Err(format!("Codexの利用枠を取得できません: {error}"));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| "Codexの利用枠レスポンスが空です".to_string())
}

fn parse_limit_data(result: &Value) -> Result<CodexLimitData, String> {
    let snapshot = result
        .pointer("/rateLimitsByLimitId/codex")
        .or_else(|| result.get("rateLimits"))
        .ok_or_else(|| "Codexの週次利用枠が見つかりません".to_string())?;
    let primary = snapshot
        .get("primary")
        .ok_or_else(|| "Codexの週次利用枠ウィンドウが見つかりません".to_string())?;

    let used_percent = primary
        .get("usedPercent")
        .and_then(Value::as_i64)
        .ok_or_else(|| "Codexの使用率が不明です".to_string())?
        .clamp(0, 100);
    let weekly_reset_at = primary
        .get("resetsAt")
        .and_then(Value::as_i64)
        .ok_or_else(|| "Codexの次回リセット日時が不明です".to_string())?;
    let window_duration_mins = primary
        .get("windowDurationMins")
        .and_then(Value::as_i64)
        .unwrap_or(10_080);

    let reset_summary = result.get("rateLimitResetCredits");
    let mut reset_available_count = reset_summary
        .and_then(|summary| summary.get("availableCount"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let mut reset_expiries = parse_app_server_expiries(reset_summary);
    let mut expiry_details_available = reset_summary
        .and_then(|summary| summary.get("credits"))
        .and_then(Value::as_array)
        .is_some();

    if !expiry_details_available && reset_available_count > 0 {
        if let Ok(details) = fetch_reset_credit_details() {
            reset_available_count = details.available_count.max(0);
            reset_expiries = details.expiries;
            expiry_details_available = true;
        }
    }

    reset_expiries.sort_unstable();

    Ok(CodexLimitData {
        remaining_percent: 100 - used_percent,
        used_percent,
        weekly_reset_at,
        window_duration_mins,
        reset_available_count,
        reset_expiries,
        expiry_details_available,
        plan_type: snapshot
            .get("planType")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        last_updated_at: chrono::Utc::now().timestamp(),
    })
}

fn parse_app_server_expiries(reset_summary: Option<&Value>) -> Vec<i64> {
    reset_summary
        .and_then(|summary| summary.get("credits"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|credit| credit.get("status").and_then(Value::as_str) == Some("available"))
        .filter_map(|credit| credit.get("expiresAt").and_then(Value::as_i64))
        .collect()
}

struct ResetDetails {
    available_count: i64,
    expiries: Vec<i64>,
}

fn fetch_reset_credit_details() -> Result<ResetDetails, String> {
    let auth_path = dirs::home_dir()
        .ok_or_else(|| "ホームディレクトリが見つかりません".to_string())?
        .join(".codex/auth.json");
    let auth: AuthFile = serde_json::from_str(
        &fs::read_to_string(&auth_path)
            .map_err(|error| format!("Codexの認証情報を読めません: {error}"))?,
    )
    .map_err(|error| format!("Codexの認証情報を解析できません: {error}"))?;
    let tokens = auth
        .tokens
        .ok_or_else(|| "CodexへChatGPTでログインしてください".to_string())?;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("HTTPクライアントを初期化できません: {error}"))?;
    let response = client
        .get("https://chatgpt.com/backend-api/wham/rate-limit-reset-credits")
        .bearer_auth(tokens.access_token)
        .header("ChatGPT-Account-Id", tokens.account_id)
        .header("User-Agent", "codex-limit-monitor")
        .send()
        .map_err(|error| format!("リセット有効期限を取得できません: {error}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "リセット有効期限を取得できません (HTTP {})",
            response.status()
        ));
    }

    let payload: ResetCreditsResponse = response
        .json()
        .map_err(|error| format!("リセット有効期限を解析できません: {error}"))?;
    let expiries = payload
        .credits
        .into_iter()
        .filter(|credit| credit.status == "available")
        .filter_map(|credit| credit.expires_at)
        .filter_map(|expires_at| DateTime::parse_from_rfc3339(&expires_at).ok())
        .map(|expires_at| expires_at.timestamp())
        .collect();

    Ok(ResetDetails {
        available_count: payload.available_count,
        expiries,
    })
}

fn find_codex_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CODEX_BINARY").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }

    let home = dirs::home_dir()?;
    [
        home.join(".local/bin/codex"),
        home.join(".codex/packages/standalone/current/bin/codex"),
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_weekly_remaining_and_sorted_expiries() {
        let result = serde_json::json!({
            "rateLimitsByLimitId": {
                "codex": {
                    "primary": {
                        "usedPercent": 27,
                        "windowDurationMins": 10080,
                        "resetsAt": 1784598710
                    },
                    "planType": "pro"
                }
            },
            "rateLimitResetCredits": {
                "availableCount": 2,
                "credits": [
                    { "status": "available", "expiresAt": 1786000000 },
                    { "status": "available", "expiresAt": 1785000000 },
                    { "status": "redeemed", "expiresAt": 1784000000 }
                ]
            }
        });

        let parsed = parse_limit_data(&result).expect("valid rate limit payload");

        assert_eq!(parsed.remaining_percent, 73);
        assert_eq!(parsed.used_percent, 27);
        assert_eq!(parsed.reset_available_count, 2);
        assert_eq!(parsed.reset_expiries, vec![1785000000, 1786000000]);
        assert!(parsed.expiry_details_available);
    }

    #[test]
    #[ignore = "requires a local Codex ChatGPT login and network access"]
    fn fetches_live_codex_limits() {
        let data = fetch_blocking().expect("live Codex rate limits");

        assert!((0..=100).contains(&data.remaining_percent));
        assert!(data.weekly_reset_at > chrono::Utc::now().timestamp());
        assert!(data.window_duration_mins > 0);
        assert!(data.reset_available_count >= 0);
        if data.reset_available_count > 0 {
            assert!(data.expiry_details_available);
            assert!(!data.reset_expiries.is_empty());
        }
    }
}
