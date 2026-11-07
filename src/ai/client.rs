//! AI client running on tokio. The UI only submits requests and reads results from a channel.

use std::os::windows::process::CommandExt as _;
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::runtime::Runtime;
use tokio::task::AbortHandle;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use super::prompt::AiPrompt;
use super::settings::{AiSettings, ProviderKind, DEFAULT_API_MODEL};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const API_BASE_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// This includes model thinking tokens, so it must not be too small even for short answers.
const MAX_TOKENS: u32 = 4096;
const EFFORT: &str = "low";
/// Models that accept `fallbacks: "default"`.
const FALLBACK_MODELS: [&str; 4] = [
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
];

/// The AI answer, or a message the user can read.
pub type AiResult = Result<String, String>;

/// Called when a result is ready, so the UI repaints.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
struct Destination {
    settings: AiSettings,
    api_key: Option<String>,
    api_base_url: String,
}

/// The runtime is created on the first request, so users who skip AI do not pay for it
/// with threads and memory.
pub struct AiClient {
    runtime: Option<Runtime>,
    results: Receiver<(u64, AiResult)>,
    sender: Sender<(u64, AiResult)>,
    running: Option<AbortHandle>,
    next_id: u64,
    api_base_url: String,
}

impl AiClient {
    pub fn new() -> Self {
        let (sender, results) = mpsc::channel();
        Self {
            runtime: None,
            results,
            sender,
            running: None,
            next_id: 0,
            api_base_url: API_BASE_URL.to_owned(),
        }
    }

    #[cfg(test)]
    pub fn with_api_base_url(base_url: String) -> Self {
        let mut client = Self::new();
        client.api_base_url = base_url;
        client
    }

    fn runtime(&mut self) -> Result<&Runtime, String> {
        if self.runtime.is_none() {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .map_err(|_| "Could not prepare the AI engine.".to_owned())?;
            self.runtime = Some(runtime);
        }
        self.runtime
            .as_ref()
            .ok_or_else(|| "Could not prepare the AI engine.".to_owned())
    }

    /// Submits a request and returns its id. Any earlier request is cancelled.
    pub fn submit(
        &mut self,
        prompt: AiPrompt,
        settings: AiSettings,
        api_key: Option<String>,
        notify: Notify,
    ) -> Result<u64, String> {
        self.cancel();
        self.next_id += 1;
        let id = self.next_id;
        let sender = self.sender.clone();
        let destination = Destination {
            settings,
            api_key,
            api_base_url: self.api_base_url.clone(),
        };
        let task = self.runtime()?.spawn(async move {
            let result = tokio::time::timeout(REQUEST_TIMEOUT, ask(prompt, destination))
                .await
                .unwrap_or_else(|_| Err("The AI took too long to respond. Try again.".to_owned()));
            let _ = sender.send((id, result));
            notify();
        });
        self.running = Some(task.abort_handle());
        Ok(id)
    }

    /// Cancels the running request. The Claude Code process stops with it.
    pub fn cancel(&mut self) {
        if let Some(task) = self.running.take() {
            task.abort();
        }
    }

    pub fn try_result(&self) -> Option<(u64, AiResult)> {
        self.results.try_recv().ok()
    }
}

impl Drop for AiClient {
    fn drop(&mut self) {
        self.cancel();
        // A runtime waits for its tasks when dropped by default, which would block the UI.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

async fn ask(prompt: AiPrompt, destination: Destination) -> AiResult {
    match destination.settings.provider {
        ProviderKind::ClaudeCode => ask_claude_code(&prompt, &destination.settings.model).await,
        ProviderKind::ApiKey => ask_api(&prompt, &destination).await,
    }
}

/// `claude -p` arguments that lock the session down: no tools, no folder or user configuration,
/// no MCP, and nothing saved. The AI can only answer with text.
fn claude_code_args(system: &str, model: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "json",
        "--tools",
        "",
        "--setting-sources",
        "",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--disable-slash-commands",
        "--permission-mode",
        "dontAsk",
        "--effort",
        EFFORT,
        "--system-prompt",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    args.push(system.to_owned());
    if !model.is_empty() {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    args
}

async fn ask_claude_code(prompt: &AiPrompt, model: &str) -> AiResult {
    // A dedicated empty folder, so no project files can be read.
    let workspace = std::env::temp_dir().join("ctermin-ai");
    tokio::fs::create_dir_all(&workspace)
        .await
        .map_err(|_| "Could not create the AI working folder.".to_owned())?;

    let mut command = Command::new("claude");
    command
        .args(claude_code_args(&prompt.system, model))
        .current_dir(&workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command.as_std_mut().creation_flags(CREATE_NO_WINDOW);

    let mut child = command.spawn().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            "Claude Code was not found. Install it, or switch provider in AI Settings.".to_owned()
        } else {
            "Could not start Claude Code.".to_owned()
        }
    })?;

    // Sent through stdin, not arguments, so it stays out of the process list.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.user.as_bytes())
            .await
            .map_err(|_| "Could not send the request to Claude Code.".to_owned())?;
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|_| "Claude Code stopped unexpectedly.".to_owned())?;
    parse_claude_code_output(&output.stdout)
}

#[derive(Deserialize)]
struct ClaudeCodeResult {
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    result: String,
}

fn parse_claude_code_output(stdout: &[u8]) -> AiResult {
    let parsed: ClaudeCodeResult = serde_json::from_slice(stdout)
        .map_err(|_| "Could not read the Claude Code response.".to_owned())?;
    if parsed.is_error {
        let detail = parsed.result.trim();
        return Err(if detail.is_empty() {
            "Claude Code failed to respond.".to_owned()
        } else {
            format!("Claude Code: {detail}")
        });
    }
    if parsed.result.trim().is_empty() {
        return Err("Claude Code returned no answer.".to_owned());
    }
    Ok(parsed.result)
}

fn api_body(prompt: &AiPrompt, model: &str) -> (Value, bool) {
    let mut body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "system": prompt.system,
        "messages": [{"role": "user", "content": prompt.user}],
        "output_config": {"effort": EFFORT},
    });
    let fallback = FALLBACK_MODELS.contains(&model);
    if fallback {
        body["fallbacks"] = json!("default");
    }
    (body, fallback)
}

async fn ask_api(prompt: &AiPrompt, destination: &Destination) -> AiResult {
    let Some(api_key) = &destination.api_key else {
        return Err("No API key set. Open AI Settings.".to_owned());
    };
    let model = match destination.settings.model.as_str() {
        "" => DEFAULT_API_MODEL,
        model => model,
    };
    let (body, fallback) = api_body(prompt, model);

    let mut request = reqwest::Client::new()
        .post(format!("{}/v1/messages", destination.api_base_url))
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .json(&body);
    if fallback {
        request = request.header("anthropic-beta", FALLBACK_BETA);
    }

    let response = request
        .send()
        .await
        .map_err(|_| "Could not connect to the Claude API.".to_owned())?;
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|_| "The Claude API response was cut off.".to_owned())?;
    parse_api_response(status, &text)
}

fn parse_api_response(status: u16, body: &str) -> AiResult {
    if status != 200 {
        return Err(api_error_message(status, body));
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|_| "Could not read the Claude API response.".to_owned())?;
    if value["stop_reason"] == "refusal" {
        return Err("Claude declined this request.".to_owned());
    }
    let text: String = value["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect();
    if text.trim().is_empty() {
        return Err("Claude returned no answer.".to_owned());
    }
    if value["stop_reason"] == "max_tokens" {
        return Err("Claude's answer was truncated.".to_owned());
    }
    Ok(text)
}

fn api_error_message(status: u16, body: &str) -> String {
    match status {
        401 => "API key rejected. Check it in AI Settings.".to_owned(),
        403 => "This API key is not allowed to make this request.".to_owned(),
        429 => "API rate limit reached. Try again shortly.".to_owned(),
        500..=599 => "The Claude service is having problems. Try again.".to_owned(),
        _ => {
            let detail = serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|value| value["error"]["message"].as_str().map(str::to_owned));
            match detail {
                Some(detail) => format!("Claude API: {detail}"),
                None => format!("Claude API request failed (code {status})."),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Instant;

    use super::*;

    const SECRET: &str = "secret-test-key";

    fn prompt() -> AiPrompt {
        AiPrompt {
            system: "system".to_owned(),
            user: "question".to_owned(),
        }
    }

    /// A tiny HTTP server: answers one request and returns the request text.
    fn mock_server(
        status: &'static str,
        body: &'static str,
    ) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("port available");
        let address = listener.local_addr().expect("has an address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("has a connection");
            let mut received = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = stream.read(&mut buffer).expect("readable");
                received.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&received);
                if let Some((head, body_part)) = text.split_once("\r\n\r\n") {
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if body_part.len() >= length {
                        break;
                    }
                }
            }
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).expect("writable");
            String::from_utf8_lossy(&received).into_owned()
        });
        (format!("http://{address}"), handle)
    }

    fn wait_result(client: &AiClient) -> AiResult {
        let start = Instant::now();
        loop {
            if let Some((_, result)) = client.try_result() {
                return result;
            }
            assert!(start.elapsed() < Duration::from_secs(20), "timed out");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn submit(client: &mut AiClient, api_key: Option<String>) {
        let settings = AiSettings {
            provider: ProviderKind::ApiKey,
            model: String::new(),
        };
        client
            .submit(prompt(), settings, api_key, Arc::new(|| {}))
            .expect("terkirim");
    }

    #[test]
    fn api_request_uses_the_right_format() {
        let answer = r#"{"content":[{"type":"text","text":"df -h"}],"stop_reason":"end_turn"}"#;
        let (url, server) = mock_server("200 OK", answer);
        let mut client = AiClient::with_api_base_url(url);
        submit(&mut client, Some(SECRET.to_owned()));

        assert_eq!(wait_result(&client).as_deref(), Ok("df -h"));

        let request = server.join().expect("server finished");
        let lower = request.to_lowercase();
        assert!(request.starts_with("POST /v1/messages "));
        assert!(lower.contains(&format!("x-api-key: {SECRET}")));
        assert!(lower.contains("anthropic-version: 2026-10-21"));
        assert!(lower.contains("anthropic-beta: server-side-fallback-2026-07-01"));
        let (_, body) = request.split_once("\r\n\r\n").expect("has a body");
        assert!(
            !body.contains(SECRET),
            "the API key must only be in the header"
        );
        assert!(!body.contains("temperature"));
        assert!(body.contains(DEFAULT_API_MODEL));
    }

    #[test]
    fn missing_api_key_sends_nothing() {
        let mut client = AiClient::with_api_base_url("http://127.0.0.1:1".to_owned());
        submit(&mut client, None);
        assert!(wait_result(&client).unwrap_err().contains("API key"));
    }

    #[test]
    fn connection_failure_gives_a_clear_message() {
        let mut client = AiClient::with_api_base_url("http://127.0.0.1:1".to_owned());
        submit(&mut client, Some(SECRET.to_owned()));
        let message = wait_result(&client).unwrap_err();
        assert!(message.contains("connect"));
        assert!(!message.contains(SECRET));
    }

    #[test]
    fn api_errors_do_not_leak_the_key() {
        let (url, server) = mock_server("401 Unauthorized", r#"{"error":{"message":"x"}}"#);
        let mut client = AiClient::with_api_base_url(url);
        submit(&mut client, Some(SECRET.to_owned()));
        let message = wait_result(&client).unwrap_err();
        server.join().expect("server finished");
        assert!(message.contains("rejected"));
        assert!(!message.contains(SECRET));
    }

    #[test]
    fn fallback_is_only_for_supported_models() {
        let (body, fallback) = api_body(&prompt(), "claude-haiku-5-5");
        assert!(!fallback);
        assert!(body.get("fallbacks").is_none());

        let (body, fallback) = api_body(&prompt(), "claude-opus-5-5");
        assert!(fallback);
        assert_eq!(body["fallbacks"], "default");
    }

    #[test]
    fn api_answers_are_parsed_correctly() {
        let thinking_then_text = r#"{"content":[{"type":"thinking","thinking":""},
            {"type":"text","text":"df"},{"type":"text","text":" -h"}],"stop_reason":"end_turn"}"#;
        assert_eq!(
            parse_api_response(200, thinking_then_text).as_deref(),
            Ok("df -h")
        );
        assert!(
            parse_api_response(200, r#"{"content":[],"stop_reason":"refusal"}"#)
                .unwrap_err()
                .contains("declined")
        );
        assert!(parse_api_response(
            200,
            r#"{"content":[{"type":"text","text":"df"}],"stop_reason":"max_tokens"}"#
        )
        .is_err());
        assert!(parse_api_response(200, "not json").is_err());
        assert!(parse_api_response(429, "")
            .unwrap_err()
            .contains("rate limit"));
        assert!(parse_api_response(529, "")
            .unwrap_err()
            .contains("problems"));
        assert_eq!(
            parse_api_response(400, r#"{"error":{"message":"wrong model"}}"#)
                .err()
                .as_deref(),
            Some("Claude API: wrong model")
        );
    }

    #[test]
    fn claude_code_output_is_parsed() {
        let ok = br#"{"is_error":false,"result":"df -h","stop_reason":"end_turn"}"#;
        assert_eq!(parse_claude_code_output(ok).as_deref(), Ok("df -h"));

        let failed = br#"{"is_error":true,"result":"Not logged in"}"#;
        assert_eq!(
            parse_claude_code_output(failed).err().as_deref(),
            Some("Claude Code: Not logged in")
        );
        assert!(parse_claude_code_output(b"").is_err());
        assert!(parse_claude_code_output(br#"{"is_error":false,"result":" "}"#).is_err());
    }

    #[test]
    fn claude_code_session_is_locked_down() {
        let args = claude_code_args("system", "");
        let position = |flag: &str| {
            args.iter()
                .position(|arg| arg == flag)
                .expect("flag exists")
        };
        assert_eq!(args[position("--tools") + 1], "", "all tools disabled");
        assert_eq!(args[position("--setting-sources") + 1], "");
        assert!(args.contains(&"--strict-mcp-config".to_owned()));
        assert!(args.contains(&"--no-session-persistence".to_owned()));
        assert_eq!(args[position("--permission-mode") + 1], "dontAsk");
        assert!(!args.contains(&"--model".to_owned()));

        let args = claude_code_args("system", "sonnet");
        assert!(args.contains(&"sonnet".to_owned()));
    }

    #[test]
    fn cancelling_produces_no_answer() {
        let mut client = AiClient::with_api_base_url("http://10.255.255.1:9".to_owned());
        submit(&mut client, Some(SECRET.to_owned()));
        client.cancel();
        thread::sleep(Duration::from_millis(100));
        assert!(client.try_result().is_none());
    }
}
