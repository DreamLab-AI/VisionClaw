# ADR-079 — Forum-Setup Skill Provider Abstraction

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-011 G5, F7 |
| Companion ADRs | ADR-073, ADR-074, ADR-075, ADR-076, ADR-077, ADR-078 |
| Companion PRDs | PRD-010, PRD-011 |
| Affected repos | `nostr-rust-forum` (canonical kit), `agentbox` (skill registry), `dreamlab-ai-website` (downstream consumer) |

## Context

PRD-011 G5 mandates an AI-assisted configurator that helps operators author per-deployment TOML configurations for the VisionFlow forum kit (`nostr-bbs-rs`). The kit's TOML schema (PRD-011 §5.2) has ~25 customisable surfaces — branding, zones, trust progression, moderation kinds, federation peers, rate limits, feature flags, etc. — too many for a "fill in the blanks" template approach but small enough that a guided conversation can produce a valid configuration in under 15 minutes.

The user instruction (Q8: b): "One skill with provider abstraction (Codex / Claude Code / agentbox-nostr / API-key swappable)". The provider question is operationally substantial — different operators have different LLM access patterns:

- **Claude Code** users have it locally; want skill invocation via `/forum-setup`
- **OpenAI Codex** users via the OpenAI API
- **Agentbox-nostr** consumers want the skill running inside the agentbox sovereign agent, dispatched via NIP-1059 DM
- **Direct API key** users (e.g. CI runners) want non-interactive operation with `--api-key` and pre-supplied answers

A naive implementation forks five separate skills. A clean implementation defines a `Provider` trait with five impls, all consuming a single conversation flow.

## Decision

### D1 — Single conversation flow, swappable provider

The `forum-setup` skill is **one** crate (`nostr-bbs-setup-skill`) with **one** conversation flow defined in `src/conversation.rs`. Provider abstraction lives in `src/providers/` — a `Provider` trait with five implementations.

```rust
// nostr-bbs-setup-skill/src/lib.rs

pub trait Provider: Send + Sync {
    /// Send a question to the LLM and parse a structured response.
    async fn ask(&self, prompt: &Prompt) -> Result<Response, ProviderError>;

    /// Provider identifier for logging / telemetry.
    fn id(&self) -> &'static str;

    /// Capabilities (e.g. structured-output support, max context window).
    fn capabilities(&self) -> Capabilities;
}

pub struct Prompt {
    pub system_message: String,
    pub conversation_history: Vec<Message>,
    pub user_question: String,
    pub expected_schema: Option<JsonSchema>,
}

pub struct Response {
    pub text: String,
    pub structured: Option<serde_json::Value>,
    pub usage: TokenUsage,
}
```

### D2 — Five provider implementations

| Provider | Module | Transport |
|----------|--------|-----------|
| `ClaudeCodeProvider` | `providers/claude_code.rs` | Reads from `$CLAUDE_CODE_SOCKET` (the active Claude Code session); calls back into Claude. Default for Claude Code skill invocation. |
| `CodexProvider` | `providers/codex.rs` | OpenAI Codex API via `OPENAI_API_KEY`. |
| `AgentboxNostrProvider` | `providers/agentbox_nostr.rs` | Sends conversation to an agentbox sovereign agent via NIP-1059 wrapped DM (per ADR-075 IS-Envelope kind=`tool_invoke`); receives result via `tool_result`. |
| `AnthropicApiProvider` | `providers/anthropic.rs` | Anthropic API direct via `ANTHROPIC_API_KEY`. |
| `OpenAiApiProvider` | `providers/openai.rs` | OpenAI API direct via `OPENAI_API_KEY`. |

Each provider implements `Provider`. Selection at runtime via `--provider <id>` flag or `[setup_provider]` config block.

### D3 — Conversation flow (~15 questions)

Defined declaratively in `src/conversation.rs`:

```rust
pub fn build_conversation_flow() -> Vec<Question> {
    vec![
        Question::new("deployment_name")
            .ask("What's your deployment called? (e.g. 'Alpha Centauri Forum')")
            .validate(NonEmpty)
            .infer_target("deployment.name"),

        Question::new("deployment_hostname")
            .ask("What hostname will your forum be reachable at? (e.g. https://forum.example.com)")
            .validate(HttpsUrl)
            .infer_target("deployment.hostname"),

        Question::new("webauthn_rp_id")
            .derived_from("deployment_hostname", |url| extract_etld_plus_one(url))
            .confirm("WebAuthn RP ID is '{value}'. Confirm or override?")
            .infer_target("webauthn.rp_id"),

        // ... 12+ more questions
    ]
}
```

Per-question:
- `ask(...)` text shown to user
- `validate(...)` rules (HttpsUrl, NonEmpty, Pubkey64Hex, IntRange, etc.)
- `infer_target(...)` TOML path the answer populates
- Optional `derived_from(...)` for inferring defaults from earlier answers
- Optional `confirm(...)` for auto-derived values
- Optional `branch_on(...)` for conditional question chains (e.g. don't ask about welcome bot if invites disabled)

### D4 — Question state machine

The skill maintains a `ConfigBuilder` that accumulates answers into a partial TOML. After each provider response:
1. Validate the answer against the question's `validate` rule.
2. On valid: write to `ConfigBuilder` at the `infer_target` path.
3. On invalid: re-ask with explanation.
4. On all questions answered: serialise to `<deployment>.toml`; run final TOML schema validator (PRD-011 F3.3); if valid, write to disk; else re-prompt to correct.

### D5 — Structured output preference

Where the provider supports structured outputs (Anthropic tool-use, OpenAI JSON mode), the skill uses them — sends a JSON schema for each answer, expects structured response. Falls back to free-text + parsing for providers without structured-output support.

```rust
async fn ask(&self, prompt: &Prompt) -> Result<Response, ProviderError> {
    if self.capabilities().supports_structured_output && prompt.expected_schema.is_some() {
        self.ask_structured(prompt).await
    } else {
        self.ask_free_text(prompt).await
    }
}
```

### D6 — Agentbox-nostr provider via IS-Envelope

The agentbox-nostr provider is the most novel. Mechanism:

1. Skill encodes the question as IS-Envelope (per ADR-075 D3) with `kind = "tool_invoke"`:
   ```jsonc
   {
     "v": 1,
     "to":   "did:nostr:<agentbox_agent_pubkey>",
     "from": "did:nostr:<operator_pubkey>",
     "kind": "tool_invoke",
     "body": {
       "tool":     "urn:agentbox:skill:nostr-bbs-setup-llm-call",
       "args":     { "system_message": "...", "history": [...], "user_question": "..." },
       "reply_to": "<this_event_id>"
     }
   }
   ```
2. Skill wraps in NIP-59 gift-wrap, publishes to operator's preferred relay.
3. Agentbox `RelayConsumer` (per PRD-010 F16) picks up event; `tool_invoke` dispatched to `urn:agentbox:skill:nostr-bbs-setup-llm-call` (a sub-skill that proxies to agentbox's LLM).
4. Agent computes response, encodes IS-Envelope `kind = "tool_result"`, publishes back.
5. Skill receives result, parses, advances conversation.

Latency: ~5-30s per question (depends on relay round-trip + agent LLM). Acceptable for an interactive setup wizard but slower than direct API providers. Trade-off: operator who runs agentbox can use the skill without provisioning external API keys.

### D7 — Telemetry & cost tracking

Each provider reports `TokenUsage { prompt_tokens, completion_tokens }`. Skill logs total cost to `~/.nostr-bbs-setup/<run-id>.log`. Operators can audit AI spend per setup.

### D8 — Determinism & replayability

The skill records every question + answer + provider response to `<run-id>.jsonl`. Operators can `nostr-bbs-setup-skill replay <run-id>` to reproduce the setup deterministically without re-querying the LLM. Useful for CI deployments and for migrating between providers.

### D9 — Error recovery

On provider failure (rate limit, network, invalid response), skill exits with informative error AND a partial TOML at `<deployment>.toml.partial`. Operator can resume with `--resume <run-id>`. Idempotent.

### D10 — Skill registration

The skill is registered in three places:

1. **agentbox skills directory**: `agentbox/skills/nostr-bbs-setup/SKILL.md` — operator runs `agentbox skill nostr-bbs-setup` from inside an agentbox container.
2. **Claude Code skills**: ships an entry in `~/.claude/skills/forum-setup.md` so operators can run `/forum-setup` inside Claude Code.
3. **Standalone CLI**: `cargo install nostr-bbs-setup-skill` provides `nostr-bbs-setup-skill wizard --provider <id>`.

All three entry points invoke the same crate; only the provider differs.

### D11 — Configuration

Skill itself takes a small config:

```toml
# ~/.nostr-bbs-setup/config.toml (optional)
[setup_provider]
default = "claude-code"

[setup_provider.claude_code]
# (no config; uses active Claude Code session)

[setup_provider.codex]
api_key_env = "OPENAI_API_KEY"
model = "gpt-4o"

[setup_provider.agentbox_nostr]
operator_nsec_env = "OPERATOR_NSEC"
agent_pubkey      = "<hex>"
relay_url         = "wss://..."

[setup_provider.anthropic]
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-opus-4-7"

[setup_provider.openai]
api_key_env = "OPENAI_API_KEY"
model = "gpt-4o"
```

CLI args override config; config overrides defaults.

### D12 — Security: operator keys never sent to providers

The operator's Nostr secret key (used for federation, mesh service-list signing) is **never** included in skill prompts. The skill reasons about TOML structure only; key generation is operator-local at deployment time. Validators ensure the skill never asks the LLM "give me a fresh nsec" — keys are generated by `nostr-bbs-admin` on first boot, never by the configurator.

### D13 — System prompt template

The skill ships a fixed system prompt (`src/system_prompt.md`) that tells the LLM:
- It is helping configure a `nostr-bbs-rs` forum deployment.
- It must produce only the structured fields requested.
- It must not invent values for security-sensitive fields (admin pubkeys, secret keys, API keys).
- It must use TOML conventions and validate suggestions against the schema (loaded from PRD-011 §5.2).

This guards against prompt injection and against LLM-generated nonsense in security-critical fields.

## Consequences

### Positive

- **Single source of truth for the conversation flow**: questions, validators, schema target paths all in one declarative file.
- **Provider parity**: same wizard works in Claude Code, Codex, agentbox-nostr, or via direct API. Operators choose based on their LLM access pattern.
- **Federation-native**: agentbox-nostr provider demonstrates the IS-Envelope `tool_invoke`/`tool_result` flow end-to-end, doubling as integration test for ADR-075.
- **Auditable**: per-run logs enable cost tracking + reproducibility.
- **Security-conscious**: secret material never flows through LLM context; operator-local key generation only.

### Negative

- **Five provider implementations to maintain**: API surfaces drift; each provider needs occasional bumps.
- **agentbox-nostr provider is slowest path**: 5-30s per question; acceptable for guided wizard but worse than direct API.
- **System prompt drift risk**: as TOML schema evolves, system prompt must evolve too. Mitigation: schema-driven prompt generation (where possible).

### Neutral

- **Skill is opt-in**: operators can write TOML by hand (PRD-011 §5.2 schema fully documented); the skill is a productivity multiplier, not a gate.

## Alternatives Considered

### Alt-A — Five separate skills, one per provider

Each provider gets its own conversation flow + binary.

*Rejected*: 5x conversation-logic duplication; updates to question wording have to land in 5 places.

### Alt-B — Web-based setup UI (no LLM)

Skip the LLM; ship a hosted web wizard at `setup.nostr-bbs.dev`.

*Rejected (for now)*: requires hosting infrastructure; doesn't help operators who deploy in air-gapped environments. Could be added as a sixth "provider" later.

### Alt-C — Question file format only (no LLM, fillable)

Operator fills out a YAML/TOML template manually.

*Rejected*: PRD-011 §5.2 schema is large enough that templates are tedious. The skill's value is the conversational guidance + validation + auto-derivation of dependent fields (e.g. WebAuthn RP ID from hostname).

### Alt-D — Anthropic-only (no provider abstraction)

Build only on Claude API.

*Rejected per Q8:b*: operator preferences vary; abstraction is the expected model.

## Implementation notes

### Crate layout

```
nostr-bbs-setup-skill/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs              # public API + Provider trait
│   ├── conversation.rs     # declarative question flow
│   ├── config_builder.rs   # accumulator + TOML serialiser
│   ├── validators.rs       # NonEmpty, HttpsUrl, Pubkey64Hex, etc.
│   ├── system_prompt.md    # fixed LLM system prompt
│   ├── replay.rs           # deterministic replay support
│   ├── providers/
│   │   ├── mod.rs
│   │   ├── claude_code.rs
│   │   ├── codex.rs
│   │   ├── agentbox_nostr.rs
│   │   ├── anthropic.rs
│   │   └── openai.rs
│   └── bin/
│       └── nostr-bbs-setup-skill.rs  # CLI entry
├── tests/
│   ├── conversation_flow.rs   # property test: every path produces valid TOML
│   ├── provider_contract.rs   # all providers satisfy same Provider contract
│   └── fixtures/
│       ├── happy-path.jsonl   # canonical wizard run
│       └── recovery.jsonl     # mid-run failure + resume
└── examples/
    └── basic-cli.rs           # minimal driver
```

### Validators ship with the schema

`nostr-bbs-config` crate exports validators consumed by the skill. Adding a new schema field automatically gets a default validator; operator-defined custom validators override.

### Tests

Per ADR-077 P1-P5:
- Conversation flow property test: random valid input → produces valid TOML.
- Provider contract test: every provider answers a fixed prompt with structurally valid response.
- Replay determinism test: same `<run-id>.jsonl` → same TOML.
- Reference test vectors for IS-Envelope `tool_invoke` shape (agentbox-nostr provider).
- Mutation testing target ≥80% on `conversation.rs` + `config_builder.rs`.

## References

- PRD-011 — VisionFlow Forum Kit Extraction, G5, F7
- ADR-073 — Mesh topology
- ADR-074 — DID:Nostr canonicalisation
- ADR-075 — IS-Envelope (tool_invoke + tool_result kinds used by agentbox-nostr provider)
- ADR-077 — Ecosystem QE Policy (skill complies)
- ADR-078 — Cross-substrate library convergence (skill consumes upstream `nostr` + LLM SDK crates)
- Anthropic SDK: https://github.com/anthropics/anthropic-sdk-rust
- OpenAI Rust client: https://crates.io/crates/openai
- Claude Code skill format: ~/.claude/skills/<name>.md
- agentbox skills directory pattern: see `agentbox/skills/`
- GitHub repos:
  - https://github.com/DreamLab-AI/nostr-rust-forum (kit hosting the skill)
  - https://github.com/DreamLab-AI/agentbox (skill registry consumer)
  - https://github.com/DreamLab-AI/VisionClaw (mesh integration; references this ADR)
