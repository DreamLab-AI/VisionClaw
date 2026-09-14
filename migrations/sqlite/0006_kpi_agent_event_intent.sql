-- 0006_kpi_agent_event_intent — PRD-augmentation-conditions FR5.1 (EXP-AC-005).
--
-- Adds the agent's DECLARED intent to the agent-event trajectory projection.
--
-- The column is the durable home of `AgentActionEnvelope::intent` (D7, pre-action
-- intent legibility). It is written VERBATIM by the `/wss/agent-events` hub tap
-- and is NULL for an envelope whose producer declared no intent. It is never
-- back-filled from `action_type_name`: `intent_match` compares the declaration
-- with the act, so a synthesised intent would make every act agree with itself.
--
-- Applies to the KPI database (`data/kpi.sqlite3`). The authoritative embedded
-- copy is `sqlite_kpi_repository::CREATE_SCHEMA` + `apply_additive_migrations`
-- (self-bootstrapping, ADR-11 §D1); this file is the human-authoring source and
-- must be kept in step with it.

ALTER TABLE kpi_agent_events ADD COLUMN intent TEXT;

INSERT OR IGNORE INTO schema_migrations (id) VALUES ('0006_kpi_agent_event_intent');
