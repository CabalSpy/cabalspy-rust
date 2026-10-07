//! # CabalSpy
//!
//! Official Rust client for the [CabalSpy API](https://docs.cabalspy.xyz): a realtime
//! multichain data layer for labeled wallets, covering Solana, Base, BNB Chain,
//! Ethereum and Robinhood Chain.
//!
//! Track what Key Opinion Leaders, smart money wallets and whales are actually
//! buying, with PnL, holder data, cluster signals and bundle detection.
//!
//! ```no_run
//! use cabalspy::{CabalSpy, Chain, Period, SignalMode, SignalOpts, WalletType, LeaderboardOpts};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), cabalspy::Error> {
//!     let client = CabalSpy::from_env()?;
//!
//!     // Who is this wallet? Searches every chain and wallet type.
//!     let wallet = client.wallets().lookup("As7HjL7dzzvbRbaD3WCun47robib2kmAKRXMvjHkSMB5").await?;
//!     println!("{:?}", wallet.data);
//!
//!     // Best KOL wallets on Solana over 7 days.
//!     let board = client
//!         .wallets()
//!         .leaderboard(Chain::Solana, LeaderboardOpts {
//!             wallet_type: Some(WalletType::Kol),
//!             period: Some(Period::D7),
//!             limit: Some(25),
//!             ..Default::default()
//!         })
//!         .await?;
//!     println!("{}", board.pagination.map(|p| p.total).unwrap_or(0));
//!
//!     // Live cluster signals: five or more KOLs buying the same token within an hour.
//!     let signals = client
//!         .signals()
//!         .list(Chain::Solana, WalletType::Kol, SignalMode::Cluster, SignalOpts {
//!             min_wallets: Some(5),
//!             hours: Some(1),
//!             ..Default::default()
//!         })
//!         .await?;
//!     for signal in &signals.data.signals {
//!         println!("{:?} {:?}", signal.token.token_name, signal.window.wallet_count);
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Try it without signing up
//!
//! [`CabalSpy::demo`] uses the public demo key, [`DEMO_API_KEY`]. Every response
//! then carries [`Envelope::demo`] with the remaining daily budget.
//!
//! ```no_run
//! # async fn run() -> Result<(), cabalspy::Error> {
//! use cabalspy::{CabalSpy, Chain, WalletType};
//!
//! let client = CabalSpy::demo()?;
//! let kols = client.wallets().list(Chain::Solana, WalletType::Kol, None, None).await?;
//! println!("{} left today", kols.demo.and_then(|d| d.remaining_today).unwrap_or(0));
//! # Ok(()) }
//! ```
//!
//! 20 requests per IP per UTC day shared with the websocket, data delayed by
//! 15 minutes, at most 5 rows per list. Once spent, requests fail with
//! [`Error::DemoLimit`]. A free test key with 1,000 realtime requests per month
//! is at <https://apidashboard.cabalspy.xyz/>.
//!
//! ## Chain coverage
//!
//! | Chain | Identifier | Currency | Wallet types |
//! |---|---|---|---|
//! | Solana | `Chain::Solana` | SOL | Kol, Smart, Whale |
//! | BNB Chain | `Chain::Bnb` | BNB | Kol, Smart |
//! | Base | `Chain::Base` | ETH | Kol, Smart |
//! | Ethereum | `Chain::Eth` | ETH | Kol |
//! | Robinhood Chain | `Chain::Rh` | ETH | Kol, Smart |
//!
//! Impossible combinations are rejected before a request goes out, so a mistake
//! costs no credits:
//!
//! ```no_run
//! # use cabalspy::{CabalSpy, Chain, WalletType, Error};
//! # async fn run() -> Result<(), Error> {
//! # let client = CabalSpy::from_env()?;
//! let result = client.wallets().list(Chain::Eth, WalletType::Whale, None, None).await;
//! assert!(matches!(result, Err(Error::InvalidRequest { .. })));
//! # Ok(()) }
//! ```
//!
//! ## Market cap availability
//!
//! On REST, `market_cap`, `price`, `unrealized_pnl_*` and `remaining_*` are
//! populated for Solana only; on the other chains they are `None`. Realized PnL,
//! invested amounts, holdings and counters work everywhere. See
//! [`Chain::has_rest_marketcap`].
//!
//! The websocket gateway populates those fields on every chain, and unlike REST it
//! does not zero the unrealized values once a position is fully sold.
//!
//! ## Timestamps
//!
//! Timestamps inside `data` arrive as `"YYYY-MM-DD HH:MM:SS"` with no offset and
//! are UTC. Only `meta.timestamp` carries a `Z`. Use [`parse_api_timestamp`] rather
//! than assuming a format, and note that treating them as local time shifts every
//! value by the machine's offset.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod client;
pub mod error;
pub mod models;

pub use client::{
    Analytics, Bundle, CabalSpy, CabalSpyBuilder, FeedOpts, LeaderboardOpts, Signals, SignalOpts,
    System, Tokens, Transactions, Wallets, BATCH_MAX_ADDRESSES, BATCH_MAX_MINTS, DEFAULT_BASE_URL,
    DEFAULT_WS_URL, DEMO_API_KEY,
};
pub use error::{ApiErrorBody, Error, RateLimit, Result};
pub use models::{
    ActiveTokensSummary, AnalyticsMode, BundleEntry, BundlePosition, BundleResponse, BundleWallet,
    Chain, CountResponse, DemoInfo, DemoUpgrade, Envelope, FeedTransaction, HealthResponse, HoldingsAfter, MetaResponse,
    Pagination, Period, PeriodStats, ResponseMeta, Signal, SignalMode, SignalTokenBlock,
    SignalWallet, SignalWindow, SignalsResponse, TokenBlock, TokenStats, TokenTrader, TotalHolders,
    TotalHoldings, TraderHoldings, TraderStats, TransactionsList, VolumeResponse, WalletProfile,
    WalletTracker, WalletType, WinRateDistribution, CHAINS,
};

/// Parses a timestamp from a response body into Unix seconds, always as UTC.
///
/// Accepts both forms the API uses:
///
/// * `"2026-07-25 21:33:14"` — what appears inside `data`. No offset, UTC.
/// * `"2026-07-25T21:33:14Z"` — what appears in `meta.timestamp`.
///
/// Returns `None` for anything it cannot read, rather than guessing. Kept
/// dependency free on purpose; feed the result into `chrono` or `time` if you
/// need a full date type.
///
/// ```
/// # use cabalspy::parse_api_timestamp;
/// assert_eq!(parse_api_timestamp("2026-07-25 21:33:14"), Some(1785015194));
/// assert_eq!(parse_api_timestamp("2026-07-25T21:33:14Z"), Some(1785015194));
/// assert_eq!(parse_api_timestamp("nonsense"), None);
/// ```
pub fn parse_api_timestamp(value: &str) -> Option<i64> {
    let text = value.trim();
    let bytes = text.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    // Both forms share the same layout, differing only in the separator.
    if !matches!(bytes[10], b' ' | b'T') {
        return None;
    }

    let num = |from: usize, to: usize| -> Option<i64> { text.get(from..to)?.parse::<i64>().ok() };

    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let hour = num(11, 13)?;
    let minute = num(14, 16)?;
    let second = num(17, 19)?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Days since the Unix epoch. Howard Hinnant's civil calendar algorithm.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_read_as_utc() {
        assert_eq!(parse_api_timestamp("2026-07-25 21:33:14"), Some(1785015194));
        assert_eq!(parse_api_timestamp("2026-07-25T21:33:14Z"), Some(1785015194));
        assert_eq!(parse_api_timestamp("1970-01-01 00:00:00"), Some(0));
        assert_eq!(parse_api_timestamp("2000-02-29 12:00:00"), Some(951825600));
    }

    #[test]
    fn bad_timestamps_return_none() {
        assert_eq!(parse_api_timestamp(""), None);
        assert_eq!(parse_api_timestamp("nonsense"), None);
        assert_eq!(parse_api_timestamp("2026-13-01 00:00:00"), None);
        assert_eq!(parse_api_timestamp("2026-07-25"), None);
    }

    #[test]
    fn chains_know_their_wallet_types() {
        assert!(Chain::Solana.supports(WalletType::Whale));
        assert!(!Chain::Eth.supports(WalletType::Whale));
        assert!(!Chain::Base.supports(WalletType::Whale));
        assert!(Chain::Rh.supports(WalletType::Smart));
        assert_eq!(Chain::Eth.wallet_types().len(), 1);
        assert_eq!(Chain::Solana.currency(), "SOL");
        assert_eq!(Chain::Rh.currency(), "ETH");
        assert!(Chain::Solana.has_rest_marketcap());
        assert!(!Chain::Base.has_rest_marketcap());
    }

    #[test]
    fn enums_serialize_to_the_wire_format() {
        assert_eq!(Chain::Rh.as_str(), "rh");
        assert_eq!(Period::D30.as_str(), "30d");
        assert_eq!(AnalyticsMode::TopPerformers.as_str(), "top_performers");
        assert_eq!(SignalMode::Cluster.as_str(), "cluster");
        assert_eq!(serde_json::to_string(&Chain::Bnb).unwrap(), "\"bnb\"");
        assert_eq!(serde_json::to_string(&Period::H6).unwrap(), "\"6h\"");
        assert_eq!(
            serde_json::to_string(&AnalyticsMode::WinRate).unwrap(),
            "\"win_rate\""
        );
    }

    #[test]
    fn responses_survive_missing_and_unknown_fields() {
        // Nulls everywhere, which is what the non-Solana chains actually return.
        let raw = r#"{
            "token": {"mint": "abc", "market_cap": null, "price_usd": null},
            "total_holders": {"kol_count": 2},
            "some_field_added_later": {"nested": true}
        }"#;
        let stats: TokenStats = serde_json::from_str(raw).unwrap();
        assert_eq!(stats.token.mint.as_deref(), Some("abc"));
        assert_eq!(stats.token.market_cap, None);
        assert_eq!(stats.total_holders.kol_count, Some(2));
        assert!(stats.traders.is_empty());
        // Unknown fields are kept rather than dropped.
        assert!(stats.extra.contains_key("some_field_added_later"));
    }
}
