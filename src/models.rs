//! Chains, wallet types and response models.
//!
//! Every response field is optional and defaulted. The API returns `null` in many
//! places, and adds fields over time; deserialization must never fail because of
//! that. Anything not modelled here is kept in the `extra` map on the larger
//! structs, so nothing is silently dropped.
//!
//! Two conventions worth knowing:
//!
//! 1. Timestamps inside `data` are not RFC 3339. They arrive as
//!    `"YYYY-MM-DD HH:MM:SS"` with no offset, and are UTC. Only `meta.timestamp`
//!    carries a `Z`. They are kept as `String` here so no timezone assumption is
//!    baked in; see [`crate::parse_api_timestamp`].
//! 2. `realized_pnl` is `total_sell - total_buy`. A wallet that bought and has
//!    not sold reports its whole investment as a loss and `-100` percent. Check
//!    `still_holding` before showing it to a user.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

// ── enums ────────────────────────────────────────────────────────────────────

/// A supported blockchain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    Solana,
    Bnb,
    Base,
    Eth,
    /// Robinhood Chain, Robinhood's Ethereum L2 on the Arbitrum Orbit stack.
    Rh,
}

/// Every chain this crate knows about.
pub const CHAINS: [Chain; 5] = [Chain::Solana, Chain::Bnb, Chain::Base, Chain::Eth, Chain::Rh];

impl Chain {
    pub fn as_str(self) -> &'static str {
        match self {
            Chain::Solana => "solana",
            Chain::Bnb => "bnb",
            Chain::Base => "base",
            Chain::Eth => "eth",
            Chain::Rh => "rh",
        }
    }

    /// The wallet types this chain actually has.
    pub fn wallet_types(self) -> &'static [WalletType] {
        match self {
            Chain::Solana => &[WalletType::Kol, WalletType::Smart, WalletType::Whale],
            Chain::Bnb | Chain::Base | Chain::Rh => &[WalletType::Kol, WalletType::Smart],
            Chain::Eth => &[WalletType::Kol],
        }
    }

    /// True when this chain has that wallet type.
    pub fn supports(self, wallet_type: WalletType) -> bool {
        self.wallet_types().contains(&wallet_type)
    }

    /// The chain's native currency.
    pub fn currency(self) -> &'static str {
        match self {
            Chain::Solana => "SOL",
            Chain::Bnb => "BNB",
            Chain::Base | Chain::Eth | Chain::Rh => "ETH",
        }
    }

    /// Whether the REST API returns market cap, price and unrealized PNL here.
    ///
    /// On every other chain those REST fields are `null`. The websocket gateway
    /// is different: it populates them on all chains.
    pub fn has_rest_marketcap(self) -> bool {
        matches!(self, Chain::Solana)
    }
}

impl fmt::Display for Chain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The kind of wallet a label describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WalletType {
    /// Key Opinion Leader: a trader whose calls move markets.
    Kol,
    /// Smart money: identified by track record rather than reach.
    Smart,
    /// Whale. Solana only.
    Whale,
}

impl WalletType {
    pub fn as_str(self) -> &'static str {
        match self {
            WalletType::Kol => "kol",
            WalletType::Smart => "smart",
            WalletType::Whale => "whale",
        }
    }
}

impl fmt::Display for WalletType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A statistics window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Period {
    #[serde(rename = "6h")]
    H6,
    #[serde(rename = "1d")]
    D1,
    #[serde(rename = "7d")]
    D7,
    #[serde(rename = "30d")]
    D30,
}

impl Period {
    pub fn as_str(self) -> &'static str {
        match self {
            Period::H6 => "6h",
            Period::D1 => "1d",
            Period::D7 => "7d",
            Period::D30 => "30d",
        }
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which kind of signal to return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignalMode {
    Cluster,
    Entry,
    Exit,
}

impl SignalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SignalMode::Cluster => "cluster",
            SignalMode::Entry => "entry",
            SignalMode::Exit => "exit",
        }
    }
}

/// Which analytics view to return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsMode {
    VolumeTrend,
    MostTraded,
    WinRate,
    TopPerformers,
}

impl AnalyticsMode {
    pub fn as_str(self) -> &'static str {
        match self {
            AnalyticsMode::VolumeTrend => "volume_trend",
            AnalyticsMode::MostTraded => "most_traded",
            AnalyticsMode::WinRate => "win_rate",
            AnalyticsMode::TopPerformers => "top_performers",
        }
    }
}

// ── envelope ─────────────────────────────────────────────────────────────────

/// Metadata attached to every successful response.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResponseMeta {
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub cached: bool,
    #[serde(default)]
    pub cache_age_seconds: i64,
    #[serde(default)]
    pub version: String,
    /// RFC 3339 with a `Z` suffix, unlike the timestamps inside `data`.
    #[serde(default)]
    pub timestamp: String,
}

/// Cursor pagination block.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Pagination {
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// A full response: the payload plus pagination, meta and rate limit headers.
#[derive(Debug, Clone)]
pub struct Envelope<T> {
    pub data: T,
    pub pagination: Option<Pagination>,
    pub meta: ResponseMeta,
    pub rate_limit: crate::error::RateLimit,
    pub status: u16,
    /// Set when the request ran on the public demo key. See [`DemoInfo`].
    pub demo: Option<DemoInfo>,
}

/// The top-level `demo` object the API adds to every response made with the
/// public demo key.
///
/// Demo data is delayed by 15 minutes and capped at 5 rows per list, and the
/// key allows 20 requests per IP per UTC day. `remaining_today` tells you how
/// many are left; the `X-Demo-Remaining` header fills it in if the body lacks it.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DemoInfo {
    #[serde(default)]
    pub notice: Option<String>,
    #[serde(default)]
    pub remaining_today: Option<u64>,
    #[serde(default)]
    pub upgrade: Option<DemoUpgrade>,
}

/// Where to go once the demo is no longer enough.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DemoUpgrade {
    /// Free test key with 1,000 realtime requests per month.
    #[serde(default)]
    pub test_key: Option<String>,
    /// Pay per call with x402, no account needed.
    #[serde(default)]
    pub pay_per_call: Option<String>,
    #[serde(default)]
    pub docs: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawEnvelope<T> {
    pub data: T,
    #[serde(default)]
    pub pagination: Option<Pagination>,
    #[serde(default)]
    pub meta: ResponseMeta,
    /// Kept loose: error bodies send `"demo": true`, success bodies an object.
    #[serde(default)]
    pub demo: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawError {
    #[serde(default)]
    pub error: crate::error::ApiErrorBody,
    /// Sent alongside `demo_limit_reached`.
    #[serde(default)]
    pub upgrade: Option<serde_json::Value>,
}

// ── shared models ────────────────────────────────────────────────────────────

/// A labeled wallet's public identity.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct WalletProfile {
    #[serde(default)]
    pub wallet_address: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub twitter: Option<String>,
    #[serde(default)]
    pub telegram: Option<String>,
    #[serde(default)]
    pub copytrade_link: Option<String>,
    #[serde(default, rename = "type")]
    pub wallet_type: Option<WalletType>,
    #[serde(default)]
    pub blockchain: Option<Chain>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Token block as returned by the token endpoints.
///
/// `market_cap_usd`, `price_usd` and `sol_price_usd` are only populated where the
/// endpoint passes the native price into its market cap builder. `tokens/holders`
/// does; `tokens/stats` and `tokens/transactions` do not, so those come back
/// `None` there even on Solana.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenBlock {
    #[serde(default)]
    pub mint: Option<String>,
    #[serde(default)]
    pub token_name: Option<String>,
    #[serde(default)]
    pub token_supply: Option<f64>,
    #[serde(default)]
    pub token_decimals: Option<i64>,
    #[serde(default)]
    pub blockchain: Option<Chain>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub market_cap: Option<f64>,
    #[serde(default)]
    pub market_cap_usd: Option<f64>,
    #[serde(default)]
    pub market_cap_currency: Option<String>,
    #[serde(default)]
    pub price: Option<f64>,
    #[serde(default)]
    pub price_usd: Option<f64>,
    #[serde(default)]
    pub sol_price_usd: Option<f64>,
    #[serde(default)]
    pub pool: Option<String>,
    #[serde(default)]
    pub on_curve: Option<bool>,
    #[serde(default)]
    pub bonding_curve_progress: Option<f64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── wallets/tracker ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PeriodStats {
    #[serde(default)]
    pub period: Option<String>,
    #[serde(default)]
    pub buy_txn: Option<i64>,
    #[serde(default)]
    pub sell_txn: Option<i64>,
    #[serde(default)]
    pub total_buy: Option<f64>,
    #[serde(default)]
    pub total_buy_usd: Option<f64>,
    #[serde(default)]
    pub total_sell: Option<f64>,
    #[serde(default)]
    pub total_sell_usd: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub volume_usd: Option<f64>,
    /// `total_sell - total_buy`. See the note at the top of this module.
    #[serde(default)]
    pub realized_pnl: Option<f64>,
    #[serde(default)]
    pub realized_pnl_usd: Option<f64>,
    #[serde(default)]
    pub realized_pnl_percentage: Option<f64>,
    #[serde(default)]
    pub win_count: Option<i64>,
    #[serde(default)]
    pub loss_count: Option<i64>,
    #[serde(default)]
    pub best_trade_pnl: Option<f64>,
    #[serde(default)]
    pub worst_trade_pnl: Option<f64>,
    #[serde(default)]
    pub largest_buy: Option<f64>,
    #[serde(default)]
    pub avg_hold_time_minutes: Option<f64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WinRateDistribution {
    #[serde(default)]
    pub below_zero: Option<i64>,
    #[serde(default)]
    pub above_zero_to_100: Option<i64>,
    #[serde(default)]
    pub above_100_to_500: Option<i64>,
    #[serde(default)]
    pub above_500: Option<i64>,
    #[serde(default)]
    pub closed_count: Option<i64>,
    #[serde(default)]
    pub win_count: Option<i64>,
    #[serde(default)]
    pub win_rate_percentage: Option<f64>,
}

/// A summary object, not a list, despite the plural name on the wire.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ActiveTokensSummary {
    #[serde(default)]
    pub active_tokens_count: Option<i64>,
    #[serde(default)]
    pub still_holding_count: Option<i64>,
    #[serde(default)]
    pub avg_position_size: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WalletTracker {
    #[serde(default)]
    pub wallet: Option<String>,
    #[serde(default)]
    pub profile: WalletProfile,
    #[serde(default)]
    pub period_stats: PeriodStats,
    #[serde(default)]
    pub period_active_tokens: ActiveTokensSummary,
    #[serde(default)]
    pub period_win_rate_distribution: WinRateDistribution,
    #[serde(default)]
    pub period_history_tokens: Vec<serde_json::Value>,
    #[serde(default)]
    pub period_realized_pnl_chart: Vec<serde_json::Value>,
    #[serde(default)]
    pub period_trades: Vec<serde_json::Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── tokens/stats ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TotalHolders {
    #[serde(default)]
    pub kol_count: Option<i64>,
    #[serde(default)]
    pub smart_count: Option<i64>,
    #[serde(default)]
    pub whale_count: Option<i64>,
    #[serde(default)]
    pub still_holding_count: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TotalHoldings {
    #[serde(default)]
    pub token_amount: Option<f64>,
    #[serde(default)]
    pub token_amount_peak: Option<f64>,
    #[serde(default)]
    pub supply_pct: Option<f64>,
    #[serde(default)]
    pub supply_pct_peak: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TraderHoldings {
    #[serde(default)]
    pub token_amount: Option<f64>,
    #[serde(default)]
    pub token_amount_peak: Option<f64>,
    #[serde(default)]
    pub supply_pct: Option<f64>,
    #[serde(default)]
    pub supply_pct_peak: Option<f64>,
    #[serde(default)]
    pub bag_pct: Option<f64>,
    #[serde(default)]
    pub still_holding: Option<bool>,
    /// Frozen at the first buy and never recomputed.
    #[serde(default)]
    pub entry_market_cap: Option<f64>,
    #[serde(default)]
    pub entry_market_cap_usd: Option<f64>,
    #[serde(default)]
    pub unrealized_pnl_sol: Option<f64>,
    #[serde(default)]
    pub unrealized_pnl_usd: Option<f64>,
    #[serde(default)]
    pub unrealized_pnl_pct: Option<f64>,
    #[serde(default)]
    pub remaining_sol: Option<f64>,
    #[serde(default)]
    pub remaining_usd: Option<f64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TraderStats {
    #[serde(default)]
    pub buy: Option<f64>,
    #[serde(default)]
    pub buy_usd: Option<f64>,
    #[serde(default)]
    pub buy_count: Option<i64>,
    #[serde(default)]
    pub buy_tokens: Option<f64>,
    #[serde(default)]
    pub sell: Option<f64>,
    #[serde(default)]
    pub sell_usd: Option<f64>,
    #[serde(default)]
    pub sell_count: Option<i64>,
    #[serde(default)]
    pub sell_tokens: Option<f64>,
    #[serde(default)]
    pub avg_buy_price: Option<f64>,
    /// `total_sell - total_buy`. See the note at the top of this module.
    #[serde(default)]
    pub realized_pnl: Option<f64>,
    #[serde(default)]
    pub realized_pnl_usd: Option<f64>,
    #[serde(default)]
    pub realized_pnl_percentage: Option<f64>,
    /// `"YYYY-MM-DD HH:MM:SS"`, UTC without a marker.
    #[serde(default)]
    pub first_trade_at: Option<String>,
    #[serde(default)]
    pub last_trade_at: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenTrader {
    #[serde(default)]
    pub profile: WalletProfile,
    #[serde(default)]
    pub trader_holdings: TraderHoldings,
    #[serde(default)]
    pub trader_stats: TraderStats,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenStats {
    #[serde(default)]
    pub token: TokenBlock,
    #[serde(default)]
    pub total_holders: TotalHolders,
    #[serde(default)]
    pub total_holdings: TotalHoldings,
    #[serde(default)]
    pub total_statistics: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub traders: Vec<TokenTrader>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── signals ──────────────────────────────────────────────────────────────────

/// Token block on the signals endpoint. Carries no market cap at all.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SignalTokenBlock {
    #[serde(default)]
    pub mint: Option<String>,
    #[serde(default)]
    pub token_name: Option<String>,
    #[serde(default)]
    pub token_supply: Option<f64>,
    #[serde(default)]
    pub token_decimals: Option<i64>,
    #[serde(default)]
    pub blockchain: Option<Chain>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SignalWallet {
    #[serde(default)]
    pub profile: WalletProfile,
    #[serde(default)]
    pub bought_at: Option<String>,
    #[serde(default)]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub still_holding: Option<bool>,
    #[serde(default)]
    pub buy_txn: Option<i64>,
    #[serde(default)]
    pub sell_txn: Option<i64>,
    #[serde(default)]
    pub held_tokens: Option<f64>,
    #[serde(default)]
    pub held_tokens_peak: Option<f64>,
    #[serde(default)]
    pub bag_pct: Option<f64>,
    #[serde(default)]
    pub supply_pct: Option<f64>,
    #[serde(default)]
    pub total_buy: Option<f64>,
    #[serde(default)]
    pub total_buy_usd: Option<f64>,
    #[serde(default)]
    pub total_sell: Option<f64>,
    #[serde(default)]
    pub total_sell_usd: Option<f64>,
    /// `total_sell - total_buy`. See the note at the top of this module.
    #[serde(default)]
    pub realized_pnl: Option<f64>,
    #[serde(default)]
    pub realized_pnl_usd: Option<f64>,
    #[serde(default)]
    pub realized_pnl_percentage: Option<f64>,
    #[serde(default)]
    pub window_buy_txn: Option<i64>,
    #[serde(default)]
    pub window_invested: Option<f64>,
    #[serde(default)]
    pub window_invested_usd: Option<f64>,
    #[serde(default)]
    pub window_tokens_bought: Option<f64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SignalWindow {
    #[serde(default)]
    pub hours: Option<f64>,
    #[serde(default)]
    pub wallet_count: Option<i64>,
    #[serde(default)]
    pub first_buy_at: Option<String>,
    #[serde(default)]
    pub latest_buy_at: Option<String>,
    #[serde(default)]
    pub time_span_minutes: Option<f64>,
    #[serde(default)]
    pub total_invested: Option<f64>,
    #[serde(default)]
    pub total_invested_usd: Option<f64>,
    #[serde(default)]
    pub avg_invested: Option<f64>,
    #[serde(default)]
    pub avg_invested_usd: Option<f64>,
    #[serde(default)]
    pub total_tokens_bought: Option<f64>,
    #[serde(default)]
    pub supply_pct_bought: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Signal {
    #[serde(default)]
    pub signal_type: Option<String>,
    #[serde(default)]
    pub signal_strength: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub token: SignalTokenBlock,
    #[serde(default)]
    pub token_stats: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub wallets: Vec<SignalWallet>,
    #[serde(default)]
    pub window: SignalWindow,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SignalsResponse {
    #[serde(default)]
    pub blockchain: Option<Chain>,
    #[serde(default, rename = "type")]
    pub wallet_type: Option<WalletType>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub signals: Vec<Signal>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── feed ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HoldingsAfter {
    #[serde(default)]
    pub token_amount: Option<f64>,
    #[serde(default)]
    pub token_amount_peak: Option<f64>,
    #[serde(default)]
    pub supply_pct: Option<f64>,
    #[serde(default)]
    pub supply_pct_peak: Option<f64>,
    #[serde(default)]
    pub bag_pct: Option<f64>,
    #[serde(default)]
    pub still_holding: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FeedTransaction {
    #[serde(default)]
    pub tx_signature: Option<String>,
    #[serde(default)]
    pub mint: Option<String>,
    #[serde(default)]
    pub token_name: Option<String>,
    #[serde(default)]
    pub transaction_type: Option<String>,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub value_usd: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub token_amount: Option<f64>,
    #[serde(default)]
    pub price_per_token: Option<f64>,
    #[serde(default)]
    pub price_per_token_usd: Option<f64>,
    /// `"YYYY-MM-DD HH:MM:SS"`, UTC without a marker.
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub profile: WalletProfile,
    #[serde(default)]
    pub holdings_after: HoldingsAfter,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TransactionsList {
    #[serde(default)]
    pub blockchain: Option<Chain>,
    #[serde(default, rename = "type")]
    pub wallet_type: Option<WalletType>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub count: Option<i64>,
    #[serde(default)]
    pub transactions: Vec<FeedTransaction>,
    #[serde(default)]
    pub time_window_seconds: Option<i64>,
    /// Present when a requested limit or time window was capped.
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CountResponse {
    #[serde(default)]
    pub blockchain: Option<Chain>,
    #[serde(default, rename = "type")]
    pub wallet_type: Option<WalletType>,
    #[serde(default)]
    pub count: Option<i64>,
    #[serde(default)]
    pub wallet_count: Option<i64>,
    #[serde(default)]
    pub time_window_seconds: Option<i64>,
    #[serde(default)]
    pub mint: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VolumeResponse {
    #[serde(default)]
    pub blockchain: Option<Chain>,
    #[serde(default, rename = "type")]
    pub wallet_type: Option<WalletType>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub volume_usd: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub time_window_seconds: Option<i64>,
    #[serde(default)]
    pub mint: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

// ── bundle ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BundlePosition {
    #[serde(default)]
    pub held: Option<f64>,
    #[serde(default)]
    pub peak: Option<f64>,
    #[serde(default)]
    pub bag_pct: Option<f64>,
    #[serde(default)]
    pub supply_pct: Option<f64>,
    #[serde(default)]
    pub invested: Option<f64>,
    #[serde(default)]
    pub invested_usd: Option<f64>,
    #[serde(default)]
    pub buy_txn: Option<i64>,
    #[serde(default)]
    pub sell_txn: Option<i64>,
    #[serde(default)]
    pub sold_value: Option<f64>,
    #[serde(default)]
    pub realized_pnl_sol: Option<f64>,
    #[serde(default)]
    pub realized_pnl_usd: Option<f64>,
    #[serde(default)]
    pub unrealized_pnl_sol: Option<f64>,
    #[serde(default)]
    pub unrealized_pnl_usd: Option<f64>,
    #[serde(default)]
    pub unrealized_pnl_pct: Option<f64>,
    #[serde(default)]
    pub remaining_sol: Option<f64>,
    #[serde(default)]
    pub remaining_usd: Option<f64>,
    #[serde(default)]
    pub entry_market_cap: Option<f64>,
    #[serde(default)]
    pub entry_market_cap_usd: Option<f64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BundleWallet {
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub is_kol: Option<bool>,
    #[serde(default)]
    pub transaction_type: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub entry_source: Option<String>,
    #[serde(default)]
    pub profile: WalletProfile,
    #[serde(default)]
    pub position: BundlePosition,
    /// Side wallet evidence. Absent on the KOL wallet itself.
    #[serde(default)]
    pub fee_lamports: Option<i64>,
    #[serde(default)]
    pub block_index: Option<i64>,
    #[serde(default)]
    pub adjacent_to_kol: Option<bool>,
    #[serde(default)]
    pub same_fee: Option<bool>,
    #[serde(default)]
    pub occurrences: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BundleEntry {
    #[serde(default)]
    pub bundle_id: Option<String>,
    #[serde(default)]
    pub kol_wallet: Option<String>,
    #[serde(default)]
    pub kol_profile: Option<WalletProfile>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub jito_confirmed: Option<bool>,
    #[serde(default)]
    pub proof_type: Option<String>,
    #[serde(default)]
    pub slot: Option<i64>,
    #[serde(default)]
    pub wallet_count: Option<i64>,
    #[serde(default)]
    pub detected_at: Option<String>,
    #[serde(default)]
    pub bundle_wallets: Vec<BundleWallet>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BundleResponse {
    #[serde(default)]
    pub blockchain: Option<Chain>,
    #[serde(default)]
    pub token: TokenBlock,
    #[serde(default)]
    pub bundles: Vec<BundleEntry>,
}

// ── system ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HealthResponse {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub components: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MetaResponse {
    #[serde(default)]
    pub chains: Vec<String>,
    #[serde(default)]
    pub wallet_types: Vec<String>,
    #[serde(default)]
    pub periods: Vec<String>,
    #[serde(default)]
    pub currencies: HashMap<String, String>,
    #[serde(default)]
    pub wallet_counts: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub limits: HashMap<String, i64>,
}
