//! The HTTP client and its resource groups.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde_json::json;

use crate::error::{error_from_body, error_from_status, ApiErrorBody, Error, RateLimit, Result};
use crate::models::*;

pub const DEFAULT_BASE_URL: &str = "https://api.cabalspy.xyz/v1";
pub const DEFAULT_WS_URL: &str = "wss://stream.cabalspy.xyz";
pub const BATCH_MAX_MINTS: usize = 100;
/// The public demo key. Works as a normal key on REST and the websocket gateway,
/// limited to 20 requests per IP per UTC day with 15 minute delayed data and at
/// most 5 rows per list. See [`CabalSpy::demo`].
pub const DEMO_API_KEY: &str = "demo";
pub const BATCH_MAX_ADDRESSES: usize = 100;

const SDK_USER_AGENT: &str = concat!("cabalspy-rust/", env!("CARGO_PKG_VERSION"));

/// A list of query parameters, built as owned strings.
type Query = Vec<(String, String)>;

/// Adds a parameter when the value is present.
fn push_opt<T: ToString>(query: &mut Query, key: &str, value: Option<T>) {
    if let Some(value) = value {
        query.push((key.to_string(), value.to_string()));
    }
}

fn push<T: ToString>(query: &mut Query, key: &str, value: T) {
    query.push((key.to_string(), value.to_string()));
}

fn header_u64(headers: &reqwest::header::HeaderMap, name: &str) -> Option<u64> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|text| text.parse::<u64>().ok())
}

/// Cheap jitter without pulling in a random number generator.
fn jitter_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.subsec_nanos() / 1_000_000) as u64 % 250)
        .unwrap_or(0)
}

fn backoff(attempt: u32, last: Option<&Error>) -> Duration {
    if let Some(Error::RateLimited {
        retry_after: Some(seconds),
        ..
    }) = last
    {
        return Duration::from_secs((*seconds).min(60));
    }
    let base = 500u64.saturating_mul(1u64 << attempt.saturating_sub(1).min(4));
    Duration::from_millis(base.min(8_000) + jitter_ms())
}

/// Reads the top-level `demo` object, falling back to the `X-Demo-Remaining`
/// header for the remaining budget.
fn demo_info(raw: Option<serde_json::Value>, remaining: Option<u64>) -> Option<DemoInfo> {
    let parsed = raw.and_then(|value| serde_json::from_value::<DemoInfo>(value).ok());
    match (parsed, remaining) {
        (Some(mut info), remaining) => {
            info.remaining_today = info.remaining_today.or(remaining);
            Some(info)
        }
        (None, Some(remaining)) => Some(DemoInfo {
            remaining_today: Some(remaining),
            ..Default::default()
        }),
        (None, None) => None,
    }
}

/// Rejects a chain and wallet type combination the API does not have.
fn check_type(chain: Chain, wallet_type: WalletType) -> Result<()> {
    if chain.supports(wallet_type) {
        return Ok(());
    }
    let allowed: Vec<&str> = chain.wallet_types().iter().map(|t| t.as_str()).collect();
    Err(Error::InvalidRequest {
        message: format!(
            "{chain} has no {wallet_type} wallets; it supports: {}",
            allowed.join(", ")
        ),
    })
}

// ═══════════════════════════════════════════════════════════════════════════
//  CLIENT
// ═══════════════════════════════════════════════════════════════════════════

/// Builder for [`CabalSpy`].
#[derive(Debug, Clone, Default)]
pub struct CabalSpyBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    ws_url: Option<String>,
    timeout: Option<Duration>,
    max_retries: Option<u32>,
}

impl CabalSpyBuilder {
    /// The API key. Falls back to the `CABALSPY_API_KEY` environment variable.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Uses the public demo key, [`DEMO_API_KEY`], instead of your own.
    ///
    /// Meant for trying the API without signing up: 20 requests per IP per UTC
    /// day shared between REST and websocket, data delayed by 15 minutes, at most
    /// 5 rows per list. When the budget is spent requests fail with
    /// [`Error::DemoLimit`].
    pub fn demo(self) -> Self {
        self.api_key(DEMO_API_KEY)
    }

    /// REST base URL including `/v1`.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Websocket gateway URL.
    pub fn ws_url(mut self, url: impl Into<String>) -> Self {
        self.ws_url = Some(url.into());
        self
    }

    /// Per-request timeout. Default 30 seconds.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Retries on 429, 5xx and network errors. Default 2.
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = Some(retries);
        self
    }

    pub fn build(self) -> Result<CabalSpy> {
        let api_key = self
            .api_key
            .or_else(|| std::env::var("CABALSPY_API_KEY").ok())
            .ok_or(Error::MissingApiKey)?;

        let timeout = self.timeout.unwrap_or(Duration::from_secs(30));
        let http = reqwest::Client::builder().timeout(timeout).build()?;

        Ok(CabalSpy {
            http,
            api_key,
            base_url: self
                .base_url
                .or_else(|| std::env::var("CABALSPY_BASE_URL").ok())
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
                .trim_end_matches('/')
                .to_string(),
            ws_url: self
                .ws_url
                .or_else(|| std::env::var("CABALSPY_WS_URL").ok())
                .unwrap_or_else(|| DEFAULT_WS_URL.to_string())
                .trim_end_matches('/')
                .to_string(),
            max_retries: self.max_retries.unwrap_or(2),
        })
    }
}

/// Asynchronous client for the CabalSpy API.
///
/// ```no_run
/// # async fn run() -> Result<(), cabalspy::Error> {
/// use cabalspy::{CabalSpy, Chain, Period, WalletType};
///
/// let client = CabalSpy::from_env()?;
/// let wallet = client.wallets().lookup("As7HjL7...").await?;
/// println!("{:?}", wallet.data);
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct CabalSpy {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    ws_url: String,
    max_retries: u32,
}

impl CabalSpy {
    pub fn builder() -> CabalSpyBuilder {
        CabalSpyBuilder::default()
    }

    /// Builds a client with an explicit API key and all other defaults.
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        CabalSpyBuilder::default().api_key(api_key).build()
    }

    /// Builds a client on the public demo key. No signup needed.
    ///
    /// ```no_run
    /// # async fn run() -> Result<(), cabalspy::Error> {
    /// use cabalspy::{CabalSpy, Chain, WalletType};
    ///
    /// let client = CabalSpy::demo()?;
    /// let kols = client.wallets().list(Chain::Solana, WalletType::Kol, None, None).await?;
    /// println!("{:?}", kols.demo.and_then(|d| d.remaining_today));
    /// # Ok(()) }
    /// ```
    ///
    /// Limits: 20 requests per IP per UTC day, data delayed by 15 minutes, at
    /// most 5 rows per list. For realtime data get a free test key at
    /// <https://apidashboard.cabalspy.xyz/>, or pay per call with x402.
    pub fn demo() -> Result<Self> {
        CabalSpyBuilder::default().demo().build()
    }

    /// True when the client is using the public demo key.
    pub fn is_demo(&self) -> bool {
        self.api_key == DEMO_API_KEY
    }

    /// Builds a client, reading the key from `CABALSPY_API_KEY`.
    pub fn from_env() -> Result<Self> {
        CabalSpyBuilder::default().build()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    /// The gateway URL including the auth query parameter.
    pub fn websocket_url(&self) -> String {
        format!("{}/?apiKey={}", self.ws_url, self.api_key)
    }

    // ── resources ────────────────────────────────────────────────────────

    pub fn system(&self) -> System<'_> {
        System { client: self }
    }
    pub fn wallets(&self) -> Wallets<'_> {
        Wallets { client: self }
    }
    pub fn tokens(&self) -> Tokens<'_> {
        Tokens { client: self }
    }
    pub fn transactions(&self) -> Transactions<'_> {
        Transactions { client: self }
    }
    pub fn signals(&self) -> Signals<'_> {
        Signals { client: self }
    }
    pub fn analytics(&self) -> Analytics<'_> {
        Analytics { client: self }
    }
    pub fn bundle(&self) -> Bundle<'_> {
        Bundle { client: self }
    }

    // ── plumbing ─────────────────────────────────────────────────────────

    /// Any GET, deserialized into `T`. Use this for endpoints not yet wrapped.
    pub async fn get<T: DeserializeOwned>(&self, path: &str, query: Query) -> Result<Envelope<T>> {
        self.send(reqwest::Method::GET, path, query, None).await
    }

    /// Any POST, deserialized into `T`.
    pub async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<Envelope<T>> {
        self.send(reqwest::Method::POST, path, Vec::new(), Some(body))
            .await
    }

    async fn send<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        query: Query,
        body: Option<serde_json::Value>,
    ) -> Result<Envelope<T>> {
        let url = format!("{}{}", self.base_url, path);
        let mut last: Option<Error> = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(backoff(attempt, last.as_ref())).await;
            }

            let mut request = self
                .http
                .request(method.clone(), &url)
                .bearer_auth(&self.api_key)
                .header(reqwest::header::USER_AGENT, SDK_USER_AGENT)
                .header(reqwest::header::ACCEPT, "application/json")
                .query(&query);
            if let Some(ref payload) = body {
                request = request.json(payload);
            }

            let response = match request.send().await {
                Ok(response) => response,
                Err(err) => {
                    let error = Error::Transport(err);
                    if error.is_retryable() && attempt < self.max_retries {
                        last = Some(error);
                        continue;
                    }
                    return Err(error);
                }
            };

            let status = response.status().as_u16();
            let rate_limit = RateLimit {
                limit: header_u64(response.headers(), "x-ratelimit-limit"),
                remaining: header_u64(response.headers(), "x-ratelimit-remaining"),
                reset: header_u64(response.headers(), "x-ratelimit-reset"),
            };
            let retry_after = header_u64(response.headers(), "retry-after");

            let demo_remaining = header_u64(response.headers(), "x-demo-remaining");
            let text = response.text().await.unwrap_or_default();

            if status >= 400 {
                let error = error_from_body(
                    status,
                    &text,
                    format!("HTTP {status} on {method} {path}"),
                    rate_limit,
                    retry_after,
                );
                if error.is_retryable() && attempt < self.max_retries {
                    last = Some(error);
                    continue;
                }
                return Err(error);
            }

            let raw: RawEnvelope<T> = serde_json::from_str(&text).map_err(|err| {
                Error::Decode(format!("{method} {path}: {err}"))
            })?;

            return Ok(Envelope {
                data: raw.data,
                pagination: raw.pagination,
                meta: raw.meta,
                rate_limit,
                status,
                demo: demo_info(raw.demo, demo_remaining),
            });
        }

        Err(last.unwrap_or(Error::Decode(format!("request failed: {method} {path}"))))
    }

    /// For `/health` and `/meta`, which answer without the success/data envelope.
    async fn get_plain<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .http
            .get(&url)
            .header(reqwest::header::USER_AGENT, SDK_USER_AGENT)
            .send()
            .await?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        if status >= 400 {
            return Err(error_from_status(
                status,
                ApiErrorBody {
                    code: format!("http_{status}"),
                    message: format!("HTTP {status} on GET {path}"),
                    ..Default::default()
                },
                RateLimit::default(),
                None,
            ));
        }
        serde_json::from_str(&text).map_err(|err| Error::Decode(format!("GET {path}: {err}")))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  OPTIONAL PARAMETERS
// ═══════════════════════════════════════════════════════════════════════════

/// Optional parameters for [`Wallets::leaderboard`].
#[derive(Debug, Clone, Default)]
pub struct LeaderboardOpts {
    /// Defaults to `kol` server side.
    pub wallet_type: Option<WalletType>,
    /// Defaults to `1d` server side.
    pub period: Option<Period>,
    /// Omitting this returns every entry.
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

/// Optional parameters for the feed endpoints.
///
/// `seconds`, `minutes` and `hours` are alternatives; the server clamps anything
/// beyond 60 minutes for `timerange` and 24 hours for `count` and `volume`, and
/// reports the clamp in `warnings`.
#[derive(Debug, Clone, Default)]
pub struct FeedOpts {
    pub seconds: Option<u64>,
    pub minutes: Option<u64>,
    pub hours: Option<u64>,
    pub limit: Option<u32>,
    /// Restrict to one token.
    pub mint: Option<String>,
}

/// Optional parameters for [`Signals::list`].
///
/// Setting any of the gated filters switches the server into gated mode, which
/// applies an AND gate across wallet types.
#[derive(Debug, Clone, Default)]
pub struct SignalOpts {
    pub limit: Option<u32>,
    /// Minimum number of buying wallets for a cluster. Server default 3.
    pub min_wallets: Option<u32>,
    /// Minimum buy value in the chain's native currency.
    pub min_value: Option<f64>,
    /// Observation window in hours. Server default 1.
    pub hours: Option<u32>,
    /// Entry thresholds for kol as CSV, for example `"3,5,10"`.
    pub kol: Option<String>,
    pub smart: Option<String>,
    pub kol_min_buy: Option<f64>,
    pub kol_max_buy: Option<f64>,
    /// Exit thresholds as remaining holders, CSV in descending order.
    pub kol_exit: Option<String>,
    pub smart_min_buy: Option<f64>,
    pub smart_max_buy: Option<f64>,
    pub smart_exit: Option<String>,
    /// Comma separated wallet addresses.
    pub include_wallets: Option<String>,
    pub exclude_wallets: Option<String>,
    /// Between 0 and 100.
    pub min_win_rate: Option<f64>,
    /// Token age in hours.
    pub min_token_age: Option<f64>,
    pub max_token_age: Option<f64>,
}

impl SignalOpts {
    fn apply(self, query: &mut Query) {
        push_opt(query, "limit", self.limit);
        push_opt(query, "min_wallets", self.min_wallets);
        push_opt(query, "min_value", self.min_value);
        push_opt(query, "hours", self.hours);
        push_opt(query, "kol", self.kol);
        push_opt(query, "smart", self.smart);
        push_opt(query, "kol_min_buy", self.kol_min_buy);
        push_opt(query, "kol_max_buy", self.kol_max_buy);
        push_opt(query, "kol_exit", self.kol_exit);
        push_opt(query, "smart_min_buy", self.smart_min_buy);
        push_opt(query, "smart_max_buy", self.smart_max_buy);
        push_opt(query, "smart_exit", self.smart_exit);
        push_opt(query, "include_wallets", self.include_wallets);
        push_opt(query, "exclude_wallets", self.exclude_wallets);
        push_opt(query, "min_win_rate", self.min_win_rate);
        push_opt(query, "min_token_age", self.min_token_age);
        push_opt(query, "max_token_age", self.max_token_age);
    }
}

impl FeedOpts {
    fn apply(self, query: &mut Query) {
        push_opt(query, "seconds", self.seconds);
        push_opt(query, "minutes", self.minutes);
        push_opt(query, "hours", self.hours);
        push_opt(query, "limit", self.limit);
        push_opt(query, "mint", self.mint);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  RESOURCES
// ═══════════════════════════════════════════════════════════════════════════

/// `/v1/health` and `/v1/meta`.
#[derive(Debug)]
pub struct System<'a> {
    client: &'a CabalSpy,
}

impl System<'_> {
    /// Redis, MySQL and websocket status per chain and wallet type.
    pub async fn health(&self) -> Result<HealthResponse> {
        self.client.get_plain("/health").await
    }

    /// Available chains, wallet types, periods, limits and wallet counts.
    pub async fn meta(&self) -> Result<MetaResponse> {
        self.client.get_plain("/meta").await
    }
}

/// Wallet endpoints.
#[derive(Debug)]
pub struct Wallets<'a> {
    client: &'a CabalSpy,
}

impl Wallets<'_> {
    /// `GET /v1/wallets` — every tracked wallet for one chain and wallet type.
    ///
    /// Omitting `limit` returns every wallet.
    pub async fn list(
        &self,
        chain: Chain,
        wallet_type: WalletType,
        limit: Option<u32>,
        cursor: Option<String>,
    ) -> Result<Envelope<serde_json::Value>> {
        check_type(chain, wallet_type)?;
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "type", wallet_type);
        push_opt(&mut query, "limit", limit);
        push_opt(&mut query, "cursor", cursor);
        self.client.get("/wallets", query).await
    }

    /// `GET /v1/wallets/history` — trade history. Server default 500, maximum 1000.
    ///
    /// This endpoint nests its pagination inside `data` rather than putting it on
    /// the envelope, so `Envelope::pagination` is `None` here. Read
    /// `data["pagination"]["next_cursor"]` instead.
    pub async fn history(
        &self,
        chain: Chain,
        address: &str,
        limit: Option<u32>,
        cursor: Option<String>,
    ) -> Result<Envelope<serde_json::Value>> {
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "address", address);
        push_opt(&mut query, "limit", limit);
        push_opt(&mut query, "cursor", cursor);
        self.client.get("/wallets/history", query).await
    }

    /// `GET /v1/wallets/lookup` — searches an address across all chains and types.
    ///
    /// Takes no chain argument by design. For EVM chains the same address can be
    /// tracked on several of them, and the endpoint returns the first match in the
    /// server's registry order, which may not be the chain you meant.
    pub async fn lookup(&self, address: &str) -> Result<Envelope<serde_json::Value>> {
        let mut query = Query::new();
        push(&mut query, "address", address);
        self.client.get("/wallets/lookup", query).await
    }

    /// `GET /v1/wallets/leaderboard` — ranking for the given period.
    pub async fn leaderboard(
        &self,
        chain: Chain,
        opts: LeaderboardOpts,
    ) -> Result<Envelope<serde_json::Value>> {
        if let Some(wallet_type) = opts.wallet_type {
            check_type(chain, wallet_type)?;
        }
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push_opt(&mut query, "type", opts.wallet_type);
        push_opt(&mut query, "period", opts.period);
        push_opt(&mut query, "limit", opts.limit);
        push_opt(&mut query, "cursor", opts.cursor);
        self.client.get("/wallets/leaderboard", query).await
    }

    /// `GET /v1/wallets/tracker` — period stats and open positions for one wallet.
    pub async fn tracker(
        &self,
        chain: Chain,
        address: &str,
        period: Option<Period>,
    ) -> Result<Envelope<WalletTracker>> {
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "address", address);
        push_opt(&mut query, "period", period);
        self.client.get("/wallets/tracker", query).await
    }

    /// `GET /v1/wallets/holdings` — current onchain holdings, period independent.
    pub async fn holdings(
        &self,
        chain: Chain,
        address: &str,
    ) -> Result<Envelope<serde_json::Value>> {
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "address", address);
        self.client.get("/wallets/holdings", query).await
    }

    /// `GET /v1/wallet/pnl_calendar` — daily PNL calendar.
    ///
    /// This endpoint only exists under `/wallet/`, singular.
    pub async fn pnl_calendar(
        &self,
        chain: Chain,
        address: &str,
    ) -> Result<Envelope<serde_json::Value>> {
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "address", address);
        self.client.get("/wallet/pnl_calendar", query).await
    }

    /// `GET /v1/wallets/connections` — wallets whose traded tokens overlap, 30 days.
    pub async fn connections(
        &self,
        chain: Chain,
        address: &str,
        limit: Option<u32>,
    ) -> Result<Envelope<serde_json::Value>> {
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "address", address);
        push_opt(&mut query, "limit", limit);
        self.client.get("/wallets/connections", query).await
    }

    /// `POST /v1/wallets/batch` — up to 100 addresses in one request.
    pub async fn batch(
        &self,
        chain: Chain,
        wallet_type: WalletType,
        addresses: &[String],
        fields: Option<&[String]>,
        period: Option<Period>,
    ) -> Result<Envelope<serde_json::Value>> {
        check_type(chain, wallet_type)?;
        if addresses.is_empty() {
            return Err(Error::InvalidRequest {
                message: "addresses must not be empty".to_string(),
            });
        }
        if addresses.len() > BATCH_MAX_ADDRESSES {
            return Err(Error::InvalidRequest {
                message: format!(
                    "at most {BATCH_MAX_ADDRESSES} addresses per request, received {}",
                    addresses.len()
                ),
            });
        }
        let mut body = json!({
            "blockchain": chain.as_str(),
            "type": wallet_type.as_str(),
            "addresses": addresses,
        });
        if let Some(period) = period {
            body["period"] = json!(period.as_str());
        }
        if let Some(fields) = fields {
            body["fields"] = json!(fields);
        }
        self.client.post("/wallets/batch", body).await
    }
}

/// Token endpoints.
#[derive(Debug)]
pub struct Tokens<'a> {
    client: &'a CabalSpy,
}

impl Tokens<'_> {
    /// `GET /v1/tokens/transactions` — trades by tracked wallets in this token.
    pub async fn transactions(
        &self,
        chain: Chain,
        mint: &str,
        wallet_type: Option<WalletType>,
        limit: Option<u32>,
    ) -> Result<Envelope<serde_json::Value>> {
        if let Some(wallet_type) = wallet_type {
            check_type(chain, wallet_type)?;
        }
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "mint", mint);
        push_opt(&mut query, "type", wallet_type);
        push_opt(&mut query, "limit", limit);
        self.client.get("/tokens/transactions", query).await
    }

    /// `GET /v1/tokens/stats` — aggregated token statistics.
    ///
    /// Passing `None` for the wallet type merges every type of that chain.
    ///
    /// Note that this endpoint does not pass the native price into its market cap
    /// builder, so `market_cap_usd`, `price_usd` and `sol_price_usd` come back
    /// `None` even on Solana. `holders` returns them populated.
    pub async fn stats(
        &self,
        chain: Chain,
        mint: &str,
        wallet_type: Option<WalletType>,
    ) -> Result<Envelope<TokenStats>> {
        if let Some(wallet_type) = wallet_type {
            check_type(chain, wallet_type)?;
        }
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "mint", mint);
        push_opt(&mut query, "type", wallet_type);
        self.client.get("/tokens/stats", query).await
    }

    /// `GET /v1/tokens/holders` — tracked holders, sorted by balance.
    pub async fn holders(
        &self,
        chain: Chain,
        mint: &str,
        wallet_type: Option<WalletType>,
        limit: Option<u32>,
    ) -> Result<Envelope<serde_json::Value>> {
        if let Some(wallet_type) = wallet_type {
            check_type(chain, wallet_type)?;
        }
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "mint", mint);
        push_opt(&mut query, "type", wallet_type);
        push_opt(&mut query, "limit", limit);
        self.client.get("/tokens/holders", query).await
    }

    /// `POST /v1/tokens/batch` — up to 100 mints in one request.
    pub async fn batch(
        &self,
        chain: Chain,
        wallet_type: WalletType,
        mints: &[String],
        fields: Option<&[String]>,
    ) -> Result<Envelope<serde_json::Value>> {
        check_type(chain, wallet_type)?;
        if mints.is_empty() {
            return Err(Error::InvalidRequest {
                message: "mints must not be empty".to_string(),
            });
        }
        if mints.len() > BATCH_MAX_MINTS {
            return Err(Error::InvalidRequest {
                message: format!(
                    "at most {BATCH_MAX_MINTS} mints per request, received {}",
                    mints.len()
                ),
            });
        }
        let mut body = json!({
            "blockchain": chain.as_str(),
            "type": wallet_type.as_str(),
            "mints": mints,
        });
        if let Some(fields) = fields {
            body["fields"] = json!(fields);
        }
        self.client.post("/tokens/batch", body).await
    }
}

/// The transaction feed.
#[derive(Debug)]
pub struct Transactions<'a> {
    client: &'a CabalSpy,
}

impl Transactions<'_> {
    /// `GET /v1/transactions/latest` — most recent trades by tracked wallets.
    pub async fn latest(
        &self,
        chain: Chain,
        wallet_type: WalletType,
        opts: FeedOpts,
    ) -> Result<Envelope<TransactionsList>> {
        check_type(chain, wallet_type)?;
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "type", wallet_type);
        opts.apply(&mut query);
        self.client.get("/transactions/latest", query).await
    }

    /// `GET /v1/transactions/timerange` — trades in the last N, capped at 60 minutes.
    pub async fn timerange(
        &self,
        chain: Chain,
        wallet_type: WalletType,
        opts: FeedOpts,
    ) -> Result<Envelope<TransactionsList>> {
        check_type(chain, wallet_type)?;
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "type", wallet_type);
        opts.apply(&mut query);
        self.client.get("/transactions/timerange", query).await
    }

    /// `GET /v1/transactions/count` — trade count and unique wallets, up to 24 hours.
    pub async fn count(
        &self,
        chain: Chain,
        wallet_type: WalletType,
        opts: FeedOpts,
    ) -> Result<Envelope<CountResponse>> {
        check_type(chain, wallet_type)?;
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "type", wallet_type);
        opts.apply(&mut query);
        self.client.get("/transactions/count", query).await
    }

    /// `GET /v1/transactions/volume` — volume in native currency and USD, up to 24 hours.
    pub async fn volume(
        &self,
        chain: Chain,
        wallet_type: WalletType,
        opts: FeedOpts,
    ) -> Result<Envelope<VolumeResponse>> {
        check_type(chain, wallet_type)?;
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "type", wallet_type);
        opts.apply(&mut query);
        self.client.get("/transactions/volume", query).await
    }
}

/// Cluster, entry and exit signals.
#[derive(Debug)]
pub struct Signals<'a> {
    client: &'a CabalSpy,
}

impl Signals<'_> {
    /// `GET /v1/signals` — live clusters, entries and exits.
    ///
    /// Smart money is unavailable on Ethereum.
    pub async fn list(
        &self,
        chain: Chain,
        wallet_type: WalletType,
        mode: SignalMode,
        opts: SignalOpts,
    ) -> Result<Envelope<SignalsResponse>> {
        check_type(chain, wallet_type)?;
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "type", wallet_type);
        push(&mut query, "mode", mode.as_str());
        opts.apply(&mut query);
        self.client.get("/signals", query).await
    }

    /// `GET /v1/signals/history` — backtest over 7, 30 or 90 days, or `"all"`.
    pub async fn history(
        &self,
        chain: Chain,
        wallet_type: WalletType,
        days: Option<&str>,
        limit: Option<u32>,
        mode: Option<SignalMode>,
    ) -> Result<Envelope<serde_json::Value>> {
        check_type(chain, wallet_type)?;
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "type", wallet_type);
        push_opt(&mut query, "days", days);
        push_opt(&mut query, "limit", limit);
        push_opt(&mut query, "mode", mode.map(|m| m.as_str()));
        self.client.get("/signals/history", query).await
    }
}

/// Aggregate analytics.
#[derive(Debug)]
pub struct Analytics<'a> {
    client: &'a CabalSpy,
}

impl Analytics<'_> {
    /// `GET /v1/analytics` — volume trend, most traded, win rate or top performers.
    pub async fn get(
        &self,
        chain: Chain,
        wallet_type: WalletType,
        mode: AnalyticsMode,
        period: Option<Period>,
        limit: Option<u32>,
    ) -> Result<Envelope<serde_json::Value>> {
        check_type(chain, wallet_type)?;
        let mut query = Query::new();
        push(&mut query, "blockchain", chain);
        push(&mut query, "type", wallet_type);
        push(&mut query, "mode", mode.as_str());
        push_opt(&mut query, "period", period);
        push_opt(&mut query, "limit", limit);
        self.client.get("/analytics", query).await
    }
}

/// KOL bundle detection. Solana only.
#[derive(Debug)]
pub struct Bundle<'a> {
    client: &'a CabalSpy,
}

impl Bundle<'_> {
    /// `GET /v1/bundle` — snapshot of the bundle stream.
    pub async fn get(&self, mint: &str) -> Result<Envelope<BundleResponse>> {
        let mut query = Query::new();
        push(&mut query, "blockchain", Chain::Solana);
        push(&mut query, "mint", mint);
        self.client.get("/bundle", query).await
    }
}

/// Kept out of the public surface, but used by the type checker in tests.
#[allow(dead_code)]
fn _assert_send_sync() {
    fn assert<T: Send + Sync>() {}
    assert::<CabalSpy>();
    assert::<HashMap<String, serde_json::Value>>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_constructor_uses_the_demo_key_and_normal_hosts() {
        let client = CabalSpy::builder()
            .demo()
            .base_url(DEFAULT_BASE_URL)
            .ws_url(DEFAULT_WS_URL)
            .build()
            .unwrap();
        assert!(client.is_demo());
        assert_eq!(client.base_url(), "https://api.cabalspy.xyz/v1");
        assert_eq!(
            client.websocket_url(),
            "wss://stream.cabalspy.xyz/?apiKey=demo"
        );
    }

    #[test]
    fn demo_shortcut_is_a_demo_client() {
        assert!(CabalSpy::demo().unwrap().is_demo());
        assert!(!CabalSpy::new("sk_live_x").unwrap().is_demo());
        assert_eq!(DEMO_API_KEY, "demo");
    }

    #[test]
    fn demo_envelope_is_parsed() {
        let raw = r#"{
            "success": true,
            "data": [{"wallet_address": "abc"}],
            "pagination": {"limit": 5, "has_more": false, "next_cursor": null},
            "meta": {"request_id": "r1"},
            "demo": {
                "notice": "Demo data, delayed 15 minutes.",
                "remaining_today": 17,
                "upgrade": {
                    "test_key": "https://apidashboard.cabalspy.xyz/",
                    "pay_per_call": "https://www.cabalspy.xyz/x402/",
                    "docs": "https://docs.cabalspy.xyz"
                }
            }
        }"#;
        let env: RawEnvelope<serde_json::Value> = serde_json::from_str(raw).unwrap();
        let pagination = env.pagination.unwrap();
        assert_eq!(pagination.limit, 5);
        assert!(!pagination.has_more);
        let info = demo_info(env.demo, Some(3)).unwrap();
        // The body wins over the header.
        assert_eq!(info.remaining_today, Some(17));
        assert_eq!(
            info.upgrade.unwrap().docs.as_deref(),
            Some("https://docs.cabalspy.xyz")
        );
    }

    #[test]
    fn demo_info_falls_back_to_the_header() {
        assert_eq!(demo_info(None, Some(4)).unwrap().remaining_today, Some(4));
        assert!(demo_info(None, None).is_none());
        // A non-object `demo` value does not break anything.
        assert!(demo_info(Some(serde_json::json!(true)), None).is_none());
    }
}
