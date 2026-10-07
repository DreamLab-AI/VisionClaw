//! Converged `urn:visionclaw` identifier minter (BC20 counterpart).
//!
//! This is the VisionClaw-side definition of the converged URN grammar whose
//! agentbox counterpart is `management-api/lib/bc20-provenance-bridge.js` +
//! `management-api/lib/uris.js`. Until this module merges to `main`, that
//! agentbox bridge was the *only* executable definition of the cross-namespace
//! contract — VisionClaw `main` still carries the legacy `urn:ngm:*` scheme,
//! which is left intact here to coexist (no rip-out yet).
//!
//! Grammar (per agentbox/CLAUDE.md "Parallel namespace"):
//!
//!   * `urn:visionclaw:concept:<domain>:<slug>`
//!     domain-scoped — a post-elevation shared ontology class.
//!   * `urn:visionclaw:kg:<hex-pubkey>:<sha256-12>`
//!     owner-scoped, content-addressed — a personal KG node.
//!   * `urn:visionclaw:bead:<hex-pubkey>:<sha256-12>`
//!     owner-scoped, content-addressed.
//!   * `urn:visionclaw:execution:<sha256-12>`
//!     content-addressed, **unscoped** — owner travels in `owner_did`.
//!   * `urn:visionclaw:group:<team>#members`
//!     team-scoped.
//!   * `urn:visionclaw:room:<sha256-12>`
//!     content-addressed, unscoped — an XR presence room (DDD-XR §7.2).
//!   * `urn:visionclaw:avatar:<hex-pubkey>`
//!     identity-bound 1:1 with the avatar's DID (DDD-XR §7.2).
//!   * identity is `did:nostr:<hex-pubkey>` — there is **no** `urn:visionclaw:agent`
//!     kind; an agent's identity *is* its DID.
//!
//! Conventions shared with the agentbox side:
//!   * content addressing → `sha256-12-<12 lowercase hex chars>`.
//!   * owner scope → the 64-char BIP-340 x-only hex pubkey (not bech32 npub).
//!
//! Discipline: every durable `urn:visionclaw` identifier MUST be minted through
//! the typed constructors below. Ad-hoc `format!()` construction is prohibited,
//! mirroring the `uris.js` mandate on the agentbox side.

use sha2::{Digest, Sha256};
use std::fmt;

/// URN namespace prefix for all converged VisionClaw identifiers.
pub const NS: &str = "urn:visionclaw";
/// Legacy URN namespace prefix (pre-convergence `urn:ngm:*`). New identifiers are
/// never minted under this prefix, but persisted IDs that predate the cutover must
/// keep resolving — see [`parse_dual`] and ADR-105 (urn convergence + ngm cutover).
pub const LEGACY_NGM_NS: &str = "urn:ngm";
/// DID method used for sovereign identity (shared with the VisionClaw substrate).
pub const DID_NOSTR_PREFIX: &str = "did:nostr:";
/// Content-address prefix (`sha256-12-<12 hex>`).
pub const CONTENT_ADDR_PREFIX: &str = "sha256-12-";

/// The set of converged URN kinds. There is deliberately NO `Agent` kind:
/// identity is `did:nostr:<pubkey>`, minted via [`did_nostr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `concept:<domain>:<slug>` — domain-scoped shared ontology class.
    Concept,
    /// `kg:<hex-pubkey>:<sha256-12>` — owner-scoped, content-addressed KG node.
    Kg,
    /// `bead:<hex-pubkey>:<sha256-12>` — owner-scoped, content-addressed.
    Bead,
    /// `execution:<sha256-12>` — content-addressed, unscoped.
    Execution,
    /// `group:<team>#members` — team-scoped.
    Group,
    /// `room:<sha256-12>` — content-addressed, unscoped XR presence room.
    Room,
    /// `avatar:<hex-pubkey>` — identity-bound 1:1 with the avatar's DID.
    Avatar,
}

impl Kind {
    /// The wire token for this kind (the segment after `urn:visionclaw:`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Concept => "concept",
            Kind::Kg => "kg",
            Kind::Bead => "bead",
            Kind::Execution => "execution",
            Kind::Group => "group",
            Kind::Room => "room",
            Kind::Avatar => "avatar",
        }
    }

    fn from_token(tok: &str) -> Option<Self> {
        Some(match tok {
            "concept" => Kind::Concept,
            "kg" => Kind::Kg,
            "bead" => Kind::Bead,
            "execution" => Kind::Execution,
            "group" => Kind::Group,
            "room" => Kind::Room,
            "avatar" => Kind::Avatar,
            _ => return None,
        })
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a mint or parse was rejected. Minting is fail-closed: a malformed input
/// yields an error rather than a structurally-invalid identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UriError {
    /// A 64-char lowercase-hex BIP-340 x-only pubkey was required.
    InvalidPubkey(String),
    /// An empty or whitespace-only required segment.
    EmptySegment(&'static str),
    /// Input was not a recognised `urn:visionclaw` identifier.
    NotVisionclaw(String),
    /// The kind token was not one of the converged kinds.
    UnknownKind(String),
    /// The identifier was the right kind but structurally malformed.
    Malformed(String),
}

impl fmt::Display for UriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UriError::InvalidPubkey(s) => write!(f, "invalid 64-hex pubkey scope: {s}"),
            UriError::EmptySegment(s) => write!(f, "empty required segment: {s}"),
            UriError::NotVisionclaw(s) => write!(f, "not a urn:visionclaw identifier: {s}"),
            UriError::UnknownKind(s) => write!(f, "unknown urn:visionclaw kind: {s}"),
            UriError::Malformed(s) => write!(f, "malformed urn:visionclaw identifier: {s}"),
        }
    }
}

impl std::error::Error for UriError {}

/// True iff `s` is a 64-char lowercase-hex BIP-340 x-only pubkey.
pub fn is_pubkey_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The number of lowercase-hex characters in a `sha256-12-` address body:
/// the first 6 bytes of the SHA-256 digest, two nibbles each.
pub const CONTENT_ADDR_HEX_LEN: usize = 12;

/// Is `s` a well-formed content address — the `sha256-12-` prefix followed by
/// **exactly** [`CONTENT_ADDR_HEX_LEN`] lowercase hex characters and nothing
/// else?
///
/// ADR-2023: emitting a correct hash and validating an incoming address are
/// separate guarantees. [`content_address`] always produced a conforming
/// string, but the constructor and parser only checked the prefix, so
/// `sha256-12-`, `sha256-12-ZZZZ`, `sha256-12-` + 40 hex chars and an
/// upper-case digest all round-tripped as valid KG addresses. This is the
/// grammar both ends now enforce.
pub fn is_content_address(s: &str) -> bool {
    match s.strip_prefix(CONTENT_ADDR_PREFIX) {
        Some(body) => {
            body.len() == CONTENT_ADDR_HEX_LEN
                && body
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }
        None => false,
    }
}

/// Validate a content address, returning it unchanged or a precise error.
fn require_content_address(s: &str) -> Result<(), UriError> {
    if is_content_address(s) {
        Ok(())
    } else {
        Err(UriError::Malformed(s.to_string()))
    }
}

/// `sha256-12-<12 lowercase hex>` content address over `input` bytes.
/// Matches the agentbox `sha12()` helper byte-for-byte.
pub fn content_address(input: impl AsRef<[u8]>) -> String {
    let digest = Sha256::digest(input.as_ref());
    let mut hex = String::with_capacity(12);
    for b in digest.iter().take(6) {
        // each byte → two lowercase hex chars; 6 bytes = 12 chars.
        hex.push(nibble(b >> 4));
        hex.push(nibble(b & 0x0f));
    }
    format!("{CONTENT_ADDR_PREFIX}{hex}")
}

#[inline]
fn nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'a' + (n - 10)) as char,
    }
}

/// Lowercase, collapse non-`[a-z0-9._-]` runs to `-`, trim leading/trailing `-`.
/// Matches the agentbox `slugify()`.
pub fn slugify(s: &str) -> String {
    let lowered = s.trim().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut last_dash = false;
    for ch in lowered.chars() {
        let keep = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        if keep {
            out.push(ch);
            last_dash = ch == '-';
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

// ── Typed mint functions (one per kind) ──────────────────────────────────────

/// Mint identity: `did:nostr:<hex-pubkey>`. Not a `urn:visionclaw` kind.
pub fn did_nostr(pubkey: &str) -> Result<String, UriError> {
    if !is_pubkey_hex(pubkey) {
        return Err(UriError::InvalidPubkey(pubkey.to_string()));
    }
    Ok(format!("{DID_NOSTR_PREFIX}{pubkey}"))
}

/// Mint `urn:visionclaw:concept:<domain>:<slug>` (domain-scoped ontology class).
/// Both `domain` and `slug` are slugified.
pub fn concept(domain: &str, slug: &str) -> Result<String, UriError> {
    let d = slugify(domain);
    let s = slugify(slug);
    if d.is_empty() {
        return Err(UriError::EmptySegment("concept domain"));
    }
    if s.is_empty() {
        return Err(UriError::EmptySegment("concept slug"));
    }
    Ok(format!("{NS}:{}:{d}:{s}", Kind::Concept))
}

/// Mint `urn:visionclaw:kg:<hex-pubkey>:<sha256-12>` from owner + raw content.
pub fn kg(owner_pubkey: &str, content: impl AsRef<[u8]>) -> Result<String, UriError> {
    if !is_pubkey_hex(owner_pubkey) {
        return Err(UriError::InvalidPubkey(owner_pubkey.to_string()));
    }
    Ok(format!(
        "{NS}:{}:{owner_pubkey}:{}",
        Kind::Kg,
        content_address(content)
    ))
}

/// Mint `urn:visionclaw:kg:<hex-pubkey>:<sha256-12>` from an already-computed
/// content address (e.g. a value crossing the BC20 boundary).
pub fn kg_with_address(owner_pubkey: &str, content_addr: &str) -> Result<String, UriError> {
    if !is_pubkey_hex(owner_pubkey) {
        return Err(UriError::InvalidPubkey(owner_pubkey.to_string()));
    }
    // ADR-2023: the full grammar, not just the prefix.
    require_content_address(content_addr)?;
    Ok(format!("{NS}:{}:{owner_pubkey}:{content_addr}", Kind::Kg))
}

/// Mint `urn:visionclaw:bead:<hex-pubkey>:<sha256-12>` from owner + raw content.
pub fn bead(owner_pubkey: &str, content: impl AsRef<[u8]>) -> Result<String, UriError> {
    if !is_pubkey_hex(owner_pubkey) {
        return Err(UriError::InvalidPubkey(owner_pubkey.to_string()));
    }
    Ok(format!(
        "{NS}:{}:{owner_pubkey}:{}",
        Kind::Bead,
        content_address(content)
    ))
}

/// Mint `urn:visionclaw:bead:<hex-pubkey>:<sha256-12>` from an already-computed
/// content address (e.g. a value crossing the BC20 boundary). Mirrors
/// [`kg_with_address`]: agentbox `bead` locals are content-addressed
/// (`management-api/lib/uris.js` `KINDS.bead.contentAddressed = true`) in the
/// same `sha256-12-<12hex>` shape VisionClaw uses, so the crossing preserves
/// the existing address rather than re-hashing (see
/// `bc20-provenance-bridge.js::toVisionclaw`'s `bead` arm, "structural
/// pass-through").
pub fn bead_with_address(owner_pubkey: &str, content_addr: &str) -> Result<String, UriError> {
    if !is_pubkey_hex(owner_pubkey) {
        return Err(UriError::InvalidPubkey(owner_pubkey.to_string()));
    }
    require_content_address(content_addr)?;
    Ok(format!("{NS}:{}:{owner_pubkey}:{content_addr}", Kind::Bead))
}

/// Mint `urn:visionclaw:execution:<sha256-12>` (unscoped; owner in `owner_did`).
pub fn execution(content: impl AsRef<[u8]>) -> String {
    format!("{NS}:{}:{}", Kind::Execution, content_address(content))
}

/// Mint `urn:visionclaw:group:<team>#members` (team-scoped membership ref).
pub fn group_members(team: &str) -> Result<String, UriError> {
    let t = slugify(team);
    if t.is_empty() {
        return Err(UriError::EmptySegment("group team"));
    }
    Ok(format!("{NS}:{}:{t}#members", Kind::Group))
}

/// Mint `urn:visionclaw:room:<sha256-12>` (unscoped XR presence room) from raw
/// room-defining content (e.g. the room descriptor).
pub fn room(content: impl AsRef<[u8]>) -> String {
    format!("{NS}:{}:{}", Kind::Room, content_address(content))
}

/// Mint `urn:visionclaw:avatar:<hex-pubkey>` — bound 1:1 with the avatar's DID.
pub fn avatar(pubkey: &str) -> Result<String, UriError> {
    if !is_pubkey_hex(pubkey) {
        return Err(UriError::InvalidPubkey(pubkey.to_string()));
    }
    Ok(format!("{NS}:{}:{pubkey}", Kind::Avatar))
}

// ── Legacy `urn:ngm:*` identifiers (ADR-2021 recorded exception) ─────────────

/// Typed constructors and parsers for the pre-convergence `urn:ngm:*` graph
/// identifiers.
///
/// ADR-2021 requires every durable mint site to use a typed constructor **or**
/// record a deliberate exception. The graph repository's node and edge IRIs are
/// that exception: they are the on-disk identity of an existing Oxigraph
/// dataset, and ADR-105's no-rewrite named-graph policy forbids re-minting them
/// under `urn:visionclaw:*`. Rather than leave the mint as an inline `format!`
/// in the adapter — untyped, unpaired with a parser, and free to drift — the
/// scheme lives here, beside its parser, so mint and lookup cannot disagree.
///
/// **This module mints legacy identifiers on purpose. Nothing new should use
/// it.** New durable identifiers go through [`kg`], [`bead`], [`execution`] and
/// friends.
pub mod ngm {
    /// `urn:ngm:node:<u32>` — the canonical legacy node IRI prefix.
    pub const NODE_PREFIX: &str = "urn:ngm:node:";
    /// `urn:ngm:edge:<source>:<target>:<edge-id>` — the legacy edge IRI prefix.
    pub const EDGE_PREFIX: &str = "urn:ngm:edge:";

    /// `urn:ngm:class:<slug>` — the canonical legacy OWL class IRI prefix,
    /// re-exported with its constructor, parser and predicate.
    ///
    /// The definition lives in `visionclaw-domain` because the *adapters*
    /// crate mints class IRIs too and is **upstream** of this crate: a typed
    /// mint that only the server could reach would have left
    /// `oxigraph_ontology_repository.rs` formatting the scheme by hand, which
    /// is exactly the drift ADR-2095 closes. Re-exporting here keeps
    /// `uri::ngm::*` the single lookup point for the legacy scheme.
    pub use visionclaw_domain::uri::ngm::{class_iri, is_class_iri, parse_class_iri, CLASS_PREFIX};

    /// Mint the canonical legacy node IRI. The full 32-bit id (class bits
    /// included) is used so the IRI round-trips losslessly.
    pub fn node_iri(id: u32) -> String {
        format!("{NODE_PREFIX}{id}")
    }

    /// Parse a legacy node IRI back into its full `u32` id.
    ///
    /// Paired with [`node_iri`]: `parse_node_iri(&node_iri(x)) == Some(x)` for
    /// every `u32`.
    pub fn parse_node_iri(iri: &str) -> Option<u32> {
        iri.strip_prefix(NODE_PREFIX)
            .and_then(|tail| tail.parse::<u32>().ok())
    }

    /// Mint the canonical legacy edge IRI. The three components are the edge's
    /// endpoints and its stored `id` field, in that order.
    pub fn edge_iri(source: u32, target: u32, edge_id: &str) -> String {
        format!("{EDGE_PREFIX}{source}:{target}:{edge_id}")
    }

    /// The parts of a legacy edge IRI.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct EdgeRef {
        pub source: u32,
        pub target: u32,
        pub edge_id: String,
    }

    /// Parse a legacy edge IRI into its parts.
    ///
    /// The `edge_id` component may itself contain `:` (upstream callers choose
    /// it), so only the first two components are split off.
    pub fn parse_edge_iri(iri: &str) -> Option<EdgeRef> {
        let tail = iri.strip_prefix(EDGE_PREFIX)?;
        let (source, rest) = tail.split_once(':')?;
        let (target, edge_id) = rest.split_once(':')?;
        if edge_id.is_empty() {
            return None;
        }
        Some(EdgeRef {
            source: source.parse().ok()?,
            target: target.parse().ok()?,
            edge_id: edge_id.to_string(),
        })
    }

    /// Is `s` already a legacy edge IRI?
    pub fn is_edge_iri(s: &str) -> bool {
        parse_edge_iri(s).is_some()
    }

    /// How to locate an edge for deletion, given whatever the caller holds.
    ///
    /// The repository hands back `Edge::id` set to the **full IRI** when an edge
    /// is read from the store, but `Edge::new` leaves it as a bare
    /// `<source>-<target>` string. A delete path that assumed one form silently
    /// matched nothing when handed the other — which is exactly what
    /// `remove_edge` did, minting a one-segment `urn:ngm:edge:<id>` that no
    /// three-segment subject could ever equal. Resolving the two forms here,
    /// once, is what keeps the mint and the lookup in agreement.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum EdgeLookup {
        /// The caller already holds the exact subject IRI.
        Exact(String),
        /// The caller holds a bare stored edge id; match any legacy edge IRI
        /// whose final component equals it.
        TrailingId(String),
    }

    /// Classify an edge identifier into the lookup it supports.
    pub fn edge_lookup(edge_id: &str) -> EdgeLookup {
        if is_edge_iri(edge_id) {
            EdgeLookup::Exact(edge_id.to_string())
        } else {
            EdgeLookup::TrailingId(edge_id.to_string())
        }
    }
}

// ── Parsing (round-trip + BC20 ingest) ───────────────────────────────────────

/// A parsed converged identifier. `did:nostr` is represented as
/// [`ParsedUri::DidNostr`]; the URN kinds carry their structural fields so
/// the ingest path can record a namespace-crossing rather than store an opaque
/// blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedUri {
    /// `did:nostr:<pubkey>` — sovereign identity.
    DidNostr { pubkey: String },
    /// `concept:<domain>:<slug>`.
    Concept { domain: String, slug: String },
    /// `kg:<pubkey>:<sha256-12>`.
    Kg { pubkey: String, address: String },
    /// `bead:<pubkey>:<sha256-12>`.
    Bead { pubkey: String, address: String },
    /// `execution:<sha256-12>` (unscoped).
    Execution { address: String },
    /// `group:<team>#members`.
    Group { team: String },
    /// `room:<sha256-12>` (unscoped XR presence room).
    Room { address: String },
    /// `avatar:<hex-pubkey>` (identity-bound).
    Avatar { pubkey: String },
    /// A legacy `urn:ngm:<sub>` identifier that predates the convergence cutover.
    /// Carried opaquely (the segment after `urn:ngm:`) so the dual-read resolve
    /// path can recognise and round-trip it without re-minting it under the
    /// converged grammar. New mints never produce this variant. See ADR-105.
    LegacyNgm { sub: String },
}

impl ParsedUri {
    /// The kind, if this is one of the `urn:visionclaw` URN kinds
    /// (identity has no `Kind`).
    pub fn kind(&self) -> Option<Kind> {
        Some(match self {
            // Identity and legacy-ngm carry no converged `Kind`.
            ParsedUri::DidNostr { .. } | ParsedUri::LegacyNgm { .. } => return None,
            ParsedUri::Concept { .. } => Kind::Concept,
            ParsedUri::Kg { .. } => Kind::Kg,
            ParsedUri::Bead { .. } => Kind::Bead,
            ParsedUri::Execution { .. } => Kind::Execution,
            ParsedUri::Group { .. } => Kind::Group,
            ParsedUri::Room { .. } => Kind::Room,
            ParsedUri::Avatar { .. } => Kind::Avatar,
        })
    }

    /// The owner pubkey scope, when the kind carries one.
    pub fn owner_pubkey(&self) -> Option<&str> {
        match self {
            ParsedUri::DidNostr { pubkey }
            | ParsedUri::Kg { pubkey, .. }
            | ParsedUri::Bead { pubkey, .. }
            | ParsedUri::Avatar { pubkey } => Some(pubkey),
            _ => None,
        }
    }

    /// Reconstruct the canonical string form (round-trip of [`parse`]).
    pub fn to_uri(&self) -> String {
        match self {
            ParsedUri::DidNostr { pubkey } => format!("{DID_NOSTR_PREFIX}{pubkey}"),
            ParsedUri::Concept { domain, slug } => {
                format!("{NS}:{}:{domain}:{slug}", Kind::Concept)
            }
            ParsedUri::Kg { pubkey, address } => format!("{NS}:{}:{pubkey}:{address}", Kind::Kg),
            ParsedUri::Bead { pubkey, address } => {
                format!("{NS}:{}:{pubkey}:{address}", Kind::Bead)
            }
            ParsedUri::Execution { address } => format!("{NS}:{}:{address}", Kind::Execution),
            ParsedUri::Group { team } => format!("{NS}:{}:{team}#members", Kind::Group),
            ParsedUri::Room { address } => format!("{NS}:{}:{address}", Kind::Room),
            ParsedUri::Avatar { pubkey } => format!("{NS}:{}:{pubkey}", Kind::Avatar),
            ParsedUri::LegacyNgm { sub } => format!("{LEGACY_NGM_NS}:{sub}"),
        }
    }

    /// True iff this is a legacy `urn:ngm:*` identifier carried for backward
    /// resolution rather than a converged `urn:visionclaw` / `did:nostr` one.
    pub fn is_legacy(&self) -> bool {
        matches!(self, ParsedUri::LegacyNgm { .. })
    }
}

/// Parse a converged identifier (`did:nostr:*` or `urn:visionclaw:*`). Returns
/// [`UriError`] for any other namespace (including the legacy `urn:ngm:*`).
pub fn parse(input: &str) -> Result<ParsedUri, UriError> {
    if let Some(pubkey) = input.strip_prefix(DID_NOSTR_PREFIX) {
        if !is_pubkey_hex(pubkey) {
            return Err(UriError::InvalidPubkey(pubkey.to_string()));
        }
        return Ok(ParsedUri::DidNostr {
            pubkey: pubkey.to_string(),
        });
    }

    let rest = input
        .strip_prefix(&format!("{NS}:"))
        .ok_or_else(|| UriError::NotVisionclaw(input.to_string()))?;

    // kind is the first ':'-delimited token.
    let (kind_tok, tail) = rest
        .split_once(':')
        .ok_or_else(|| UriError::Malformed(input.to_string()))?;
    let kind =
        Kind::from_token(kind_tok).ok_or_else(|| UriError::UnknownKind(kind_tok.to_string()))?;

    match kind {
        Kind::Concept => {
            let (domain, slug) = tail
                .split_once(':')
                .ok_or_else(|| UriError::Malformed(input.to_string()))?;
            if domain.is_empty() {
                return Err(UriError::EmptySegment("concept domain"));
            }
            if slug.is_empty() {
                return Err(UriError::EmptySegment("concept slug"));
            }
            Ok(ParsedUri::Concept {
                domain: domain.to_string(),
                slug: slug.to_string(),
            })
        }
        Kind::Kg | Kind::Bead => {
            let (pubkey, address) = tail
                .split_once(':')
                .ok_or_else(|| UriError::Malformed(input.to_string()))?;
            if !is_pubkey_hex(pubkey) {
                return Err(UriError::InvalidPubkey(pubkey.to_string()));
            }
            // ADR-2023: reject a malformed suffix after the prefix, not just
            // a missing prefix.
            require_content_address(address)?;
            if kind == Kind::Kg {
                Ok(ParsedUri::Kg {
                    pubkey: pubkey.to_string(),
                    address: address.to_string(),
                })
            } else {
                Ok(ParsedUri::Bead {
                    pubkey: pubkey.to_string(),
                    address: address.to_string(),
                })
            }
        }
        Kind::Execution => {
            if !is_content_address(tail) {
                return Err(UriError::Malformed(input.to_string()));
            }
            Ok(ParsedUri::Execution {
                address: tail.to_string(),
            })
        }
        Kind::Group => {
            let team = tail
                .strip_suffix("#members")
                .ok_or_else(|| UriError::Malformed(input.to_string()))?;
            if team.is_empty() {
                return Err(UriError::EmptySegment("group team"));
            }
            Ok(ParsedUri::Group {
                team: team.to_string(),
            })
        }
        Kind::Room => {
            if !is_content_address(tail) {
                return Err(UriError::Malformed(input.to_string()));
            }
            Ok(ParsedUri::Room {
                address: tail.to_string(),
            })
        }
        Kind::Avatar => {
            if !is_pubkey_hex(tail) {
                return Err(UriError::InvalidPubkey(tail.to_string()));
            }
            Ok(ParsedUri::Avatar {
                pubkey: tail.to_string(),
            })
        }
    }
}

/// Dual-read parse for the resolve path (ADR-105). Accepts BOTH the converged
/// grammar (`did:nostr:*` / `urn:visionclaw:*`, via [`parse`]) AND legacy
/// `urn:ngm:*` identifiers persisted before the convergence cutover, which are
/// returned as [`ParsedUri::LegacyNgm`].
///
/// This is the function every *resolve/lookup* surface should call so that
/// already-stored `urn:ngm:node:*` / `urn:ngm:edge:*` / `urn:ngm:domain:*` IDs
/// keep resolving. Minting and strict validation continue to use [`parse`],
/// which deliberately rejects `urn:ngm:*` (new durable IDs are converged-only).
///
/// `urn:ngm:graph:*` named graphs are recognised here like any other legacy
/// sub-namespace; per ADR-100 they remain the persistence named-graph IRIs and
/// are not rewritten.
pub fn parse_dual(input: &str) -> Result<ParsedUri, UriError> {
    match parse(input) {
        Ok(p) => Ok(p),
        Err(UriError::NotVisionclaw(_)) => {
            if let Some(sub) = input.strip_prefix(&format!("{LEGACY_NGM_NS}:")) {
                if sub.is_empty() {
                    return Err(UriError::Malformed(input.to_string()));
                }
                return Ok(ParsedUri::LegacyNgm {
                    sub: sub.to_string(),
                });
            }
            Err(UriError::NotVisionclaw(input.to_string()))
        }
        Err(e) => Err(e),
    }
}

// ── BC20 cross-namespace ingest (urn:agentbox:* → urn:visionclaw:*) ──────────

/// The shared federation kind artefact, embedded at build time (ADR-2061).
///
/// `agentbox/schema/federation-kinds.json` is the SINGLE versioned authority for
/// which `urn:agentbox` kinds cross this boundary. The agentbox tree is the
/// `./agentbox` submodule of this checkout, so the path resolves from `src/uri/`
/// and the bytes compiled in here are byte-identical to the ones
/// `management-api/lib/bc20-provenance-bridge.js` reads at load. Neither
/// translator transcribes the kind list; [`cross_from_agentbox`] derives its
/// closed map from this artefact, and the paired fixture
/// `federation_kind_artefact_matches_translator` (plus its JS twin
/// `tests/contract/federation-kind-parity.contract.spec.js`) turns any
/// disagreement into a test failure.
const FEDERATION_KINDS_JSON: &str = include_str!("../../agentbox/schema/federation-kinds.json");

/// A fixture row: an input identifier and the output both translators must
/// produce for it (`None` = the crossing is refused).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FederationFixture {
    /// The identifier fed to the translator.
    pub input: String,
    /// The expected output, or `None` when the crossing must be refused.
    #[serde(default)]
    pub expect: Option<String>,
}

/// The arguments that would unlock a `deliberate` refusal, recorded so that
/// "deliberately closed" stays distinguishable from "not implemented".
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FederationElevation {
    /// Side-channel arguments the hot path does not carry (e.g. `domain`, `slug`).
    pub required_args: Vec<String>,
}

/// One row of the shared kind artefact.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FederationKind {
    /// The `urn:agentbox` kind this row governs.
    pub kind: String,
    /// Whether the kind crosses on the ingest hot path (no side information).
    pub crosses: bool,
    /// Whether crossing requires a 64-hex owner pubkey scope.
    #[serde(default)]
    pub requires_owner_scope: bool,
    /// `identity` | `content-address-of-urn` | `structural-passthrough`.
    #[serde(default)]
    pub crossing_form: Option<String>,
    /// The `urn:agentbox` grammar this row accepts.
    pub source_grammar: String,
    /// The VisionClaw target kind, when one exists at all.
    #[serde(default)]
    pub target_kind: Option<String>,
    /// The grammar of the translated identifier.
    #[serde(default)]
    pub target_grammar: Option<String>,
    /// `deliberate` | `not-federated`, when `crosses` is false.
    #[serde(default)]
    pub refusal_class: Option<String>,
    /// Why the crossing is refused, when `crosses` is false.
    #[serde(default)]
    pub refusal_reason: Option<String>,
    /// Present on `deliberate` refusals only.
    #[serde(default)]
    pub elevation: Option<FederationElevation>,
    /// The canonical fixture both translators must agree on.
    pub fixture: FederationFixture,
    /// An owner-scope-less input that must be refused, where scope is required.
    #[serde(default)]
    pub unscoped_fixture: Option<FederationFixture>,
}

/// The already-converged `did:nostr` identity pass-through.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct IdentityPassthrough {
    /// Inbound grammar.
    pub source_grammar: String,
    /// Outbound grammar (identical — identity round-trips structurally).
    pub target_grammar: String,
    /// A well-formed DID that must pass through unchanged.
    pub fixture: FederationFixture,
    /// A malformed DID that must be refused.
    pub malformed_fixture: FederationFixture,
}

/// The parsed federation kind artefact.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FederationKindArtefact {
    /// Artefact discriminator (`federation-kinds`).
    pub artefact: String,
    /// Semantic version of the kind list.
    pub version: String,
    /// The governing decision record (`ADR-2061`).
    pub adr: String,
    /// The `did:nostr` pass-through contract.
    pub identity_passthrough: IdentityPassthrough,
    /// One row per `urn:agentbox` kind.
    pub kinds: Vec<FederationKind>,
}

/// The shared federation kind artefact, parsed once.
///
/// # Panics
/// Panics if the embedded artefact is malformed. It is compiled in, so a parse
/// failure is a build-time authoring error, not a runtime input.
pub fn federation_kinds() -> &'static FederationKindArtefact {
    static ARTEFACT: std::sync::OnceLock<FederationKindArtefact> = std::sync::OnceLock::new();
    ARTEFACT.get_or_init(|| {
        serde_json::from_str(FEDERATION_KINDS_JSON).expect(
            "agentbox/schema/federation-kinds.json is malformed - the federation kind \
             artefact is embedded at build time and must parse",
        )
    })
}

/// Look up the artefact row governing one `urn:agentbox` kind.
pub fn federation_kind(kind: &str) -> Option<&'static FederationKind> {
    federation_kinds().kinds.iter().find(|k| k.kind == kind)
}

/// A namespace crossing recorded at the federation boundary. Carries both ends
/// so the ingest path stores the translation rather than an opaque foreign blob,
/// and the audit surface can recover the agentbox source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrnCrossing {
    /// The original `urn:agentbox:*` (or `did:nostr:*`) identifier as received.
    pub agentbox_urn: String,
    /// The translated converged VisionClaw identifier.
    pub visionclaw_id: String,
    /// `did:nostr:<pubkey>` owner when the source carried a pubkey scope.
    pub owner_did: Option<String>,
}

/// Translate an inbound `urn:agentbox:<kind>:<scope>:<local>` (or `did:nostr:*`)
/// into its converged VisionClaw identifier — the VisionClaw-side counterpart of
/// agentbox `bc20-provenance-bridge.js::toVisionclaw`.
///
/// The kind map is **not written here**. It is derived from the shared artefact
/// [`FEDERATION_KINDS_JSON`] (`agentbox/schema/federation-kinds.json`), the same
/// bytes the JS bridge reads, so the two translators cannot drift. As of
/// artefact 1.0.0 the crossing kinds are:
///
///   * `agent`    → `did:nostr:<pubkey>` (identity, structural round-trip)
///   * `activity` → `urn:visionclaw:execution:<sha256-12>` (unscoped)
///   * `thing`    → `urn:visionclaw:kg:<pubkey>:<sha256-12>`
///   * `bead`     → `urn:visionclaw:bead:<pubkey>:<sha256-12>` (structural
///     pass-through — ADR-2072 / agentbox ADR-2061: agentbox
///     `bead` locals are already
///     `sha256-12-<12hex>` content addresses, so the crossing
///     preserves the existing address instead of re-hashing)
///
/// and `memory` is a *recorded* refusal rather than an absent arm: it would
/// reach `urn:visionclaw:concept:<domain>:<slug>`, but that needs a
/// `{domain, slug}` elevation the ingest hot path does not carry, so the
/// artefact marks it `crosses: false` with `refusal_class: "deliberate"` and
/// names the arguments that would unlock it. Every other kind is
/// `not-federated`. Consult [`federation_kind`] for a row's reason.
///
/// A `did:nostr:*` input is already converged and round-trips structurally.
/// Returns `None` (the caller records the raw string + an unmapped marker) for
/// any refused or unknown kind, mirroring the agentbox B04 closed-map discipline.
pub fn cross_from_agentbox(agentbox_urn: &str) -> Option<UrnCrossing> {
    // Already-converged identity passes through unchanged.
    if let Some(pk) = agentbox_urn.strip_prefix(DID_NOSTR_PREFIX) {
        if is_pubkey_hex(pk) {
            return Some(UrnCrossing {
                agentbox_urn: agentbox_urn.to_string(),
                visionclaw_id: agentbox_urn.to_string(),
                owner_did: Some(agentbox_urn.to_string()),
            });
        }
        return None;
    }

    let rest = agentbox_urn.strip_prefix("urn:agentbox:")?;
    let (kind, tail) = rest.split_once(':')?;
    // scope is the next token when it is a 64-hex pubkey; otherwise unscoped.
    let scope = tail.split(':').next().filter(|s| is_pubkey_hex(s));
    let owner_did = scope.and_then(|pk| did_nostr(pk).ok());

    // The closed map is DERIVED from the shared artefact (ADR-2061), never
    // transcribed here. An unknown kind, or one the artefact records as a
    // refusal, returns None — the caller stores the raw string plus an unmapped
    // marker rather than a synthetic identifier (B04).
    let spec = federation_kind(kind)?;
    if !spec.crosses {
        return None;
    }

    // Target kind -> the typed constructor that mints its grammar. This is the
    // only mapping the Rust side owns; which kinds reach it is the artefact's
    // decision. A crossing row naming a target with no arm falls to the final
    // arm and is refused, and `federation_targets_all_have_an_arm` fails, so a
    // one-sided artefact addition cannot ship as a silent drop.
    let visionclaw_id = match spec.target_kind.as_deref() {
        Some("did:nostr") => did_nostr(scope?).ok()?,
        Some("execution") => execution(agentbox_urn),
        Some("kg") => kg(scope?, agentbox_urn).ok()?,
        Some("bead") => {
            // Structural pass-through (ADR-2072 landed the arm, agentbox
            // ADR-2061 the shared artefact): the agentbox `bead` local is
            // already a `sha256-12-<12hex>` content address (uris.js
            // KINDS.bead.contentAddressed = true), the same shape VisionClaw
            // uses, so the crossing preserves it rather than re-hashing the
            // whole agentbox URN the way `thing`/`activity` do.
            let pk = scope?;
            let local = tail.strip_prefix(pk)?.strip_prefix(':')?;
            bead_with_address(pk, local).ok()?
        }
        _ => return None,
    };

    Some(UrnCrossing {
        agentbox_urn: agentbox_urn.to_string(),
        visionclaw_id,
        owner_did,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn pubkey_validation() {
        assert!(is_pubkey_hex(PK_A));
        assert!(!is_pubkey_hex("AAAA")); // too short
        assert!(!is_pubkey_hex(&PK_A.to_uppercase())); // must be lowercase
        assert!(!is_pubkey_hex(&format!("{}g", &PK_A[..63]))); // non-hex char
    }

    #[test]
    fn content_address_is_12_lowercase_hex() {
        let a = content_address(b"hello world");
        assert!(a.starts_with(CONTENT_ADDR_PREFIX));
        let hex = a.strip_prefix(CONTENT_ADDR_PREFIX).unwrap();
        assert_eq!(hex.len(), 12);
        assert!(hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        // deterministic
        assert_eq!(a, content_address(b"hello world"));
        // matches a hand-computed sha256-12 of "hello world".
        assert_eq!(a, "sha256-12-b94d27b9934d");
    }

    #[test]
    fn slugify_matches_agentbox_shape() {
        assert_eq!(slugify("  Hello World!! "), "hello-world");
        assert_eq!(slugify("Foo/Bar.baz_qux"), "foo-bar.baz_qux");
        assert_eq!(slugify("---trim---"), "trim");
    }

    // ── per-kind shape + round-trip ──────────────────────────────────────────

    #[test]
    fn did_nostr_shape_and_roundtrip() {
        let id = did_nostr(PK_A).unwrap();
        assert_eq!(id, format!("did:nostr:{PK_A}"));
        let p = parse(&id).unwrap();
        assert_eq!(
            p,
            ParsedUri::DidNostr {
                pubkey: PK_A.into()
            }
        );
        assert_eq!(p.to_uri(), id);
        assert_eq!(p.kind(), None);
        assert_eq!(p.owner_pubkey(), Some(PK_A));
        assert!(did_nostr("nope").is_err());
    }

    #[test]
    fn concept_shape_and_roundtrip() {
        let id = concept("Knowledge Graph", "Spreading Activation").unwrap();
        assert_eq!(
            id,
            "urn:visionclaw:concept:knowledge-graph:spreading-activation"
        );
        let p = parse(&id).unwrap();
        assert_eq!(p.kind(), Some(Kind::Concept));
        assert_eq!(p.to_uri(), id);
        assert!(concept("", "x").is_err());
        assert!(concept("d", "  ").is_err());
    }

    #[test]
    fn kg_shape_and_roundtrip() {
        let id = kg(PK_A, b"node-payload").unwrap();
        assert!(id.starts_with(&format!("urn:visionclaw:kg:{PK_A}:sha256-12-")));
        let p = parse(&id).unwrap();
        assert_eq!(p.kind(), Some(Kind::Kg));
        assert_eq!(p.owner_pubkey(), Some(PK_A));
        assert_eq!(p.to_uri(), id);
        // content-addressing is deterministic on the same owner + content.
        assert_eq!(id, kg(PK_A, b"node-payload").unwrap());
        assert!(kg("short", b"x").is_err());
    }

    #[test]
    fn kg_with_precomputed_address_roundtrip() {
        let addr = content_address(b"crossing-value");
        let id = kg_with_address(PK_B, &addr).unwrap();
        let p = parse(&id).unwrap();
        assert_eq!(
            p,
            ParsedUri::Kg {
                pubkey: PK_B.into(),
                address: addr
            }
        );
        assert_eq!(p.to_uri(), id);
        assert!(kg_with_address(PK_B, "not-an-addr").is_err());
    }

    #[test]
    fn bead_shape_and_roundtrip() {
        let id = bead(PK_B, b"bead-content").unwrap();
        assert!(id.starts_with(&format!("urn:visionclaw:bead:{PK_B}:sha256-12-")));
        let p = parse(&id).unwrap();
        assert_eq!(p.kind(), Some(Kind::Bead));
        assert_eq!(p.owner_pubkey(), Some(PK_B));
        assert_eq!(p.to_uri(), id);
    }

    #[test]
    fn execution_shape_and_roundtrip() {
        let id = execution(b"urn:agentbox:activity:abc:run-1");
        assert!(id.starts_with("urn:visionclaw:execution:sha256-12-"));
        // unscoped — no pubkey segment.
        let p = parse(&id).unwrap();
        assert_eq!(p.kind(), Some(Kind::Execution));
        assert_eq!(p.owner_pubkey(), None);
        assert_eq!(p.to_uri(), id);
        // a stray scope segment must be rejected.
        assert!(parse("urn:visionclaw:execution:sha256-12-deadbeef0011:extra").is_err());
    }

    #[test]
    fn group_shape_and_roundtrip() {
        let id = group_members("Dream Lab").unwrap();
        assert_eq!(id, "urn:visionclaw:group:dream-lab#members");
        let p = parse(&id).unwrap();
        assert_eq!(
            p,
            ParsedUri::Group {
                team: "dream-lab".into()
            }
        );
        assert_eq!(p.to_uri(), id);
        assert!(group_members("   ").is_err());
    }

    #[test]
    fn room_shape_and_roundtrip() {
        let id = room(b"room-descriptor");
        assert!(id.starts_with("urn:visionclaw:room:sha256-12-"));
        let p = parse(&id).unwrap();
        assert_eq!(p.kind(), Some(Kind::Room));
        assert_eq!(p.owner_pubkey(), None);
        assert_eq!(p.to_uri(), id);
        // deterministic on content
        assert_eq!(id, room(b"room-descriptor"));
        // a stray scope segment must be rejected.
        assert!(parse("urn:visionclaw:room:sha256-12-deadbeef0011:extra").is_err());
    }

    #[test]
    fn avatar_shape_and_roundtrip() {
        let id = avatar(PK_A).unwrap();
        assert_eq!(id, format!("urn:visionclaw:avatar:{PK_A}"));
        let p = parse(&id).unwrap();
        assert_eq!(
            p,
            ParsedUri::Avatar {
                pubkey: PK_A.into()
            }
        );
        assert_eq!(p.kind(), Some(Kind::Avatar));
        assert_eq!(p.owner_pubkey(), Some(PK_A));
        assert_eq!(p.to_uri(), id);
        assert!(avatar("nope").is_err());
        assert!(parse(&format!("urn:visionclaw:avatar:{}", PK_A.to_uppercase())).is_err());
    }

    #[test]
    fn rejects_other_namespaces_including_legacy_ngm() {
        assert!(matches!(
            parse("urn:ngm:node:42"),
            Err(UriError::NotVisionclaw(_))
        ));
        assert!(matches!(
            parse("urn:agentbox:thing:x:y"),
            Err(UriError::NotVisionclaw(_))
        ));
        assert!(matches!(
            parse("urn:visionclaw:bogus:whatever"),
            Err(UriError::UnknownKind(_))
        ));
    }

    #[test]
    fn parse_dual_accepts_converged_and_legacy_ngm() {
        // Converged forms round-trip identically through the dual reader.
        let vc = kg(PK_A, b"x").unwrap();
        assert_eq!(parse_dual(&vc).unwrap(), parse(&vc).unwrap());
        let did = did_nostr(PK_A).unwrap();
        assert_eq!(parse_dual(&did).unwrap(), parse(&did).unwrap());

        // Legacy persisted IDs resolve as LegacyNgm and round-trip their string.
        for legacy in [
            "urn:ngm:node:42",
            "urn:ngm:edge:1:2:1-2",
            "urn:ngm:domain:knowledge",
            "urn:ngm:graph:knowledge",
        ] {
            let p = parse_dual(legacy).unwrap();
            assert!(p.is_legacy());
            assert_eq!(p.kind(), None);
            assert_eq!(p.owner_pubkey(), None);
            assert_eq!(p.to_uri(), legacy);
        }

        // Strict parse still rejects legacy (mint path is converged-only).
        assert!(matches!(
            parse("urn:ngm:node:42"),
            Err(UriError::NotVisionclaw(_))
        ));
        // Neither reader accepts an empty legacy sub or a foreign namespace.
        assert!(parse_dual("urn:ngm:").is_err());
        assert!(matches!(
            parse_dual("urn:agentbox:thing:x:y"),
            Err(UriError::NotVisionclaw(_))
        ));
        // A malformed converged id still surfaces its specific error, not a downgrade.
        assert!(matches!(
            parse_dual("urn:visionclaw:bogus:whatever"),
            Err(UriError::UnknownKind(_))
        ));
    }

    #[test]
    fn bc20_crosses_agentbox_kinds_per_closed_map() {
        // agent → did:nostr (identity, structural round-trip)
        let c = cross_from_agentbox(&format!("urn:agentbox:agent:{PK_A}:planner")).unwrap();
        assert_eq!(c.visionclaw_id, format!("did:nostr:{PK_A}"));
        assert_eq!(c.owner_did.as_deref(), Some(&*format!("did:nostr:{PK_A}")));

        // thing → kg (owner-scoped, content-addressed)
        let c = cross_from_agentbox(&format!("urn:agentbox:thing:{PK_A}:proposal-7")).unwrap();
        assert!(c
            .visionclaw_id
            .starts_with(&format!("urn:visionclaw:kg:{PK_A}:sha256-12-")));
        assert!(parse(&c.visionclaw_id).is_ok());

        // activity → execution (unscoped)
        let c = cross_from_agentbox(&format!("urn:agentbox:activity:{PK_A}:run-3")).unwrap();
        assert!(c
            .visionclaw_id
            .starts_with("urn:visionclaw:execution:sha256-12-"));
        assert_eq!(c.owner_did.as_deref(), Some(&*format!("did:nostr:{PK_A}")));

        // already-converged did:nostr passes through unchanged
        let did = format!("did:nostr:{PK_A}");
        let c = cross_from_agentbox(&did).unwrap();
        assert_eq!(c.visionclaw_id, did);

        // memory→concept (no elevation target on the hot path) is unmapped → None
        assert!(cross_from_agentbox(&format!("urn:agentbox:memory:{PK_A}:lesson-x")).is_none());
        // unknown kind → None (closed map)
        assert!(cross_from_agentbox(&format!("urn:agentbox:credential:{PK_A}:vc-1")).is_none());
        // not a foreign agentbox urn → None
        assert!(cross_from_agentbox("urn:ngm:node:1").is_none());
    }

    #[test]
    fn bc20_crosses_bead_structurally_preserving_content_address() {
        // bead → bead (owner-scoped, structural pass-through of the existing
        // content address — ADR-2072 / agentbox ADR-2061). Unlike thing/activity,
        // the local is NOT re-hashed: the agentbox local IS the VisionClaw
        // address, unchanged.
        let addr = content_address(b"some bead payload");
        let agentbox_urn = format!("urn:agentbox:bead:{PK_A}:{addr}");
        let c = cross_from_agentbox(&agentbox_urn).unwrap();
        assert_eq!(
            c.visionclaw_id,
            format!("urn:visionclaw:bead:{PK_A}:{addr}")
        );
        assert_eq!(c.owner_did.as_deref(), Some(&*format!("did:nostr:{PK_A}")));
        assert_eq!(c.agentbox_urn, agentbox_urn);
        // Well-formed per the converged grammar and round-trips through parse().
        let parsed = parse(&c.visionclaw_id).unwrap();
        assert_eq!(parsed.kind(), Some(Kind::Bead));
        assert_eq!(
            parsed,
            ParsedUri::Bead {
                pubkey: PK_A.to_string(),
                address: addr,
            }
        );
    }

    #[test]
    fn bc20_bead_crossing_rejects_invalid_scope() {
        // An invalid (non-64-hex) scope must not cross — the closed map treats
        // it the same as a missing scope: None, never a mis-scoped mint.
        let addr = content_address(b"payload");
        assert!(cross_from_agentbox(&format!("urn:agentbox:bead:not-a-pubkey:{addr}")).is_none());
        // Too-short hex also rejected.
        assert!(cross_from_agentbox(&format!("urn:agentbox:bead:deadbeef:{addr}")).is_none());
    }

    #[test]
    fn matches_agentbox_kg_target_urn_fixture() {
        // The schema.rs cross-repo fixture carries this exact target_urn shape;
        // it must parse cleanly as a kg node on the VisionClaw side.
        let fixture =
            "urn:visionclaw:kg:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:sha256-12-deadbeef0011";
        let p = parse(fixture).unwrap();
        assert_eq!(p.kind(), Some(Kind::Kg));
        assert_eq!(p.owner_pubkey(), Some(PK_B));
        assert_eq!(p.to_uri(), fixture);
    }

    // ---- ADR-2023: content-address grammar --------------------------------

    /// Every address the emitter produces satisfies the grammar the parser
    /// enforces. Hash emission and input validation are separate guarantees,
    /// and this is the assertion that ties them together.
    #[test]
    fn emitted_addresses_satisfy_the_grammar() {
        for input in ["", "a", "hello world", "\u{1F600}", &"x".repeat(10_000)] {
            let addr = content_address(input);
            assert!(
                is_content_address(&addr),
                "emitter produced an address the parser rejects: {addr}"
            );
            assert_eq!(addr.len(), CONTENT_ADDR_PREFIX.len() + CONTENT_ADDR_HEX_LEN);
        }
    }

    /// The reproduced defect: a malformed suffix after a correct prefix.
    #[test]
    fn malformed_precomputed_addresses_are_rejected() {
        // (address, why it is malformed)
        let bad = [
            ("sha256-12-", "empty body"),
            ("sha256-12-abc", "too short"),
            ("sha256-12-abcdef0123456", "too long"),
            ("sha256-12-ABCDEF012345", "upper-case hex"),
            ("sha256-12-abcdefghijkl", "non-hex letters"),
            ("sha256-12-abcdef01234!", "punctuation"),
            ("sha256-12-abcdef 12345", "embedded space"),
            ("sha256-12-abcdef:12345", "embedded colon"),
            ("sha256-12-abcdef01234\n", "trailing newline"),
            ("sha256-12--bcdef012345", "leading dash in body"),
            ("not-an-addr", "no prefix at all"),
            ("sha256-12", "truncated prefix"),
            ("SHA256-12-abcdef012345", "upper-case prefix"),
        ];
        for (addr, why) in bad {
            assert!(
                !is_content_address(addr),
                "{addr:?} must be rejected ({why})"
            );
            assert!(
                matches!(kg_with_address(PK_A, addr), Err(UriError::Malformed(_))),
                "kg_with_address must reject {addr:?} ({why})"
            );
        }
    }

    /// A malformed address embedded in a full URN is rejected by the parser
    /// too, not merely by the constructor.
    #[test]
    fn parser_rejects_malformed_addresses_in_kg_and_bead_urns() {
        for kind in ["kg", "bead"] {
            for body in ["", "abc", "ABCDEF012345", "abcdefghijkl", "abcdef0123456"] {
                let urn = format!("{NS}:{kind}:{PK_A}:{CONTENT_ADDR_PREFIX}{body}");
                assert!(
                    matches!(parse(&urn), Err(UriError::Malformed(_))),
                    "parser must reject {urn}"
                );
            }
        }
    }

    /// Execution and room addresses obey the same grammar.
    #[test]
    fn parser_rejects_malformed_execution_and_room_addresses() {
        for kind in ["execution", "room"] {
            for body in ["", "abc", "ABCDEF012345", "abcdef0123456"] {
                let urn = format!("{NS}:{kind}:{CONTENT_ADDR_PREFIX}{body}");
                assert!(
                    matches!(parse(&urn), Err(UriError::Malformed(_))),
                    "parser must reject {urn}"
                );
            }
        }
    }

    /// A well-formed precomputed address still round-trips — the tightened
    /// grammar rejects malformed input without narrowing the valid set.
    #[test]
    fn well_formed_precomputed_address_round_trips() {
        let addr = content_address("crossing the BC20 boundary");
        let urn = kg_with_address(PK_A, &addr).expect("valid address is accepted");
        match parse(&urn).expect("parses") {
            ParsedUri::Kg { pubkey, address } => {
                assert_eq!(pubkey, PK_A);
                assert_eq!(address, addr);
            }
            other => panic!("expected Kg, got {other:?}"),
        }
    }

    /// Persisted round-trip recovery: an address minted, serialised into a URN,
    /// parsed back and re-minted yields the identical string.
    #[test]
    fn precomputed_address_survives_a_persistence_round_trip() {
        let original = kg(PK_B, "durable content").expect("mint");
        let parsed = parse(&original).expect("parse");
        let ParsedUri::Kg { pubkey, address } = &parsed else {
            panic!("expected Kg, got {parsed:?}");
        };
        let reminted = kg_with_address(pubkey, address).expect("re-mint from the parsed parts");
        assert_eq!(reminted, original);
        assert_eq!(parsed.to_uri(), original);
    }

    /// Every hex digit is accepted; the grammar is not accidentally narrower
    /// than the emitter.
    #[test]
    fn all_lowercase_hex_digits_are_accepted() {
        assert!(is_content_address("sha256-12-0123456789ab"));
        assert!(is_content_address("sha256-12-cdef01234567"));
        assert!(is_content_address("sha256-12-000000000000"));
        assert!(is_content_address("sha256-12-ffffffffffff"));
    }

    // ---- ADR-2021: legacy mint sites routed through typed constructors -----

    /// The legacy node scheme mints and parses as a pair, for the whole `u32`
    /// range including the class-bit boundaries.
    #[test]
    fn legacy_node_iri_round_trips() {
        for id in [0u32, 1, 42, 0x0400_0000, 0x4000_0000, 0x1C00_0000, u32::MAX] {
            let iri = ngm::node_iri(id);
            assert!(iri.starts_with(ngm::NODE_PREFIX));
            assert_eq!(
                ngm::parse_node_iri(&iri),
                Some(id),
                "{iri} must parse back to {id}"
            );
        }
    }

    #[test]
    fn legacy_node_iri_rejects_foreign_input() {
        assert_eq!(ngm::parse_node_iri("urn:ngm:edge:1:2:x"), None);
        assert_eq!(ngm::parse_node_iri("urn:ngm:node:"), None);
        assert_eq!(ngm::parse_node_iri("urn:ngm:node:not-a-number"), None);
        assert_eq!(ngm::parse_node_iri("urn:ngm:node:-1"), None);
        // u32::MAX + 1 does not fit.
        assert_eq!(ngm::parse_node_iri("urn:ngm:node:4294967296"), None);
        assert_eq!(ngm::parse_node_iri("urn:visionclaw:kg:x:y"), None);
    }

    /// The legacy edge scheme likewise round-trips, including an `edge_id` that
    /// itself contains a colon.
    #[test]
    fn legacy_edge_iri_round_trips() {
        for (source, target, id) in [
            (1u32, 2u32, "1-2"),
            (0, 0, "self"),
            (u32::MAX, 0, "max-to-zero"),
            (7, 9, "has:colons:inside"),
        ] {
            let iri = ngm::edge_iri(source, target, id);
            let parsed = ngm::parse_edge_iri(&iri).expect("parses");
            assert_eq!(parsed.source, source);
            assert_eq!(parsed.target, target);
            assert_eq!(parsed.edge_id, id);
        }
    }

    #[test]
    fn legacy_edge_iri_rejects_foreign_input() {
        assert_eq!(ngm::parse_edge_iri("urn:ngm:node:5"), None);
        // One segment — the shape the old remove_edge minted.
        assert_eq!(ngm::parse_edge_iri("urn:ngm:edge:1-2"), None);
        // Two segments.
        assert_eq!(ngm::parse_edge_iri("urn:ngm:edge:1:2"), None);
        // Non-numeric endpoints.
        assert_eq!(ngm::parse_edge_iri("urn:ngm:edge:a:b:c"), None);
        // Empty trailing id.
        assert_eq!(ngm::parse_edge_iri("urn:ngm:edge:1:2:"), None);
    }

    /// The reproduced mint/lookup mismatch. `remove_edge` was handed either a
    /// full IRI (what a read returns in `Edge::id`) or a bare stored id (what
    /// `Edge::new` leaves there), and minted a one-segment IRI for both — a
    /// subject that no three-segment edge could ever equal. `edge_lookup`
    /// distinguishes the two forms, which is what makes the delete match.
    #[test]
    fn edge_lookup_distinguishes_a_full_iri_from_a_bare_id() {
        let full = ngm::edge_iri(3, 4, "3-4");
        assert_eq!(
            ngm::edge_lookup(&full),
            ngm::EdgeLookup::Exact(full.clone()),
            "a full IRI is matched exactly"
        );
        assert_eq!(
            ngm::edge_lookup("3-4"),
            ngm::EdgeLookup::TrailingId("3-4".to_string()),
            "a bare stored id is matched by its trailing component"
        );
        // The old one-segment form is not a valid edge IRI, so it falls through
        // to a trailing-id match rather than being treated as an exact subject.
        assert_eq!(
            ngm::edge_lookup("urn:ngm:edge:3-4"),
            ngm::EdgeLookup::TrailingId("urn:ngm:edge:3-4".to_string())
        );
    }

    /// A full IRI ends with `:<edge_id>`, so the trailing-id match the
    /// repository issues finds exactly the edge the mint wrote.
    #[test]
    fn a_minted_edge_iri_ends_with_its_bare_id() {
        let iri = ngm::edge_iri(11, 12, "11-12");
        assert!(
            iri.ends_with(":11-12"),
            "the trailing-id delete filter depends on this: {iri}"
        );
        assert!(iri.starts_with(ngm::EDGE_PREFIX));
    }

    // ---- ADR-2095: the legacy class scheme, re-exported from the domain ----

    /// The re-export is the same typed pair the adapters crate mints through,
    /// so `uri::ngm` stays the single lookup point for the legacy scheme.
    #[test]
    fn legacy_class_iri_round_trips() {
        for slug in ["camera", "finality-mechanism", "built-environment", "0"] {
            let iri = ngm::class_iri(slug);
            assert!(iri.starts_with(ngm::CLASS_PREFIX));
            assert_eq!(ngm::parse_class_iri(&iri), Some(slug));
            assert!(ngm::is_class_iri(&iri));
        }
    }

    /// The constructor reproduces the exact literal the raw `format!` mints in
    /// `elevation_actor.rs` and `oxigraph_ontology_repository.rs` emitted
    /// before ADR-2095 — persisted class IRIs must not shift.
    #[test]
    fn legacy_class_iri_preserves_the_previous_literal() {
        assert_eq!(
            ngm::class_iri("finality-mechanism"),
            "urn:ngm:class:finality-mechanism"
        );
        assert_eq!(ngm::class_iri("camera"), "urn:ngm:class:camera");
    }

    /// The class parser rejects the other legacy schemes and the empty slug,
    /// exactly as `parse_node_iri`/`parse_edge_iri` reject theirs.
    #[test]
    fn legacy_class_iri_parser_rejects_other_schemes() {
        assert_eq!(ngm::parse_class_iri("urn:ngm:class:"), None);
        assert_eq!(ngm::parse_class_iri(&ngm::node_iri(5)), None);
        assert_eq!(ngm::parse_class_iri(&ngm::edge_iri(1, 2, "1-2")), None);
        assert_eq!(ngm::parse_class_iri("urn:ngm:property:has-part"), None);
        assert_eq!(
            ngm::parse_class_iri("urn:visionclaw:concept:xr:camera"),
            None
        );
    }

    /// Legacy identifiers stay legacy: `parse` never claims them as converged
    /// VisionClaw URNs, and `parse_dual` resolves them as legacy.
    #[test]
    fn legacy_identifiers_are_not_converged_urns() {
        for iri in [
            ngm::node_iri(5),
            ngm::edge_iri(5, 6, "5-6"),
            ngm::class_iri("camera"),
        ] {
            assert!(
                parse(&iri).is_err(),
                "{iri} must not parse as a converged URN"
            );
            assert!(
                matches!(parse_dual(&iri), Ok(ParsedUri::LegacyNgm { .. })),
                "{iri} must resolve as a legacy identifier"
            );
        }
    }

    /// The converged execution URN the mutation service now mints parses
    /// cleanly — the old `execution:<kind>-<id>` form did not.
    #[test]
    fn mutation_service_execution_urn_shape_parses() {
        let urn = execution("class-create:proposal-123");
        match parse(&urn).expect("the minted execution URN must parse") {
            ParsedUri::Execution { address } => assert!(is_content_address(&address)),
            other => panic!("expected Execution, got {other:?}"),
        }

        // The shape the service used to mint is now correctly rejected.
        let legacy_shape = format!("{NS}:execution:class-create-proposal-123");
        assert!(
            matches!(parse(&legacy_shape), Err(UriError::Malformed(_))),
            "the pre-ADR-2021 execution shape must not parse"
        );
    }

    // ── ADR-2061: federation kind-map parity (the Rust half of the fixture) ──
    //
    // Every case below is generated from `agentbox/schema/federation-kinds.json`
    // — the same bytes `bc20-provenance-bridge.js` reads. Nothing here restates
    // the kind list, so a kind added to one translator but not the other, or to
    // the artefact with no arm on this side, fails rather than diverging
    // silently. The JS twin is
    // `agentbox/tests/contract/federation-kind-parity.contract.spec.js`.

    /// The VisionClaw targets `cross_from_agentbox` has a constructor arm for.
    /// Kept in step with the `match spec.target_kind` above.
    const RUST_HANDLED_TARGETS: &[&str] = &["did:nostr", "execution", "kg", "bead"];

    /// Expand a declared grammar (`urn:visionclaw:kg:<pubkey>:<sha256-12>`) into
    /// a regex, so the artefact's prose grammar is executable rather than a
    /// comment that can drift from the code.
    fn grammar_to_regex(grammar: &str) -> regex::Regex {
        let mut out = String::from("^");
        let mut rest = grammar;
        while let Some(start) = rest.find('<') {
            let end = rest[start..]
                .find('>')
                .map(|i| start + i + 1)
                .expect("grammar placeholder must be closed");
            out.push_str(&regex::escape(&rest[..start]));
            out.push_str(match &rest[start..end] {
                "<pubkey>" => "[0-9a-f]{64}",
                "<sha256-12>" => "sha256-12-[0-9a-f]{12}",
                "<domain>" | "<slug>" => "[a-z0-9._-]+",
                "<local>" | "<name>" => "[^:]+",
                other => panic!("grammar placeholder {other} has no regex mapping"),
            });
            rest = &rest[end..];
        }
        out.push_str(&regex::escape(rest));
        out.push('$');
        regex::Regex::new(&out).expect("generated grammar regex must compile")
    }

    #[test]
    fn federation_artefact_is_versioned_and_embedded() {
        let art = federation_kinds();
        assert_eq!(art.artefact, "federation-kinds");
        assert_eq!(art.adr, "ADR-2061");
        assert!(
            art.version.split('.').count() == 3,
            "artefact version must be semver, got {}",
            art.version
        );
        assert!(
            !art.kinds.is_empty(),
            "the embedded artefact must enumerate the federated kinds"
        );
    }

    /// Acceptance 2 + 4: this translator agrees with EVERY artefact row, for
    /// crossing and for refusal alike, and the list is read, never restated.
    #[test]
    fn federation_kind_artefact_matches_translator() {
        for row in &federation_kinds().kinds {
            let got = cross_from_agentbox(&row.fixture.input).map(|c| c.visionclaw_id);
            match (&row.fixture.expect, &got) {
                (Some(want), Some(have)) => assert_eq!(
                    want, have,
                    "kind `{}` crossed to the wrong identifier",
                    row.kind
                ),
                (None, None) => {}
                (want, have) => panic!(
                    "kind `{}` disagrees with the shared artefact: expected {want:?}, translator produced {have:?}. \
                     Either add the arm on this side or amend agentbox/schema/federation-kinds.json.",
                    row.kind
                ),
            }
            // A crossing row must actually be marked as crossing, and vice
            // versa — the fixture and the `crosses` flag cannot disagree.
            assert_eq!(
                row.crosses,
                row.fixture.expect.is_some(),
                "kind `{}`: `crosses` and the fixture expectation disagree",
                row.kind
            );
        }
    }

    /// Acceptance 1 + 3: crossings satisfy the declared target grammar, and the
    /// fixture targets — which were computed by the JS hasher — are reproduced
    /// byte-for-byte by this side's independent sha256. That extends the
    /// `entity_urn_matches_uris_js_golden` proof from the content address alone
    /// to the whole crossing.
    #[test]
    fn federation_crossings_match_declared_grammar() {
        for row in federation_kinds().kinds.iter().filter(|k| k.crosses) {
            let grammar = row
                .target_grammar
                .as_deref()
                .unwrap_or_else(|| panic!("crossing kind `{}` needs a target_grammar", row.kind));
            let out = cross_from_agentbox(&row.fixture.input)
                .unwrap_or_else(|| panic!("crossing kind `{}` must cross", row.kind))
                .visionclaw_id;
            assert!(
                grammar_to_regex(grammar).is_match(&out),
                "kind `{}` produced `{out}`, which does not match declared grammar `{grammar}`",
                row.kind
            );
        }
    }

    /// A crossing row naming a target this translator cannot mint would drop
    /// silently at the wildcard arm. Fail loudly instead — this is the gate that
    /// makes a one-sided artefact addition a build failure.
    #[test]
    fn federation_targets_all_have_an_arm() {
        for row in federation_kinds().kinds.iter().filter(|k| k.crosses) {
            let target = row
                .target_kind
                .as_deref()
                .unwrap_or_else(|| panic!("crossing kind `{}` needs a target_kind", row.kind));
            assert!(
                RUST_HANDLED_TARGETS.contains(&target),
                "artefact crosses `{}` to `{target}`, which cross_from_agentbox has no arm for",
                row.kind
            );
        }
    }

    /// A refusal must be *recorded*, with a reason, and a `deliberate` one must
    /// name both the target it would reach and the arguments that would unlock
    /// it — that is what keeps it distinguishable from "not implemented".
    #[test]
    fn federation_refusals_are_recorded_not_absent() {
        let mut deliberate = 0;
        for row in federation_kinds().kinds.iter().filter(|k| !k.crosses) {
            let reason = row
                .refusal_reason
                .as_deref()
                .unwrap_or_else(|| panic!("refused kind `{}` needs a refusal_reason", row.kind));
            assert!(
                reason.len() > 20,
                "refused kind `{}` needs a substantive reason, got `{reason}`",
                row.kind
            );
            match row.refusal_class.as_deref() {
                Some("deliberate") => {
                    deliberate += 1;
                    assert!(
                        row.target_kind.is_some(),
                        "deliberate refusal `{}` must name the target it would reach",
                        row.kind
                    );
                    let elev = row.elevation.as_ref().unwrap_or_else(|| {
                        panic!("deliberate refusal `{}` must name its unlock", row.kind)
                    });
                    assert!(
                        !elev.required_args.is_empty(),
                        "deliberate refusal `{}` must list the missing arguments",
                        row.kind
                    );
                }
                Some("not-federated") => assert!(
                    row.elevation.is_none(),
                    "`{}` is not-federated, so it must not carry an elevation unlock",
                    row.kind
                ),
                other => panic!("kind `{}` has unknown refusal_class {other:?}", row.kind),
            }
        }
        assert!(
            deliberate > 0,
            "the artefact must record at least one deliberate refusal (memory)"
        );
    }

    /// Scope-bearing crossings refuse an input with no 64-hex owner scope, on
    /// both sides, rather than minting an unowned identifier.
    #[test]
    fn federation_unscoped_inputs_are_refused() {
        let mut checked = 0;
        for row in &federation_kinds().kinds {
            let Some(fixture) = row.unscoped_fixture.as_ref() else {
                continue;
            };
            checked += 1;
            assert!(
                fixture.expect.is_none(),
                "an unscoped fixture must expect a refusal (`{}`)",
                row.kind
            );
            assert!(
                cross_from_agentbox(&fixture.input).is_none(),
                "kind `{}` crossed without an owner scope: `{}`",
                row.kind,
                fixture.input
            );
        }
        assert!(checked > 0, "expected at least one unscoped fixture");
    }

    /// The already-converged identity pass-through, and its malformed twin.
    #[test]
    fn federation_identity_passthrough_matches_artefact() {
        let ip = &federation_kinds().identity_passthrough;
        let crossed = cross_from_agentbox(&ip.fixture.input)
            .expect("a well-formed did:nostr must pass through");
        assert_eq!(Some(crossed.visionclaw_id.clone()), ip.fixture.expect);
        assert_eq!(crossed.owner_did, Some(crossed.visionclaw_id.clone()));
        assert!(grammar_to_regex(&ip.target_grammar).is_match(&crossed.visionclaw_id));
        assert!(
            cross_from_agentbox(&ip.malformed_fixture.input).is_none(),
            "a malformed did:nostr must be refused, not passed through"
        );
    }
}
