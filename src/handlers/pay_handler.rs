//! HTTP 402 Payment Required — per-resource payment gating via `did:nostr` pubkeys.
//!
//! Wires `solid-pod-rs` payment types (Web Ledgers, PayConfig) into VisionClaw's
//! actix-web surface. Balances are tracked per `did:nostr:<hex-pubkey>` identity in
//! a filesystem-backed JSON ledger under a configurable data directory.
//!
//! ## Routes (all gated behind `PAY_ENABLED`)
//!
//! | Method | Path                 | Description                                 |
//! |--------|----------------------|---------------------------------------------|
//! | GET    | `/pay/.info`         | Payment info (cost, methods, endpoints)     |
//! | GET    | `/pay/.balance`      | Caller's balance (requires NIP-98 auth)     |
//! | POST   | `/pay/.deposit`      | Deposit stub (returns 501)                  |
//! | POST   | `/pay/.sell`         | Create a sell order on the order book       |
//! | GET    | `/pay/.offers`       | List active sell orders                     |
//! | POST   | `/pay/.swap`         | Execute a trade against a sell order        |
//! | GET    | `/pay/.pool`         | AMM pool info (query: `?a=&b=`)             |
//! | POST   | `/pay/.pool/swap`    | Execute an AMM constant-product swap        |
//! | POST   | `/pay/.pool/liquidity` | Add liquidity to an AMM pool              |
//! | GET    | `/pay/{resource}`    | Payment-gated resource access               |
//!
//! ## Storage
//!
//! `FsPaymentStore` persists every identity's balance in one Web Ledger
//! document, `{ledger_dir}/ledger.json`. File-level locking via `flock(2)`
//! makes each access-fee charge atomic on POSIX systems. Balances rise only
//! through a deposit receipt (`WebLedger::credit_by_outpoint`).
//!
//! @see <https://webledgers.org>
//! @see `solid_pod_rs::payments` for upstream types
//!
//! ## Reference
//!
//! The HTTP 402 pattern and PaymentCondition ACL model originate from the
//! JavaScript Solid Server. See Melvin Carvalho's *Practical Guide to Solid*
//! for the reference walkthrough: <https://melvin.me/public/solid/>
//!
//! Parts 3-4 cover the 402 flow and per-read micropayments. Parts 6-7 cover
//! blocktrails and MRC20 tokens. This handler extends the pattern with
//! per-endpoint GPU cost tables for the VisionClaw actor mesh.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};

use solid_pod_rs::payments::{
    balance_response, pay_info, payment_required_body, payment_response_headers, pubkey_to_did,
    PayConfig as UpstreamPayConfig, PaymentError, WebLedger,
};
use solid_pod_rs::trading::Exchange;

// P2-06: File-level advisory locking for cross-process safety.
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

// ---------------------------------------------------------------------------
// PayConfig — local config loaded from env / config.yml
// ---------------------------------------------------------------------------

/// VisionClaw payment configuration.
///
/// Loaded from environment variables at startup:
/// - `PAY_ENABLED` (bool, default `false`)
/// - `PAY_COST_SATS` (u64, default `1`)
/// - `PAY_LEDGER_DIR` (path, default `./data/ledger`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcPayConfig {
    pub enabled: bool,
    pub cost_sats: u64,
    pub ledger_dir: PathBuf,
    pub inference_cost_sats: u64,
    pub image_gen_cost_sats: u64,
    pub analytics_cost_sats: u64,
}

impl Default for VcPayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cost_sats: 1,
            ledger_dir: PathBuf::from("./data/ledger"),
            inference_cost_sats: 10,
            image_gen_cost_sats: 100,
            analytics_cost_sats: 5,
        }
    }
}

impl VcPayConfig {
    /// Load payment configuration from environment variables.
    pub fn from_env() -> Self {
        let enabled = std::env::var("PAY_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let cost_sats = std::env::var("PAY_COST_SATS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let ledger_dir = std::env::var("PAY_LEDGER_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data/ledger"));

        let inference_cost_sats = std::env::var("PAY_INFERENCE_COST_SATS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(cost_sats * 10);
        let image_gen_cost_sats = std::env::var("PAY_IMAGE_GEN_COST_SATS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(cost_sats * 100);
        let analytics_cost_sats = std::env::var("PAY_ANALYTICS_COST_SATS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(cost_sats * 5);

        Self {
            enabled,
            cost_sats,
            ledger_dir,
            inference_cost_sats,
            image_gen_cost_sats,
            analytics_cost_sats,
        }
    }

    /// Look up the sat cost for a given endpoint path.
    pub fn cost_for_endpoint(&self, endpoint: &str) -> u64 {
        if endpoint.starts_with("/api/inference/") {
            self.inference_cost_sats
        } else if endpoint.starts_with("/api/image-gen/") {
            self.image_gen_cost_sats
        } else if endpoint.starts_with("/api/analytics/") {
            self.analytics_cost_sats
        } else {
            self.cost_sats
        }
    }

    /// Convert to the upstream `PayConfig` for interop with `solid-pod-rs`
    /// helper functions (`pay_info`, `balance_response`, etc.).
    pub fn to_upstream(&self) -> UpstreamPayConfig {
        UpstreamPayConfig {
            enabled: self.enabled,
            cost_sats: self.cost_sats,
            token: None,
            chains: Vec::new(),
        }
    }

    /// P2-05: Build a JSON map of per-endpoint GPU cost tiers suitable for
    /// inclusion in upstream request headers or the `/pay/.info` response.
    ///
    /// Tier multipliers relative to `cost_sats`:
    /// - inference = 10x base rate
    /// - image-gen = 100x base rate
    /// - analytics = 5x base rate
    pub fn cost_tiers_json(&self) -> serde_json::Value {
        serde_json::json!({
            "default_sats": self.cost_sats,
            "tiers": {
                "inference": {
                    "cost_sats": self.inference_cost_sats,
                    "multiplier": self.inference_cost_sats.checked_div(self.cost_sats).unwrap_or(10),
                    "endpoints": ["/api/inference/*"]
                },
                "image-gen": {
                    "cost_sats": self.image_gen_cost_sats,
                    "multiplier": self.image_gen_cost_sats.checked_div(self.cost_sats).unwrap_or(100),
                    "endpoints": ["/api/image-gen/*"]
                },
                "analytics": {
                    "cost_sats": self.analytics_cost_sats,
                    "multiplier": self.analytics_cost_sats.checked_div(self.cost_sats).unwrap_or(5),
                    "endpoints": ["/api/analytics/*"]
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// FsPaymentStore — filesystem-backed ledger with file locking
// ---------------------------------------------------------------------------

/// Filesystem-backed payment store using a single `WebLedger` JSON file.
///
/// The ledger lives at `{ledger_dir}/ledger.json`. Mutations acquire BOTH a
/// `tokio::sync::Mutex` (in-process serialization) AND an advisory file lock
/// via `flock(2)` on `{ledger_dir}/ledger.lock` (cross-process safety, P2-06).
///
/// The file lock prevents concurrent writes from multiple server processes
/// sharing the same ledger directory (e.g. blue-green deployments, multiple
/// workers). The in-process mutex prevents the tokio task from holding the
/// file lock across an await point.
pub struct FsPaymentStore {
    ledger_path: PathBuf,
    lock_path: PathBuf,
    pub(crate) lock: tokio::sync::Mutex<()>,
}

/// RAII guard for an advisory file lock. Releases on drop.
#[cfg(unix)]
struct FileLockGuard {
    file: std::fs::File,
}

#[cfg(unix)]
impl FileLockGuard {
    /// Acquire an exclusive advisory lock on the given path. Creates the
    /// lock file if it does not exist.
    fn lock_exclusive(path: &Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        // SAFETY: flock operates on a valid fd from an open File.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { file })
    }
}

#[cfg(unix)]
impl Drop for FileLockGuard {
    fn drop(&mut self) {
        // Best-effort unlock; failure here is benign (fd close also releases).
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

impl FsPaymentStore {
    /// Create a new store, ensuring the ledger directory exists.
    pub fn new(ledger_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(ledger_dir)?;
        let ledger_path = ledger_dir.join("ledger.json");
        let lock_path = ledger_dir.join("ledger.lock");
        Ok(Self {
            ledger_path,
            lock_path,
            lock: tokio::sync::Mutex::new(()),
        })
    }

    #[cfg(unix)]
    pub(crate) fn acquire_file_lock(&self) -> Result<FileLockGuard, PaymentError> {
        FileLockGuard::lock_exclusive(&self.lock_path)
            .map_err(|e| PaymentError::Store(format!("flock: {e}")))
    }

    pub(crate) async fn read_ledger(&self) -> WebLedger {
        match tokio::fs::read_to_string(&self.ledger_path).await {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
                warn!(
                    "[pay] ledger parse error at {}: {e} — starting fresh",
                    self.ledger_path.display()
                );
                WebLedger::new("VisionClaw Credits")
            }),
            Err(_) => WebLedger::new("VisionClaw Credits"),
        }
    }

    pub(crate) async fn write_ledger(&self, ledger: &WebLedger) -> Result<(), PaymentError> {
        let json = serde_json::to_string_pretty(ledger)
            .map_err(|e| PaymentError::Store(format!("serialize: {e}")))?;

        let tmp_path = self.ledger_path.with_extension("json.tmp");
        tokio::fs::write(&tmp_path, json.as_bytes())
            .await
            .map_err(|e| PaymentError::Store(format!("write tmp: {e}")))?;
        tokio::fs::rename(&tmp_path, &self.ledger_path)
            .await
            .map_err(|e| PaymentError::Store(format!("rename: {e}")))?;
        Ok(())
    }

    /// Get the balance for a `did:nostr:<hex>` identity.
    pub async fn get_balance(&self, did: &str) -> u64 {
        let ledger = self.read_ledger().await;
        ledger.get_balance(did)
    }

    /// Atomically charge an access fee. Returns the new balance on success,
    /// or `PaymentError::InsufficientBalance` if the account cannot cover
    /// the cost.
    ///
    /// The ledger only moves balances through receipts (solid-pod-rs
    /// 0.5.0-alpha.11): a fee is a [`WebLedger::charge`]. Nothing here
    /// credits an account; balances arrive through a recorded deposit.
    ///
    /// Acquires both the tokio mutex (in-process) and an advisory flock
    /// (cross-process) before touching the ledger.
    pub async fn debit(&self, did: &str, amount: u64) -> Result<u64, PaymentError> {
        let _guard = self.lock.lock().await;
        #[cfg(unix)]
        let _flock = self.acquire_file_lock()?;
        let mut ledger = self.read_ledger().await;
        ledger.charge(did, amount)?;
        self.write_ledger(&ledger).await?;
        Ok(ledger.get_balance(did))
    }
}

// ---------------------------------------------------------------------------
// FsExchangeStore — filesystem-backed order book + AMM pool persistence
// ---------------------------------------------------------------------------

/// Filesystem-backed exchange store (order book + AMM pools).
///
/// Persisted at `{ledger_dir}/exchange.json`, uses the same dual-lock pattern
/// as `FsPaymentStore`.
pub struct FsExchangeStore {
    exchange_path: PathBuf,
    lock_path: PathBuf,
    lock: tokio::sync::Mutex<()>,
}

impl FsExchangeStore {
    pub fn new(ledger_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(ledger_dir)?;
        Ok(Self {
            exchange_path: ledger_dir.join("exchange.json"),
            lock_path: ledger_dir.join("exchange.lock"),
            lock: tokio::sync::Mutex::new(()),
        })
    }

    #[cfg(unix)]
    fn acquire_file_lock(&self) -> Result<FileLockGuard, PaymentError> {
        FileLockGuard::lock_exclusive(&self.lock_path)
            .map_err(|e| PaymentError::Store(format!("flock: {e}")))
    }

    async fn read_exchange(&self) -> Exchange {
        match tokio::fs::read_to_string(&self.exchange_path).await {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
                warn!("[pay] exchange parse error: {e} — starting fresh");
                Exchange::new()
            }),
            Err(_) => Exchange::new(),
        }
    }

    async fn write_exchange(&self, exchange: &Exchange) -> Result<(), PaymentError> {
        let json = serde_json::to_string_pretty(exchange)
            .map_err(|e| PaymentError::Store(format!("serialize: {e}")))?;
        let tmp = self.exchange_path.with_extension("json.tmp");
        tokio::fs::write(&tmp, json.as_bytes())
            .await
            .map_err(|e| PaymentError::Store(format!("write tmp: {e}")))?;
        tokio::fs::rename(&tmp, &self.exchange_path)
            .await
            .map_err(|e| PaymentError::Store(format!("rename: {e}")))?;
        Ok(())
    }

    /// Run a read-modify-write cycle on the exchange + ledger under locks.
    pub async fn with_exchange_and_ledger<F, T>(
        &self,
        ledger_store: &FsPaymentStore,
        f: F,
    ) -> Result<T, PaymentError>
    where
        F: FnOnce(&mut Exchange, &mut WebLedger) -> Result<T, PaymentError>,
    {
        let _guard = self.lock.lock().await;
        let _lguard = ledger_store.lock.lock().await;
        #[cfg(unix)]
        let _flock1 = self.acquire_file_lock()?;
        #[cfg(unix)]
        let _flock2 = ledger_store.acquire_file_lock()?;

        let mut exchange = self.read_exchange().await;
        let mut ledger = ledger_store.read_ledger().await;

        let result = f(&mut exchange, &mut ledger)?;

        self.write_exchange(&exchange).await?;
        ledger_store.write_ledger(&ledger).await?;
        Ok(result)
    }

    /// Read-only exchange access (no ledger needed).
    pub async fn read_only(&self) -> Exchange {
        self.read_exchange().await
    }
}

// ---------------------------------------------------------------------------
// NIP-98 auth extraction (thin wrapper matching solid_pod_handler pattern)
// ---------------------------------------------------------------------------

/// Extract the caller's hex pubkey from a NIP-98 `Authorization` header.
/// Returns `None` if the header is missing or verification fails.
///
/// The actual HTTP method is derived from the request so that POST endpoints
/// (e.g. `/pay/.deposit`, `/pay/.estimate`) verify correctly against the
/// method tag in the NIP-98 token.
async fn extract_caller_pubkey(req: &HttpRequest) -> Option<String> {
    let header = req
        .headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())?;

    let url = req.uri().to_string();
    let method = req.method().as_str();
    match solid_pod_rs::auth::nip98::verify(header, &url, method, None).await {
        Ok(pubkey) => Some(pubkey),
        Err(e) => {
            debug!("[pay] NIP-98 verify failed for {method} {url}: {e}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

/// `GET /pay/.info` — public endpoint, returns payment configuration.
///
/// P2-05: The response now includes per-endpoint GPU cost tiers so downstream
/// consumers (including solid-pod-rs) can discover tiered pricing without a
/// separate `/pay/.costs` call.
async fn pay_info_handler(config: web::Data<VcPayConfig>) -> HttpResponse {
    let upstream = config.to_upstream();
    let mut info = pay_info(&upstream);
    // Augment with VisionClaw-specific fields
    info["enabled"] = serde_json::json!(config.enabled);
    info["methods"] = serde_json::json!(["lightning"]);
    // P2-05: propagate per-endpoint GPU cost tiers
    info["cost_tiers"] = config.cost_tiers_json();
    HttpResponse::Ok().json(info)
}

/// `GET /pay/.balance` — requires NIP-98 auth, returns caller's balance.
async fn pay_balance_handler(
    req: HttpRequest,
    config: web::Data<VcPayConfig>,
    store: web::Data<Arc<FsPaymentStore>>,
) -> HttpResponse {
    if !config.enabled {
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "Payment system is disabled"
        }));
    }

    let pubkey = match extract_caller_pubkey(&req).await {
        Some(pk) => pk,
        None => {
            return HttpResponse::Unauthorized().json(serde_json::json!({
                "error": "Authentication required — provide a NIP-98 Authorization header"
            }));
        }
    };

    let did = pubkey_to_did(&pubkey);
    let balance = store.get_balance(&did).await;
    let body = balance_response(&did, balance, config.cost_sats);
    HttpResponse::Ok().json(body)
}

/// `POST /pay/.deposit` — stub: manual funding instructions.
async fn pay_deposit_handler(config: web::Data<VcPayConfig>) -> HttpResponse {
    if !config.enabled {
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "Payment system is disabled"
        }));
    }

    HttpResponse::NotImplemented().json(serde_json::json!({
        "error": "Deposit not yet available via API",
        "message": "Contact the server operator for manual funding. \
                    Lightning deposit support is planned for a future release.",
        "spec": "https://webledgers.org"
    }))
}

/// `GET /pay/{resource_path}` — payment-gated resource access.
///
/// Flow:
/// 1. Authenticate caller via NIP-98.
/// 2. Check balance >= cost_sats.
/// 3. Debit the cost.
/// 4. Return a JSON receipt with payment headers (`X-Balance`, `X-Cost`).
///
/// The actual resource proxying is a stub: in a full deployment this would
/// forward the request to the underlying resource handler. For now it
/// returns a JSON receipt confirming the charge.
async fn pay_resource_handler(
    req: HttpRequest,
    path: web::Path<String>,
    config: web::Data<VcPayConfig>,
    store: web::Data<Arc<FsPaymentStore>>,
) -> HttpResponse {
    if !config.enabled {
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "Payment system is disabled"
        }));
    }

    let pubkey = match extract_caller_pubkey(&req).await {
        Some(pk) => pk,
        None => {
            return HttpResponse::Unauthorized().json(serde_json::json!({
                "error": "Authentication required — provide a NIP-98 Authorization header"
            }));
        }
    };

    let did = pubkey_to_did(&pubkey);
    let resource = path.into_inner();
    // P2-05: Use per-endpoint cost tier instead of flat cost_sats
    let endpoint_path = format!("/{resource}");
    let cost = config.cost_for_endpoint(&endpoint_path);

    match store.debit(&did, cost).await {
        Ok(remaining) => {
            let headers = payment_response_headers(remaining, cost, "sat");
            let body = serde_json::json!({
                "resource": resource,
                "charged": cost,
                "balance": remaining,
                "unit": "sat"
            });
            let mut resp = HttpResponse::Ok().json(body);
            for (name, value) in headers {
                if let Ok(hv) = actix_web::http::header::HeaderValue::from_str(&value) {
                    resp.headers_mut()
                        .insert(actix_web::http::header::HeaderName::from_static(name), hv);
                }
            }
            resp
        }
        Err(PaymentError::InsufficientBalance { balance, cost }) => {
            let body = payment_required_body(balance, cost);
            HttpResponse::build(actix_web::http::StatusCode::PAYMENT_REQUIRED).json(body)
        }
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": format!("Payment store error: {e}")
        })),
    }
}

// ---------------------------------------------------------------------------
// Trading endpoints (sell / swap / offers / pool)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SellRequest {
    sell_currency: String,
    sell_amount: u64,
    buy_currency: String,
    price: u64,
}

/// `POST /pay/.sell` — create a sell order on the order book.
async fn pay_sell_handler(
    req: HttpRequest,
    body: web::Json<SellRequest>,
    config: web::Data<VcPayConfig>,
    ledger_store: web::Data<Arc<FsPaymentStore>>,
    exchange_store: web::Data<Arc<FsExchangeStore>>,
) -> HttpResponse {
    if !config.enabled {
        return HttpResponse::Forbidden()
            .json(serde_json::json!({"error": "Payment system is disabled"}));
    }
    let pubkey = match extract_caller_pubkey(&req).await {
        Some(pk) => pk,
        None => {
            return HttpResponse::Unauthorized()
                .json(serde_json::json!({"error": "Authentication required"}))
        }
    };
    let did = pubkey_to_did(&pubkey);
    let b = body.into_inner();

    match exchange_store
        .with_exchange_and_ledger(&ledger_store, |exchange, _ledger| {
            let order = exchange.order_book.create_order(
                &did,
                &b.sell_currency,
                b.sell_amount,
                &b.buy_currency,
                b.price,
            );
            Ok(order)
        })
        .await
    {
        Ok(order) => HttpResponse::Created().json(serde_json::json!({
            "order": order,
        })),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({"error": format!("{e}")})),
    }
}

/// `GET /pay/.offers` — list active sell orders.
async fn pay_offers_handler(
    req: HttpRequest,
    config: web::Data<VcPayConfig>,
    exchange_store: web::Data<Arc<FsExchangeStore>>,
    query: web::Query<HashMap<String, String>>,
) -> HttpResponse {
    if !config.enabled {
        return HttpResponse::Forbidden()
            .json(serde_json::json!({"error": "Payment system is disabled"}));
    }
    // Order-book state is non-public exchange data; require an authenticated
    // caller, matching the write-side .sell/.swap handlers in this file.
    if extract_caller_pubkey(&req).await.is_none() {
        return HttpResponse::Unauthorized()
            .json(serde_json::json!({"error": "Authentication required"}));
    }
    let exchange = exchange_store.read_only().await;
    let pair = query
        .get("sell")
        .and_then(|s| query.get("buy").map(|b| (s.as_str(), b.as_str())));
    let offers: Vec<_> = exchange
        .order_book
        .list_offers(pair)
        .into_iter()
        .cloned()
        .collect();
    HttpResponse::Ok().json(serde_json::json!({"offers": offers, "count": offers.len()}))
}

#[derive(Debug, Deserialize)]
struct SwapRequest {
    order_id: String,
}

/// `POST /pay/.swap` — execute a trade against an existing sell order.
async fn pay_swap_handler(
    req: HttpRequest,
    body: web::Json<SwapRequest>,
    config: web::Data<VcPayConfig>,
    ledger_store: web::Data<Arc<FsPaymentStore>>,
    exchange_store: web::Data<Arc<FsExchangeStore>>,
) -> HttpResponse {
    if !config.enabled {
        return HttpResponse::Forbidden()
            .json(serde_json::json!({"error": "Payment system is disabled"}));
    }
    let pubkey = match extract_caller_pubkey(&req).await {
        Some(pk) => pk,
        None => {
            return HttpResponse::Unauthorized()
                .json(serde_json::json!({"error": "Authentication required"}))
        }
    };
    let did = pubkey_to_did(&pubkey);
    let order_id = body.into_inner().order_id;

    match exchange_store
        .with_exchange_and_ledger(&ledger_store, |exchange, ledger| {
            exchange.order_book.execute_swap(&order_id, &did, ledger)
        })
        .await
    {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            let status = match &e {
                PaymentError::InsufficientBalance { .. } => {
                    actix_web::http::StatusCode::PAYMENT_REQUIRED
                }
                _ => actix_web::http::StatusCode::BAD_REQUEST,
            };
            HttpResponse::build(status).json(serde_json::json!({"error": format!("{e}")}))
        }
    }
}

#[derive(Debug, Deserialize)]
struct PoolSwapRequest {
    from_currency: String,
    amount: u64,
}

#[derive(Debug, Deserialize)]
struct AddLiquidityRequest {
    currency_a: String,
    currency_b: String,
    amount_a: u64,
    amount_b: u64,
    #[serde(default = "default_fee_bps")]
    fee_bps: u64,
}

fn default_fee_bps() -> u64 {
    30
}

/// `GET /pay/.pool?a={}&b={}` — pool info for a currency pair.
/// `POST /pay/.pool` with body `{"from_currency": ..., "amount": ...}` — AMM swap.
async fn pay_pool_get_handler(
    req: HttpRequest,
    config: web::Data<VcPayConfig>,
    exchange_store: web::Data<Arc<FsExchangeStore>>,
    query: web::Query<HashMap<String, String>>,
) -> HttpResponse {
    if !config.enabled {
        return HttpResponse::Forbidden()
            .json(serde_json::json!({"error": "Payment system is disabled"}));
    }
    // AMM pool balances are non-public liquidity state; require an
    // authenticated caller, matching the .pool/swap write handler.
    if extract_caller_pubkey(&req).await.is_none() {
        return HttpResponse::Unauthorized()
            .json(serde_json::json!({"error": "Authentication required"}));
    }
    let exchange = exchange_store.read_only().await;
    match (query.get("a"), query.get("b")) {
        (Some(a), Some(b)) => match exchange.get_pool(a, b) {
            Some(pool) => HttpResponse::Ok().json(pool.pool_info()),
            None => HttpResponse::NotFound().json(serde_json::json!({"error": "Pool not found"})),
        },
        _ => {
            let pools: Vec<_> = exchange.pools.values().map(|p| p.pool_info()).collect();
            HttpResponse::Ok().json(serde_json::json!({"pools": pools}))
        }
    }
}

/// `POST /pay/.pool/swap` — execute an AMM swap.
async fn pay_pool_swap_handler(
    req: HttpRequest,
    body: web::Json<PoolSwapRequest>,
    config: web::Data<VcPayConfig>,
    ledger_store: web::Data<Arc<FsPaymentStore>>,
    exchange_store: web::Data<Arc<FsExchangeStore>>,
    query: web::Query<HashMap<String, String>>,
) -> HttpResponse {
    if !config.enabled {
        return HttpResponse::Forbidden()
            .json(serde_json::json!({"error": "Payment system is disabled"}));
    }
    let pubkey = match extract_caller_pubkey(&req).await {
        Some(pk) => pk,
        None => {
            return HttpResponse::Unauthorized()
                .json(serde_json::json!({"error": "Authentication required"}))
        }
    };
    let did = pubkey_to_did(&pubkey);
    let b = body.into_inner();
    let currency_a = query.get("a").cloned().unwrap_or_default();
    let currency_b = query.get("b").cloned().unwrap_or_default();

    match exchange_store
        .with_exchange_and_ledger(&ledger_store, |exchange, ledger| {
            let pool = exchange.get_or_create_pool(&currency_a, &currency_b, 30);
            pool.swap(&did, &b.from_currency, b.amount, ledger)
        })
        .await
    {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            let status = match &e {
                PaymentError::InsufficientBalance { .. } => {
                    actix_web::http::StatusCode::PAYMENT_REQUIRED
                }
                _ => actix_web::http::StatusCode::BAD_REQUEST,
            };
            HttpResponse::build(status).json(serde_json::json!({"error": format!("{e}")}))
        }
    }
}

/// `POST /pay/.pool/liquidity` — add liquidity to an AMM pool.
async fn pay_pool_liquidity_handler(
    req: HttpRequest,
    body: web::Json<AddLiquidityRequest>,
    config: web::Data<VcPayConfig>,
    ledger_store: web::Data<Arc<FsPaymentStore>>,
    exchange_store: web::Data<Arc<FsExchangeStore>>,
) -> HttpResponse {
    if !config.enabled {
        return HttpResponse::Forbidden()
            .json(serde_json::json!({"error": "Payment system is disabled"}));
    }
    let pubkey = match extract_caller_pubkey(&req).await {
        Some(pk) => pk,
        None => {
            return HttpResponse::Unauthorized()
                .json(serde_json::json!({"error": "Authentication required"}))
        }
    };
    let did = pubkey_to_did(&pubkey);
    let b = body.into_inner();

    match exchange_store
        .with_exchange_and_ledger(&ledger_store, |exchange, ledger| {
            let pool = exchange.get_or_create_pool(&b.currency_a, &b.currency_b, b.fee_bps);
            let shares = pool.add_liquidity(&did, b.amount_a, b.amount_b, ledger)?;
            Ok(serde_json::json!({
                "shares_issued": shares,
                "pool": pool.pool_info(),
            }))
        })
        .await
    {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({"error": format!("{e}")})),
    }
}

// ---------------------------------------------------------------------------
// Route configuration
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Agent job estimation endpoint
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct EstimateRequest {
    endpoint: String,
}

async fn pay_estimate_handler(
    req: HttpRequest,
    body: web::Json<EstimateRequest>,
    config: web::Data<VcPayConfig>,
) -> HttpResponse {
    if !config.enabled {
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "Payment system is disabled"
        }));
    }

    let pubkey = match extract_caller_pubkey(&req).await {
        Some(pk) => pk,
        None => {
            return HttpResponse::Unauthorized().json(serde_json::json!({
                "error": "Authentication required"
            }));
        }
    };

    let did = pubkey_to_did(&pubkey);
    let estimated_sats = config.cost_for_endpoint(&body.endpoint);

    HttpResponse::Ok().json(serde_json::json!({
        "did": did,
        "endpoint": body.endpoint,
        "estimated_sats": estimated_sats,
        "unit": "sat",
        "note": "Pre-execution estimate. GPU-metered endpoints may settle at actual cost."
    }))
}

// ---------------------------------------------------------------------------
// Cost table endpoint (public — no auth required)
// ---------------------------------------------------------------------------

async fn pay_cost_table_handler(config: web::Data<VcPayConfig>) -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "default": config.cost_sats,
        "endpoints": {
            "/api/inference/*": config.inference_cost_sats,
            "/api/image-gen/*": config.image_gen_cost_sats,
            "/api/analytics/*": config.analytics_cost_sats,
        },
        "unit": "sat",
        "note": "GPU-metered endpoints may settle at actual cost after execution."
    }))
}

/// Mount all `/pay/*` routes. Called from `main.rs` via
/// `.configure(configure_pay_routes)`.
///
/// When `PAY_ENABLED=false` the routes still mount but `.info` reports
/// `enabled: false` and gated endpoints return 403.
pub fn configure_pay_routes(cfg: &mut web::ServiceConfig) {
    info!("=== REGISTERING PAYMENT ROUTES (/pay/*) ===");
    cfg.service(
        web::scope("/pay")
            .route("/.info", web::get().to(pay_info_handler))
            .route("/.balance", web::get().to(pay_balance_handler))
            .route("/.deposit", web::post().to(pay_deposit_handler))
            .route("/.estimate", web::post().to(pay_estimate_handler))
            .route("/.costs", web::get().to(pay_cost_table_handler))
            .route("/.sell", web::post().to(pay_sell_handler))
            .route("/.offers", web::get().to(pay_offers_handler))
            .route("/.swap", web::post().to(pay_swap_handler))
            .route("/.pool", web::get().to(pay_pool_get_handler))
            .route("/.pool/swap", web::post().to(pay_pool_swap_handler))
            .route(
                "/.pool/liquidity",
                web::post().to(pay_pool_liquidity_handler),
            )
            .route("/{resource_path:.*}", web::get().to(pay_resource_handler)),
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use solid_pod_rs::payments::ReceiptOutcome;
    use tempfile::TempDir;

    #[test]
    fn pay_config_defaults() {
        let config = VcPayConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.cost_sats, 1);
        assert_eq!(config.ledger_dir, PathBuf::from("./data/ledger"));
        assert_eq!(config.inference_cost_sats, 10);
        assert_eq!(config.image_gen_cost_sats, 100);
        assert_eq!(config.analytics_cost_sats, 5);
    }

    #[test]
    fn cost_for_endpoint_inference() {
        let config = VcPayConfig::default();
        assert_eq!(config.cost_for_endpoint("/api/inference/run"), 10);
        assert_eq!(config.cost_for_endpoint("/api/inference/batch"), 10);
    }

    #[test]
    fn cost_for_endpoint_image_gen() {
        let config = VcPayConfig::default();
        assert_eq!(config.cost_for_endpoint("/api/image-gen/submit"), 100);
        assert_eq!(config.cost_for_endpoint("/api/image-gen/agent-submit"), 100);
    }

    #[test]
    fn cost_for_endpoint_analytics() {
        let config = VcPayConfig::default();
        assert_eq!(config.cost_for_endpoint("/api/analytics/pagerank"), 5);
    }

    #[test]
    fn cost_for_endpoint_default() {
        let config = VcPayConfig::default();
        assert_eq!(config.cost_for_endpoint("/api/health"), 1);
        assert_eq!(config.cost_for_endpoint("/pay/.info"), 1);
    }

    #[test]
    fn pay_config_from_env() {
        // Save originals
        let orig_enabled = std::env::var("PAY_ENABLED").ok();
        let orig_cost = std::env::var("PAY_COST_SATS").ok();
        let orig_dir = std::env::var("PAY_LEDGER_DIR").ok();

        std::env::set_var("PAY_ENABLED", "true");
        std::env::set_var("PAY_COST_SATS", "42");
        std::env::set_var("PAY_LEDGER_DIR", "/tmp/test-ledger");

        let config = VcPayConfig::from_env();
        assert!(config.enabled);
        assert_eq!(config.cost_sats, 42);
        assert_eq!(config.ledger_dir, PathBuf::from("/tmp/test-ledger"));

        // Restore
        match orig_enabled {
            Some(v) => std::env::set_var("PAY_ENABLED", v),
            None => std::env::remove_var("PAY_ENABLED"),
        }
        match orig_cost {
            Some(v) => std::env::set_var("PAY_COST_SATS", v),
            None => std::env::remove_var("PAY_COST_SATS"),
        }
        match orig_dir {
            Some(v) => std::env::set_var("PAY_LEDGER_DIR", v),
            None => std::env::remove_var("PAY_LEDGER_DIR"),
        }
    }

    #[test]
    fn upstream_config_conversion() {
        let config = VcPayConfig {
            enabled: true,
            cost_sats: 10,
            ledger_dir: PathBuf::from("/tmp"),
            inference_cost_sats: 100,
            image_gen_cost_sats: 1000,
            analytics_cost_sats: 50,
        };
        let upstream = config.to_upstream();
        assert!(upstream.enabled);
        assert_eq!(upstream.cost_sats, 10);
        assert!(upstream.token.is_none());
        assert!(upstream.chains.is_empty());
    }

    /// A 64-hex account, as `pubkey_to_did` yields for a NIP-98 caller.
    fn account(c: char) -> String {
        format!("did:nostr:{}", c.to_string().repeat(64))
    }

    /// Fund `did` the only way the ledger allows: a deposit receipt keyed by
    /// the outpoint `<n repeated>:0`.
    async fn deposit(store: &FsPaymentStore, did: &str, n: u8, amount: u64) -> ReceiptOutcome {
        let mut ledger = store.read_ledger().await;
        let txid = format!("{n:02x}").repeat(32);
        let outcome = ledger
            .credit_by_outpoint(did, "satoshi", &txid, 0, amount)
            .unwrap();
        store.write_ledger(&ledger).await.unwrap();
        outcome
    }

    #[tokio::test]
    async fn fs_store_deposit_and_balance() {
        let tmp = TempDir::new().unwrap();
        let store = FsPaymentStore::new(tmp.path()).unwrap();
        let did = account('a');

        assert_eq!(store.get_balance(&did).await, 0);

        assert_eq!(deposit(&store, &did, 1, 100).await, ReceiptOutcome::Applied);
        assert_eq!(store.get_balance(&did).await, 100);

        assert_eq!(deposit(&store, &did, 2, 50).await, ReceiptOutcome::Applied);
        assert_eq!(store.get_balance(&did).await, 150);

        // The same outpoint never pays twice.
        assert_eq!(
            deposit(&store, &did, 2, 50).await,
            ReceiptOutcome::AlreadyApplied
        );
        assert_eq!(store.get_balance(&did).await, 150);
    }

    #[tokio::test]
    async fn fs_store_debit_success() {
        let tmp = TempDir::new().unwrap();
        let store = FsPaymentStore::new(tmp.path()).unwrap();
        let did = account('a');

        deposit(&store, &did, 1, 200).await;
        let remaining = store.debit(&did, 50).await.unwrap();
        assert_eq!(remaining, 150);
        assert_eq!(store.get_balance(&did).await, 150);
    }

    #[tokio::test]
    async fn fs_store_debit_insufficient() {
        let tmp = TempDir::new().unwrap();
        let store = FsPaymentStore::new(tmp.path()).unwrap();
        let did = account('a');

        deposit(&store, &did, 1, 10).await;
        let err = store.debit(&did, 100).await.unwrap_err();
        assert!(matches!(
            err,
            PaymentError::InsufficientBalance {
                balance: 10,
                cost: 100
            }
        ));
        assert_eq!(store.get_balance(&did).await, 10);
    }

    #[tokio::test]
    async fn fs_store_debit_unknown_did() {
        let tmp = TempDir::new().unwrap();
        let store = FsPaymentStore::new(tmp.path()).unwrap();

        let err = store.debit(&account('f'), 1).await.unwrap_err();
        assert!(matches!(
            err,
            PaymentError::InsufficientBalance {
                balance: 0,
                cost: 1
            }
        ));
    }

    #[tokio::test]
    async fn fs_store_persistence() {
        let tmp = TempDir::new().unwrap();
        let did = account('c');

        // Write with one store instance
        {
            let store = FsPaymentStore::new(tmp.path()).unwrap();
            deposit(&store, &did, 1, 500).await;
            store.debit(&did, 20).await.unwrap();
        }

        // Read with a new store instance
        {
            let store = FsPaymentStore::new(tmp.path()).unwrap();
            assert_eq!(store.get_balance(&did).await, 480);
        }
    }

    #[tokio::test]
    async fn fs_store_multiple_identities() {
        let tmp = TempDir::new().unwrap();
        let store = FsPaymentStore::new(tmp.path()).unwrap();

        let alice = account('a');
        let bob = account('b');

        deposit(&store, &alice, 1, 100).await;
        deposit(&store, &bob, 2, 200).await;
        store.debit(&alice, 30).await.unwrap();

        assert_eq!(store.get_balance(&alice).await, 70);
        assert_eq!(store.get_balance(&bob).await, 200);
    }

    #[test]
    fn info_endpoint_shape() {
        let config = VcPayConfig {
            enabled: true,
            cost_sats: 5,
            ledger_dir: PathBuf::from("/tmp"),
            inference_cost_sats: 50,
            image_gen_cost_sats: 500,
            analytics_cost_sats: 25,
        };
        let upstream = config.to_upstream();
        let info = pay_info(&upstream);
        assert_eq!(info["cost"], 5);
        assert_eq!(info["unit"], "sat");
    }
}
