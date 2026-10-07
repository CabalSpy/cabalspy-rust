# CabalSpy Rust SDK — KOL and smart money wallet tracking API

Official Rust client for the **CabalSpy API**: a realtime multichain data layer for labeled wallets, covering **Solana, Base, BNB Chain, Ethereum and Robinhood Chain**.

Track what Key Opinion Leaders, smart money wallets and whales are actually buying — with PnL, holder data, cluster signals and bundle detection.

**What you can build with it:** copy-trading bots, KOL leaderboards, memecoin alert systems, wallet analytics dashboards, bundle and sniper detection, portfolio and PnL trackers.

```toml
[dependencies]
cabalspy = "0.2"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Quick start

```rust
use cabalspy::{CabalSpy, Chain, LeaderboardOpts, Period, SignalMode, SignalOpts, WalletType};

#[tokio::main]
async fn main() -> Result<(), cabalspy::Error> {
    let client = CabalSpy::from_env()?;   // reads CABALSPY_API_KEY

    let wallet = client.wallets().lookup("As7HjL7dzzvbRbaD3WCun47robib2kmAKRXMvjHkSMB5").await?;
    println!("{:?}", wallet.data);

    let board = client.wallets().leaderboard(Chain::Solana, LeaderboardOpts {
        wallet_type: Some(WalletType::Kol),
        period: Some(Period::D7),
        limit: Some(25),
        ..Default::default()
    }).await?;

    let signals = client.signals().list(
        Chain::Solana, WalletType::Kol, SignalMode::Cluster,
        SignalOpts { min_wallets: Some(5), hours: Some(1), ..Default::default() },
    ).await?;

    for signal in &signals.data.signals {
        println!("{:?} {:?}", signal.token.token_name, signal.window.wallet_count);
    }
    Ok(())
}
```

## Try it without signing up

The public demo key needs no account. `CabalSpy::demo()` uses it against the normal API:

```rust
let client = cabalspy::CabalSpy::demo()?;   // api key "demo"
let kols = client.wallets().list(Chain::Solana, WalletType::Kol, None, None).await?;
println!("{:?} requests left today", kols.demo.and_then(|d| d.remaining_today));
```

Or with no key at all:

```bash
curl "https://demo-api.cabalspy.xyz/v1/wallets?blockchain=solana&type=kol"
```

- 20 requests per IP per UTC day, shared between REST and websocket (each connection counts as one)
- Data delayed by 15 minutes, at most 5 rows per list, no pagination
- Websocket: one connection per IP, up to 3 subscriptions, closed after 30 minutes
- Every response carries a `demo` block (`Envelope::demo`) with `remaining_today` and upgrade links
- Once the budget is spent, requests fail with `Error::DemoLimit { resets_in_seconds, .. }`

For realtime data, get a free test key (1,000 requests per month) at
[apidashboard.cabalspy.xyz](https://apidashboard.cabalspy.xyz/), or pay per call with
[x402](https://www.cabalspy.xyz/x402/).

## Chain coverage

| Chain | Variant | Currency | Wallet types |
|---|---|---|---|
| Solana | `Chain::Solana` | SOL | Kol, Smart, Whale |
| BNB Chain | `Chain::Bnb` | BNB | Kol, Smart |
| Base | `Chain::Base` | ETH | Kol, Smart |
| Ethereum | `Chain::Eth` | ETH | Kol |
| Robinhood Chain | `Chain::Rh` | ETH | Kol, Smart |

Impossible combinations are rejected before a request goes out, so a mistake costs no credits:

```rust
let result = client.wallets().list(Chain::Eth, WalletType::Whale, None, None).await;
// Err(Error::InvalidRequest { message: "eth has no whale wallets; it supports: kol" })
```

### KOL tracking API for Solana

The deepest coverage of the five. Solana is the only chain with `WalletType::Whale`, live market cap,
pump.fun bonding curve progress, and the bundle endpoint that detects KOL wallets buying through Jito
bundles alongside side wallets they control.

```rust
let bundles = client.bundle().get("MINT_ADDRESS").await?;
for entry in &bundles.data.bundles {
    println!("{:?} confidence {:?}", entry.kol_wallet, entry.confidence);
}
```

### KOL tracking API for Base

Base carries both `Kol` and `Smart` wallets. Addresses are EVM format, values are denominated in ETH.

```rust
let feed = client.transactions()
    .latest(Chain::Base, WalletType::Smart, FeedOpts { limit: Some(50), ..Default::default() })
    .await?;
```

### KOL tracking API for BNB Chain

`Kol` and `Smart` wallets, denominated in BNB.

```rust
let board = client.wallets()
    .leaderboard(Chain::Bnb, LeaderboardOpts { period: Some(Period::D7), ..Default::default() })
    .await?;
```

### KOL tracking API for Ethereum

Ethereum mainnet carries `Kol` wallets only; smart money signals are unavailable there.

### KOL tracking API for Robinhood Chain

Robinhood Chain is Robinhood's Ethereum L2 on the Arbitrum Orbit stack, with ETH as the gas token.
Both `Kol` and `Smart` wallets are tracked.

```rust
let volume = client.transactions()
    .volume(Chain::Rh, WalletType::Kol, FeedOpts { hours: Some(24), ..Default::default() })
    .await?;
```

> **Market cap availability differs between REST and websocket.**
>
> On REST, `market_cap`, `price`, `unrealized_pnl_*` and `remaining_*` are populated for Solana
> only; elsewhere they are `None`. Realized PnL, invested amounts, holdings and counters work on
> every chain. See `Chain::has_rest_marketcap()`.
>
> The gateway populates them on every chain, and unlike REST it does not zero the unrealized
> values once a position is fully sold.

## Errors

```rust
match client.wallets().tracker(Chain::Solana, addr, None).await {
    Ok(env) => println!("{:?}", env.data.period_stats.realized_pnl),
    Err(cabalspy::Error::NotFound { .. }) => println!("not tracked"),
    Err(cabalspy::Error::InsufficientCredits { .. }) => println!("top up"),
    Err(cabalspy::Error::RateLimited { retry_after, .. }) => println!("{retry_after:?}"),
    Err(cabalspy::Error::DemoLimit { resets_in_seconds, .. }) => println!("demo resets in {resets_in_seconds:?}s"),
    Err(err) => return Err(err),
}
```

Every API error carries `.code()`, `.request_id()`, `.parameter()`, `.allowed()` and `.rate_limit()`.
`429` (except `DemoLimit`), `5xx` and network failures are retried with exponential backoff and jitter; a server-sent
`Retry-After` wins over the SDK's own timing.

## Timestamps

Timestamps inside `data` arrive as `"YYYY-MM-DD HH:MM:SS"` with no offset and are UTC.
Only `meta.timestamp` carries a `Z`. Treating them as local time shifts every value.

```rust
use cabalspy::parse_api_timestamp;
assert_eq!(parse_api_timestamp("2026-07-25 21:33:14"), Some(1785015194)); // Unix seconds, UTC
```

Dependency free on purpose. Feed the result into `chrono` or `time` if you need a full date type.

## A note on `realized_pnl`

It is `total_sell - total_buy`. A wallet that bought and has not sold reports its whole investment
as a loss, with `-100` percent. Check `still_holding` before showing that number to a user.

## Unknown fields are kept

Every response struct has an `extra: HashMap<String, serde_json::Value>` catch-all and every field
is optional, so a server-side addition never breaks deserialization and nothing is silently dropped.

For endpoints not yet modelled, or when you want the raw JSON:

```rust
let env = client.get::<serde_json::Value>("/wallets/leaderboard",
    vec![("blockchain".into(), "bnb".into()), ("period".into(), "30d".into())]).await?;
println!("{} {:?}", env.status, env.rate_limit.remaining);
```

## Status

REST is complete. The websocket gateway is not wrapped yet; until it is, connect to
`client.websocket_url()` with `tokio-tungstenite` and send
`{"op":"subscribe","stream":"tx","blockchain":"solana","type":"kol"}`.

## FAQ

### What is a KOL wallet?

KOL stands for Key Opinion Leader: a trader or crypto personality whose token calls move markets.
CabalSpy tracks their onchain wallets with a public identity attached — name, avatar, Twitter and
Telegram handle — so you can verify whether they actually bought what they promoted.

### How is smart money different from a KOL?

A KOL is identified by influence, a smart money wallet by track record. KOL trades carry social
signal, smart money trades carry statistical signal. Both are wallet types on the same endpoints, so
you can query either or merge them.

### What does bundle detection do?

On Solana, KOL wallets often buy through Jito bundles together with side wallets they control, which
hides the true size of a position. The bundle endpoint groups those wallets, reports a confidence
score and exposes the evidence: fee match, block index, adjacency to the KOL transaction.

### Which async runtime does this need?

Tokio. `reqwest` and the retry backoff both build on it. TLS is `rustls`, so there is no OpenSSL
dependency and cross compilation stays simple.

## Docs and support

[docs.cabalspy.xyz](https://docs.cabalspy.xyz) · free API key at [apidashboard.cabalspy.xyz](https://apidashboard.cabalspy.xyz/)

## Related

- [SDK overview](https://www.cabalspy.xyz/sdks/) on cabalspy.xyz · [KOL API](https://www.cabalspy.xyz/kol-api/) · [Smart Money API](https://www.cabalspy.xyz/smart-money-api/) · [use cases](https://www.cabalspy.xyz/use-cases/)
- SDKs: [TypeScript](https://www.npmjs.com/package/cabalspy) · [Python](https://pypi.org/project/cabalspy/) · [Rust](https://crates.io/crates/cabalspy)
- x402 clients (pay per request, no API key): [TypeScript](https://www.npmjs.com/package/cabalspy-x402) · [Python](https://pypi.org/project/cabalspy-x402/)
- [MCP server](https://www.cabalspy.xyz/mcp/) — for Claude, Cursor and VS Code
- [CabalSpy Terminal](https://app.cabalspy.xyz/) — trade with the same wallet data

## License

MIT
