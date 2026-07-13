//! `ai-memory auth` — manage upstream LLM provider credentials.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ai_memory_llm::{
    CODEX_CLIENT_ID, CopilotToken, DEVIN_OAUTH_CALLBACK_PATH, DEVIN_OAUTH_CALLBACK_PORT,
    DeviceAuthorizationResponse, DevinToken, GITHUB_ACCESS_TOKEN_URL, GITHUB_COPILOT_CLIENT_ID,
    GITHUB_DEVICE_CODE_URL, OIDC_DEFAULT_SCOPE, OPENAI_OAUTH_TOKEN_URL, OidcDiscovery, OidcToken,
    OidcTokenResponse, OpenAiOAuthToken, OpenAiOAuthTokenResponse, PollOutcome,
    XAI_OAUTH_REDIRECT_PATH, XAI_OAUTH_REDIRECT_PORT, XaiOAuthToken, build_devin_authorize_url,
    build_xai_authorize_url, discover, discover_xai, exchange_devin_token, exchange_xai_code,
    generate_pkce, poll_token_once, request_device_code,
};
use anyhow::{Context, Result, anyhow, bail};
use secrecy::ExposeSecret as _;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use uuid::Uuid;

use crate::cli::{AuthArgs, AuthCommand, AuthProviderChoice};
use crate::config::Config;

const DEVICE_USER_CODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
const DEVICE_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
const DEVICE_BROWSER_URL: &str = "https://auth.openai.com/codex/device";
const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const POLLING_SAFETY_MARGIN_SECS: u64 = 3;

/// Run the `auth` subcommand.
///
/// # Errors
/// Returns an error if token storage or provider auth requests fail.
pub async fn run(config: &Config, args: AuthArgs) -> Result<()> {
    match args.command {
        AuthCommand::Login(args) => match args.provider {
            AuthProviderChoice::OpenaiOauth => {
                if args.github_token.is_some() || args.client_id.is_some() || args.issuer.is_some()
                {
                    bail!(
                        "--github-token / --client-id / --issuer do not apply to `auth login openai-oauth`"
                    );
                }
                login_openai_oauth(config, args.timeout_secs).await
            }
            AuthProviderChoice::Copilot => {
                if args.issuer.is_some() {
                    bail!("--issuer applies only to `auth login oidc-device`");
                }
                login_copilot(config, args.timeout_secs, args.github_token, args.client_id).await
            }
            AuthProviderChoice::OidcDevice => {
                if args.github_token.is_some() {
                    bail!("--github-token applies only to `auth login copilot`");
                }
                let issuer = args
                    .issuer
                    .context("--issuer is required for `auth login oidc-device`")?;
                let client_id = args
                    .client_id
                    .context("--client-id is required for `auth login oidc-device`")?;
                login_oidc_device(config, &issuer, &client_id, args.timeout_secs).await
            }
            AuthProviderChoice::XaiOauth => {
                if args.github_token.is_some() || args.client_id.is_some() || args.issuer.is_some()
                {
                    bail!(
                        "--github-token / --client-id / --issuer do not apply to `auth login xai-oauth`"
                    );
                }
                login_xai_oauth(config, args.timeout_secs).await
            }
            AuthProviderChoice::Devin => {
                if args.github_token.is_some() || args.client_id.is_some() || args.issuer.is_some()
                {
                    bail!(
                        "--github-token / --client-id / --issuer do not apply to `auth login devin`"
                    );
                }
                login_devin(config, args.timeout_secs).await
            }
        },
        AuthCommand::Logout(args) => match args.provider {
            AuthProviderChoice::OpenaiOauth => logout_openai_oauth(config),
            AuthProviderChoice::Copilot => logout_copilot(config),
            AuthProviderChoice::OidcDevice => logout_oidc_device(config),
            AuthProviderChoice::XaiOauth => logout_xai_oauth(config),
            AuthProviderChoice::Devin => logout_devin(config),
        },
        AuthCommand::Status(_) => status(config),
    }
}

async fn login_oidc_device(
    config: &Config,
    issuer: &str,
    client_id: &str,
    timeout_secs: u64,
) -> Result<()> {
    let client = auth_http_client()?;
    let discovery = discover(&client, issuer).await?;
    let device = request_device_code(&client, &discovery, client_id, OIDC_DEFAULT_SCOPE).await?;

    match device.verification_uri_complete.as_deref() {
        Some(complete) => println!("Open this URL: {complete}"),
        None => {
            println!("Open this URL: {}", device.verification_uri);
            println!("Enter code: {}", device.user_code);
        }
    }
    println!("Waiting for authorization...");

    let token_response = poll_oidc_device(
        &client,
        &discovery,
        client_id,
        &device,
        Duration::from_secs(timeout_secs),
    )
    .await?;
    let token = OidcToken::from_token_response(
        &token_response,
        issuer,
        client_id,
        &discovery.token_endpoint,
        None,
    )?;
    let path = config.oidc_device_token_path();
    token.save(&path).map_err(anyhow::Error::from)?;

    println!("oidc-device: logged in");
    println!("issuer: {issuer}");
    println!("token file: {}", path.display());
    Ok(())
}

async fn poll_oidc_device(
    client: &reqwest::Client,
    discovery: &OidcDiscovery,
    client_id: &str,
    device: &DeviceAuthorizationResponse,
    timeout: Duration,
) -> Result<OidcTokenResponse> {
    let started = Instant::now();
    let mut interval = Duration::from_secs(
        device
            .interval
            .unwrap_or(5)
            .max(1)
            .saturating_add(POLLING_SAFETY_MARGIN_SECS),
    );
    let device_timeout = Duration::from_secs(device.expires_in.unwrap_or(600).max(1));
    let timeout = timeout.min(device_timeout);
    loop {
        if started.elapsed() >= timeout {
            bail!("timed out waiting for oidc-device authorization");
        }
        match poll_token_once(client, discovery, client_id, &device.device_code).await? {
            PollOutcome::Token(token) => return Ok(*token),
            PollOutcome::Pending => {}
            PollOutcome::SlowDown => interval = interval.saturating_add(Duration::from_secs(5)),
            PollOutcome::Denied => bail!("oidc-device authorization denied"),
            PollOutcome::Expired => bail!("oidc-device code expired before authorization"),
            PollOutcome::Other(error) => bail!("oidc-device authorization failed: {error}"),
        }
        sleep(interval).await;
    }
}

fn logout_oidc_device(config: &Config) -> Result<()> {
    let path = config.oidc_device_token_path();
    OidcToken::remove(&path).map_err(anyhow::Error::from)?;
    println!("oidc-device: logged out");
    println!("token file: {}", path.display());
    Ok(())
}

async fn login_openai_oauth(config: &Config, timeout_secs: u64) -> Result<()> {
    let client = auth_http_client()?;
    let device = start_device_authorization(&client).await?;
    println!("Open this URL: {DEVICE_BROWSER_URL}");
    println!("Enter code: {}", device.user_code);
    println!("Waiting for authorization...");

    let code =
        poll_device_authorization(&client, &device, Duration::from_secs(timeout_secs)).await?;
    let tokens = exchange_authorization_code(&client, code).await?;
    let refresh = tokens.refresh_token.clone().ok_or_else(|| {
        anyhow::anyhow!("openai-oauth token response did not include refresh_token")
    })?;
    let token = OpenAiOAuthToken::from_token_response(
        tokens.access_token,
        refresh,
        tokens.expires_in.unwrap_or(3600),
        tokens.id_token.as_deref(),
        None,
    );
    let path = config.openai_oauth_token_path();
    token.save(&path).map_err(anyhow::Error::from)?;

    println!("openai-oauth: logged in");
    if let Some(account_id) = token.account_id.as_deref() {
        println!("account: {account_id}");
    }
    println!("token file: {}", path.display());
    Ok(())
}

fn logout_openai_oauth(config: &Config) -> Result<()> {
    let path = config.openai_oauth_token_path();
    OpenAiOAuthToken::remove(&path).map_err(anyhow::Error::from)?;
    println!("openai-oauth: logged out");
    println!("token file: {}", path.display());
    Ok(())
}

async fn login_copilot(
    config: &Config,
    timeout_secs: u64,
    github_token_arg: Option<String>,
    client_id_arg: Option<String>,
) -> Result<()> {
    let github_token = match github_token_arg.filter(|s| !s.trim().is_empty()) {
        Some(token) => token,
        None => match config.copilot_github_token() {
            Some(token) => token.expose_secret().to_string(),
            None => {
                let client_id = client_id_arg
                    .as_deref()
                    .or_else(|| config.copilot_client_id())
                    .unwrap_or(GITHUB_COPILOT_CLIENT_ID);
                run_copilot_device_flow(client_id, timeout_secs).await?
            }
        },
    };

    let token = CopilotToken::from_github_token(github_token, None);
    let path = config.copilot_token_path();
    token.save(&path).map_err(anyhow::Error::from)?;
    println!("copilot: logged in");
    println!("token file: {}", path.display());
    Ok(())
}

fn logout_copilot(config: &Config) -> Result<()> {
    let path = config.copilot_token_path();
    CopilotToken::remove(&path).map_err(anyhow::Error::from)?;
    println!("copilot: logged out");
    println!("token file: {}", path.display());
    Ok(())
}

fn status(config: &Config) -> Result<()> {
    let openai_path = config.openai_oauth_token_path();
    match OpenAiOAuthToken::load(&openai_path).map_err(anyhow::Error::from)? {
        Some(token) => {
            println!("openai-oauth: logged in");
            if let Some(account_id) = token.account_id.as_deref() {
                println!("account: {account_id}");
            }
            println!("expires in: {}", format_duration_until(token.expires_at_ms));
            println!("token file: {}", openai_path.display());
        }
        None => {
            println!("openai-oauth: not logged in");
            println!("token file: {}", openai_path.display());
        }
    }

    let copilot_path = config.copilot_token_path();
    match CopilotToken::load(&copilot_path).map_err(anyhow::Error::from)? {
        Some(token) => {
            let has_refreshable_github = token.has_refreshable_github_token();
            let has_valid_cached = token.has_valid_cached_copilot_token();
            if has_refreshable_github {
                println!("copilot: logged in");
            } else if has_valid_cached {
                println!("copilot: cached token valid (no GitHub token stored for refresh)");
            } else {
                println!(
                    "copilot: not logged in (cached token expired and no refreshable GitHub token stored)"
                );
            }
            if let Some(expires_at_ms) = token.github_expires_at_ms {
                println!(
                    "github token expires in: {}",
                    format_duration_until(expires_at_ms)
                );
            }
            if let Some(expires_at_ms) = token.copilot_expires_at_ms {
                println!(
                    "cached copilot token expires in: {}",
                    format_duration_until(expires_at_ms)
                );
            }
            if let Some(api_base_url) = token.api_base_url.as_deref() {
                println!("api base: {api_base_url}");
            }
            println!("token file: {}", copilot_path.display());
        }
        None => {
            println!("copilot: not logged in");
            println!("token file: {}", copilot_path.display());
        }
    }

    let oidc_path = config.oidc_device_token_path();
    match OidcToken::load(&oidc_path).map_err(anyhow::Error::from)? {
        Some(token) => {
            println!("oidc-device: logged in");
            println!("issuer: {}", token.issuer);
            println!("expires in: {}", format_duration_until(token.expires_at_ms));
            println!("token file: {}", oidc_path.display());
        }
        None => {
            println!("oidc-device: not logged in");
            println!("token file: {}", oidc_path.display());
        }
    }

    let xai_path = config.auth_token_path();
    match XaiOAuthToken::load(&xai_path).map_err(anyhow::Error::from)? {
        Some(token) => {
            println!("xai-oauth: logged in");
            println!("expires in: {}", format_duration_until(token.expires_at_ms));
            println!("token file: {}", xai_path.display());
        }
        None => {
            println!("xai-oauth: not logged in");
            println!("token file: {}", xai_path.display());
        }
    }

    match DevinToken::load(&xai_path).map_err(anyhow::Error::from)? {
        Some(_) => {
            println!("devin: logged in");
            println!("token file: {}", xai_path.display());
        }
        None => {
            println!("devin: not logged in");
            println!("token file: {}", xai_path.display());
        }
    }
    Ok(())
}

async fn login_xai_oauth(config: &Config, timeout_secs: u64) -> Result<()> {
    let client = auth_http_client()?;
    let discovery = discover_xai(&client).await.map_err(anyhow::Error::from)?;
    let (verifier, challenge) = generate_pkce();
    let state = Uuid::new_v4().simple().to_string();
    let nonce = Uuid::new_v4().simple().to_string();
    let redirect_uri =
        format!("http://127.0.0.1:{XAI_OAUTH_REDIRECT_PORT}{XAI_OAUTH_REDIRECT_PATH}");
    let auth_url = build_xai_authorize_url(&discovery, &redirect_uri, &challenge, &state, &nonce);
    println!("Open this URL: {auth_url}");
    open_browser(&auth_url);
    println!("Waiting for authorization on {redirect_uri} ...");
    let code = wait_for_oauth_code(
        XAI_OAUTH_REDIRECT_PORT,
        XAI_OAUTH_REDIRECT_PATH,
        &state,
        Duration::from_secs(timeout_secs),
    )
    .await?;
    let token = exchange_xai_code(&client, &discovery, &code, &verifier, &redirect_uri)
        .await
        .map_err(anyhow::Error::from)?;
    let path = config.auth_token_path();
    token.save(&path).map_err(anyhow::Error::from)?;
    println!("xai-oauth: logged in");
    println!("token file: {}", path.display());
    Ok(())
}

fn logout_xai_oauth(config: &Config) -> Result<()> {
    let path = config.auth_token_path();
    XaiOAuthToken::remove(&path).map_err(anyhow::Error::from)?;
    println!("xai-oauth: logged out");
    println!("token file: {}", path.display());
    Ok(())
}

async fn login_devin(config: &Config, timeout_secs: u64) -> Result<()> {
    let client = auth_http_client()?;
    let (verifier, challenge) = generate_pkce();
    let state = Uuid::new_v4().simple().to_string();
    let redirect_uri =
        format!("http://127.0.0.1:{DEVIN_OAUTH_CALLBACK_PORT}{DEVIN_OAUTH_CALLBACK_PATH}");
    let auth_url = build_devin_authorize_url(&redirect_uri, &state, &challenge);
    println!("Open this URL: {auth_url}");
    open_browser(&auth_url);
    println!("Waiting for authorization on {redirect_uri} ...");
    let code = wait_for_oauth_code(
        DEVIN_OAUTH_CALLBACK_PORT,
        DEVIN_OAUTH_CALLBACK_PATH,
        &state,
        Duration::from_secs(timeout_secs),
    )
    .await?;
    let token = exchange_devin_token(&client, &code, &verifier)
        .await
        .map_err(anyhow::Error::from)?;
    let path = config.auth_token_path();
    token.save(&path).map_err(anyhow::Error::from)?;
    println!("devin: logged in");
    println!("token file: {}", path.display());
    Ok(())
}

fn logout_devin(config: &Config) -> Result<()> {
    let path = config.auth_token_path();
    DevinToken::remove(&path).map_err(anyhow::Error::from)?;
    println!("devin: logged out");
    println!("token file: {}", path.display());
    Ok(())
}

fn open_browser(url: &str) {
    let _ = std::process::Command::new(if cfg!(windows) {
        "cmd"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    })
    .args(if cfg!(windows) {
        vec!["/C", "start", "", url]
    } else {
        vec![url]
    })
    .spawn();
}

/// Minimal loopback OAuth callback: bind `127.0.0.1:port`, wait for GET path
/// with `?code=&state=`, validate state, return code.
async fn wait_for_oauth_code(
    port: u16,
    path: &str,
    expected_state: &str,
    timeout: Duration,
) -> Result<String> {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("bind 127.0.0.1:{port} for OAuth callback"))?;
    listener
        .set_nonblocking(true)
        .context("set oauth callback listener nonblocking")?;
    let expected_state = expected_state.to_string();
    let path = path.to_string();
    let started = Instant::now();

    loop {
        if started.elapsed() >= timeout {
            bail!("timed out waiting for OAuth callback on port {port}");
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let first = req.lines().next().unwrap_or("");
                // GET /callback?code=...&state=... HTTP/1.1
                let target = first.split_whitespace().nth(1).unwrap_or("");
                let (req_path, query) = target.split_once('?').unwrap_or((target, ""));
                let ok_path = req_path == path || req_path.ends_with(path.as_str());
                let mut code = None;
                let mut state = None;
                for pair in query.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        match k {
                            "code" => code = Some(urlencoding_decode(v)),
                            "state" => state = Some(urlencoding_decode(v)),
                            _ => {}
                        }
                    }
                }
                let success =
                    ok_path && code.is_some() && state.as_deref() == Some(expected_state.as_str());
                let body = if success {
                    "<html><body><h1>Login successful</h1><p>You can close this tab.</p></body></html>"
                } else {
                    "<html><body><h1>Login failed</h1><p>Invalid callback.</p></body></html>"
                };
                let status = if success { "200 OK" } else { "400 Bad Request" };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                if let (Some(code), Some(state)) = (code, state)
                    && state == expected_state
                {
                    return Ok(code);
                }
                bail!("OAuth callback received invalid state or missing code");
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                sleep(Duration::from_millis(100)).await;
            }
            Err(e) => return Err(e).context("accept oauth callback"),
        }
    }
}

fn urlencoding_decode(s: &str) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v as char);
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            b => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    out
}

async fn run_copilot_device_flow(client_id: &str, timeout_secs: u64) -> Result<String> {
    let client = auth_http_client()?;
    let device = start_github_device_authorization(&client, client_id).await?;
    println!("Open this URL: {}", device.verification_uri);
    println!("Enter code: {}", device.user_code);
    println!("Waiting for authorization...");
    poll_github_device_authorization(
        &client,
        client_id,
        &device,
        Duration::from_secs(timeout_secs),
    )
    .await
}

fn auth_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(format!("ai-memory/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building HTTP client")
}

async fn start_device_authorization(client: &reqwest::Client) -> Result<DeviceAuthorization> {
    let resp = client
        .post(DEVICE_USER_CODE_URL)
        .json(&DeviceAuthorizationRequest {
            client_id: CODEX_CLIENT_ID,
        })
        .send()
        .await
        .context("starting openai-oauth device authorization")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("openai-oauth device authorization failed ({status}): {body}");
    }
    resp.json::<DeviceAuthorization>()
        .await
        .context("parsing openai-oauth device authorization response")
}

async fn poll_device_authorization(
    client: &reqwest::Client,
    device: &DeviceAuthorization,
    timeout: Duration,
) -> Result<DeviceAuthorizationCode> {
    let started = Instant::now();
    let interval = Duration::from_secs(
        device
            .interval
            .parse::<u64>()
            .unwrap_or(5)
            .max(1)
            .saturating_add(POLLING_SAFETY_MARGIN_SECS),
    );
    loop {
        if started.elapsed() >= timeout {
            bail!("timed out waiting for openai-oauth authorization");
        }
        let resp = client
            .post(DEVICE_TOKEN_URL)
            .json(&DeviceTokenRequest {
                device_auth_id: &device.device_auth_id,
                user_code: &device.user_code,
            })
            .send()
            .await
            .context("polling openai-oauth device authorization")?;
        if resp.status().is_success() {
            return resp
                .json::<DeviceAuthorizationCode>()
                .await
                .context("parsing openai-oauth authorization code");
        }
        if resp.status() != reqwest::StatusCode::FORBIDDEN
            && resp.status() != reqwest::StatusCode::NOT_FOUND
        {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("openai-oauth authorization polling failed ({status}): {body}");
        }
        sleep(interval).await;
    }
}

async fn exchange_authorization_code(
    client: &reqwest::Client,
    code: DeviceAuthorizationCode,
) -> Result<OpenAiOAuthTokenResponse> {
    let resp = client
        .post(OPENAI_OAUTH_TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.authorization_code.as_str()),
            ("redirect_uri", DEVICE_REDIRECT_URI),
            ("client_id", CODEX_CLIENT_ID),
            ("code_verifier", code.code_verifier.as_str()),
        ])
        .send()
        .await
        .context("exchanging openai-oauth authorization code")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("openai-oauth token exchange failed ({status}): {body}");
    }
    resp.json::<OpenAiOAuthTokenResponse>()
        .await
        .context("parsing openai-oauth token response")
}

async fn start_github_device_authorization(
    client: &reqwest::Client,
    client_id: &str,
) -> Result<GitHubDeviceAuthorization> {
    let resp = client
        .post(GITHUB_DEVICE_CODE_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[("client_id", client_id), ("scope", "read:user")])
        .send()
        .await
        .context("starting copilot GitHub device authorization")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("copilot GitHub device authorization failed ({status}): {body}");
    }
    resp.json::<GitHubDeviceAuthorization>()
        .await
        .context("parsing copilot GitHub device authorization response")
}

async fn poll_github_device_authorization(
    client: &reqwest::Client,
    client_id: &str,
    device: &GitHubDeviceAuthorization,
    timeout: Duration,
) -> Result<String> {
    let started = Instant::now();
    let mut interval = Duration::from_secs(device.interval.unwrap_or(5).max(1));
    let device_timeout = Duration::from_secs(device.expires_in.max(1));
    let timeout = timeout.min(device_timeout);
    loop {
        if started.elapsed() >= timeout {
            bail!("timed out waiting for copilot GitHub authorization");
        }
        let resp = client
            .post(GITHUB_ACCESS_TOKEN_URL)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("client_id", client_id),
                ("device_code", device.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .context("polling copilot GitHub device authorization")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("copilot GitHub authorization polling failed ({status}): {body}");
        }
        let body = resp
            .json::<GitHubDeviceAccessToken>()
            .await
            .context("parsing copilot GitHub token response")?;
        if let Some(token) = body.access_token {
            return Ok(token);
        }
        match body.error.as_deref() {
            Some("authorization_pending") => {}
            Some("slow_down") => interval = interval.saturating_add(Duration::from_secs(5)),
            Some("expired_token") => bail!("copilot GitHub device code expired"),
            Some("access_denied") => bail!("copilot GitHub authorization denied"),
            Some(error) => {
                let description = body.error_description.unwrap_or_default();
                return Err(anyhow!(
                    "copilot GitHub authorization failed: {error} {description}"
                ));
            }
            None => bail!("copilot GitHub token response did not include access_token"),
        }
        sleep(interval).await;
    }
}

fn format_duration_until(expires_at_ms: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    if expires_at_ms <= now {
        return "expired".into();
    }
    let secs = (expires_at_ms - now) / 1000;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

#[derive(Debug, Serialize)]
struct DeviceAuthorizationRequest {
    client_id: &'static str,
}

#[derive(Debug, Deserialize)]
struct DeviceAuthorization {
    device_auth_id: String,
    user_code: String,
    interval: String,
}

#[derive(Debug, Serialize)]
struct DeviceTokenRequest<'a> {
    device_auth_id: &'a str,
    user_code: &'a str,
}

#[derive(Debug, Deserialize)]
struct DeviceAuthorizationCode {
    authorization_code: String,
    code_verifier: String,
}

#[derive(Debug, Deserialize)]
struct GitHubDeviceAuthorization {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GitHubDeviceAccessToken {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_status_does_not_print_exact_token_time() {
        assert_eq!(format_duration_until(0), "expired");
    }
}
