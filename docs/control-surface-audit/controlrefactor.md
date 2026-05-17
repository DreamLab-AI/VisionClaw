# Control Surface Audit

Branch: `feature/unified-control-surface`. Compiled 2026-04-28.

This audit maps every control / display surface in the project, every settings entity behind those surfaces, and every disconnect between the two. It then proposes an aspirational entity-level UI inventory.

## Files

| File | Purpose |
|---|---|
| [`raw/01-auth-surface.md`](raw/01-auth-surface.md) | Sign-in / Onboarding / NIP-98 + passkey / dev login / access-control middleware |
| [`raw/02-control-center.md`](raw/02-control-center.md) | In-app Control Center panel — interactive mode, AI text-entry, advanced toggle, all 180+ leaf controls |
| [`raw/03-enterprise-management.md`](raw/03-enterprise-management.md) | Enterprise Standalone (Broker / Workflows / KPI / Connectors / Policy + WASM drawer_fx), Contributor Studio, Health/Monitoring |
| [`raw/04-server-settings.md`](raw/04-server-settings.md) | `AppFullSettings` schema, every settings HTTP/WS route, validation rules, feature flags, dev_config, back-end-only tunables |
| [`raw/05-client-plumbing.md`](raw/05-client-plumbing.md) | Settings store, settingsApi, autoSaveManager, retry manager, defaults pipeline, WS subscriptions |
| [`raw/06-ancillary-config.md`](raw/06-ancillary-config.md) | Env vars, agentbox manifest knobs, ADR-defined settings, compose orchestration |
| [`comprehensive-settings-table.md`](comprehensive-settings-table.md) | Single inventory of every settings entity across all surfaces, with status flags (LIVE / CLIENT-ONLY / SERVER-ONLY / STUB / DEAD / DUPLICATE / DEPRECATED) |
| [`aspirational-inventory.md`](aspirational-inventory.md) | Same entities, rationalised. Decision codes (KEEP / PROMOTE / DEMOTE / EXPOSE / HIDE / CUT / MERGE / WIRE / VALIDATE / AUTH-GATE) with inline justification |

## How it was produced

Six research agents ran in parallel, each scoped to a non-overlapping concern. Outputs landed in `raw/`. The two synthesis documents draw entirely from those raw files; they do not introduce new findings, only consolidate and rationalise.

## What this audit deliberately did not do

- Did not change any code outside `docs/control-surface-audit/`.
- Did not make grouping, layout, visual-design, or interaction-pattern decisions.
- Did not specify naming above the entity-key level (no copy/labels).
- Did not ignore broken connections — captured them all in the disconnects sections.

## Headline numbers

- **~205** leaf controls in the Control Center today, across **10 tabs**.
- **14** root server settings fields, **40+** nested structs.
- **120+** env vars, **15** agentbox knobs, **15** compose knobs.
- **70+** server-side tunables with no UI today.
- **30** distinct cross-cutting disconnects identified.
- After rationalisation: **~140-150** entities at user tiers 1-2, **~25** at PU tier 3, the rest moved to operator tier 4 or cut.

## Next steps (out of scope for this audit)

1. Review the aspirational inventory's **CUT** and **MERGE** decisions — these are the cheapest wins in tech debt reduction.
2. Decide which **WIRE** entities are real product intents (XR, studio backend, policy persistence, NL command, settings profiles) vs cuts.
3. Once entity set is agreed, start the design-language conversation: grouping, disclosure, naming, theming.
4. Schema work: pick canonical paths for the **MERGE** decisions; deprecate the duplicates with serde aliases for one release.

Replace the 10-tab, 205-knob settings surface with a single scrollable list of plain-English sentences. Each sentence describes the current state of the graph and expands in place to reveal the controls that produced it.

Contents
Problem
Proposal
Why this over the alternatives
Goals & non-goals
Users & tiers
Data model: the Setting descriptor
The summary function
The spine renderer
Key interactions
How audit decisions land
Migration plan
Risks & open questions
Success metrics
Out of scope
01Problem
The current Control Center surfaces ~205 leaf settings across 10 tabs, with a ~70-knob server-only tail and ~30 cross-cutting disconnects (CC-AI keyword regex, dormant /api/nl-query/*, four overlapping pubkey allowlists, etc). The audit tagged each control with a decision: KEEP CUT MERGE EXPOSE WIRE.

The audit is the easy part. The hard part is the shape of the rationalised UI. Today's tab structure cannot absorb the merges without becoming inconsistent — a "Render quality" preset alongside the three booleans it folds in is worse than either alone.

Every tab today answers: what knobs exist? The spine answers: what is the graph doing right now, and what would I touch to change it?

02Proposal
One scrollable list. Every row is a sentence describing current state. Click a row to expand the underlying knobs in place. No tabs, no per-panel "Show advanced" toggles. Tier gating is a single global filter, not a separate UI.

Concretely:

Each setting becomes a descriptor — a small object that declares its tier, category, summary function, and inline editor. The Control Center is then a flat array of these.
Merged settings are first-class. "Render at standard quality" is one descriptor that folds in aa, shadows, ao, envIntensity. The folded knobs no longer have their own row — they only appear inside the parent's expanded editor.
Tier 3 (power-user) and tier 4 (operator) are visual dividers in the same scroll, not separate pages. The pubkey gate filters them out for non-eligible users.
Search is free. Fuzzy-match against the rendered summary text — users typing "shadows" find the "Render quality" row.
03Why this over the alternatives
Six directions were sketched. The spine wins on the dimensions that matter most for VisionClaw specifically:

Direction	Strength	Why not now
Tier mode	Familiar mental model	Doesn't reduce control count — only hides it
Preset-first	Lowest cognitive load	Power users hate "lifting out of preset"; doesn't scale to ops settings
Instrument rack	Tactile, memorable	Heavy build; suits performers more than analysts
Command-first	Wires the dormant NL endpoints	Strong companion, weak primary — keyboard-first users only
Patch bay	Best PU surface for grants	Solves a sub-problem, not the whole surface
Inline spine	Absorbs every audit decision into one shape	Summary writing is real work
The spine is also the most additive: command-first and patch-bay can ship later as overlays without rework.

04Goals & non-goals
Goals
Reduce visible-at-tier-1 control count from ~205 to ~40 summary rows.
Make every visible row read as a sentence about current state, not a label.
Land all 11 audit MERGE decisions, all 12 CUTs, the 35 EXPOSEs and 25 WIREs in one shape.
Deep-link any row (?expand=render.quality) for support and team handoff.
Power user and operator settings live in the same scroll, not a separate app.
Non-goals (this PRD)
Replacing the in-graph HUD pill (knowledge/ontology/agent toggles) — stays as is.
Building the NL command surface — separate PRD; spine should be a willing host.
Re-theming. Spine ships in current visual language.
Mobile. Desktop only at v1.
05Users & tiers
Tier	Who	Visible by default	Gated by
1 — Basic	Analysts, viewers	~25 rows	none
2 — Advanced	Heavy users	~40 rows	per-user toggle (sticky)
3 — Power	Owners, integrators	+ ~25 rows after divider	NIP-98 pubkey allowlist
4 — Operator	SRE, support	+ deployment block (read-only)	role claim
06Data model: the Setting descriptor
The single new abstraction. Every existing tab becomes an array of these.

type Setting<T> = {
  id: string;                       // "render.quality"
  path: string[];                    // store lookup
  tier: 1 | 2 | 3 | 4;
  category: Category;             // visual | behaviour | data | team | power | operator

  label: string;                    // short name, used in search & breadcrumbs
  summary: (v: T, ctx: Ctx) => string; // THE NEW DESIGN WORK

  Editor: React.FC<{ value: T; onChange: (v: T) => void }>;

  folds?: string[];                  // ids of merged child settings
  decision?: AuditDecision;          // drives the chips when annotations on
  ref?: string;                       // "§3.4-6" — back to audit
  readOnly?: boolean;                 // tier 4 ops
};
Adding a setting is now adding one object to an array. The store schema and the spine schema are decoupled — descriptors read/write through path, so the existing Redux/Zustand layer doesn't move.

07The summary function
This is the design work, not a styling pass. A good summary describes state, not the control. Examples:

Bad	Good
Quality: standard	Render at standard quality (AA on, shadows on, AO off)
Show clusters: true	Cluster tightness: tight
Boundary damping: 0.85	Boundary feel: standard
Auto-pause threshold: 0.04	Auto-pause when settled (kinetic < 0.04)
Verbosity: 2	Diagnostics: errors only
const renderQuality: Setting<RenderState> = {
  id: 'render.quality',
  label: 'Render quality',
  tier: 1,
  category: 'visual',
  folds: ['render.aa', 'render.shadows', 'render.ao', 'render.envIntensity'],
  decision: 'MERGE',
  ref: '§3.4-6',
  summary: (v) => {
    const preset = detectPreset(v);  // 'lite' | 'standard' | 'high' | 'custom'
    if (preset === 'custom') return 'Render at custom quality';
    return `Render at ${preset} quality`;
  },
  Editor: RenderQualityEditor,
};
Authoring rules for summaries
Present tense, declarative. "Showing knowledge nodes only." not "Show only knowledge nodes."
Numbers only when meaningful at a glance — kinetic thresholds yes; raw spring constants no.
Compound state needs detectPreset() + 'custom' fallback. Never crash the row.
Localisation-ready: summary functions return strings via an i18n helper.
08The spine renderer
~80 lines. Stateless on top of the existing store.

function Spine({ settings, state, dispatch, expandedId, setExpandedId }) {
  const visible = settings.filter(s => s.tier <= currentTier(state));
  const grouped = groupBy(visible, s => s.category);

  return (
    <div className="spine">
      {Object.entries(grouped).map(([cat, items]) => (
        <Section key={cat} title={cat} divider={cat === 'power' || cat === 'operator'}>
          {items.map(s => (
            <SettingRow
              key={s.id}
              setting={s}
              value={getPath(state, s.path)}
              expanded={expandedId === s.id}
              onToggle={() => setExpandedId(expandedId === s.id ? null : s.id)}
              onChange={(v) => dispatch({ type: 'set', path: s.path, value: v })}
            />
          ))}
        </Section>
      ))}
    </div>
  );
}
Single-row expansion (accordion) at v1 — only one row open at a time. Multi-row open is a v2 power-user setting.

09Key interactions
Search
Top-of-spine input (also ⌘F). Fuzzy match against summary(value) + label + folded child labels. Matched rows highlight; non-matches dim. Hitting Enter expands the top match.

Deep links
URL state: ?expand=render.quality&tier=power. Support links straight to the row in question. Right-click a node in the graph: "edit how this is shown" → links to the relevant row, scrolls, expands.

Drift indication
If a folded child has been edited away from its parent's preset shape, the row's summary shows the 'custom' form. A "reset to preset" affordance appears in the editor.

Audit annotations
Off by default for end users. Internal toggle (?annotate=1 or settings flag) shows the decision chip plus the § reference per row — same as the wireframe Tweaks panel does.

10How audit decisions land
Decision	What changes in the descriptor
KEEP	Descriptor exists, summary rephrased to state-form.
CUT	No descriptor. Store key may persist for backward-compat read.
MERGE	Parent descriptor with folds: [childIds]. Children have no top-level descriptor; rendered only inside parent editor.
EXPOSE	New descriptor wired to a previously server-only key. Tier set per audit.
WIRE	Descriptor exists; Editor calls the new transport (NL query, audit trail, GitHub sync, etc).
Approximate landing: ~140 visible descriptors total · ~25 at tier 1 default · ~12 cuts · 11 merges (each retiring 3-12 children) · 35 exposes · 25 wires.

11Migration plan
Phase 0 — Scaffolding (1-2 weeks)
Land the Setting<T> type, Spine component, SettingRow, deep-link plumbing.
Feature-flag spine.enabled per pubkey. Both surfaces co-exist; same store.
Author 5 descriptors covering one tab (Visual) end-to-end. Internal dogfood.
Phase 1 — Visual category (2 weeks)
Highest leverage; most MERGE wins. Land render.quality, glow, metadata viz, node visibility, cluster tightness. Delete the Visual tab.

Phase 2 — Behaviour, Data, Team (3-4 weeks)
Includes the boundary-feel and detail-policy merges. Tier 2 reveal logic gets exercised here.

Phase 3 — Power & Operator (2 weeks)
Pubkey-gated section. Diagnostics tri-state, GitHub sync (WIRE), audit trail (WIRE), feature grants. Operator block is read-only descriptors only.

Phase 4 — Cleanup
Remove tab shell. Remove dead store keys. Flip spine.enabled default to true.

12Risks & open questions
Summary copy is real work, easy to underestimate. Treat as content design, not strings. Budget review with two technical writers across phases 1-3.
Preset detection edge cases will produce ugly "custom" labels until tuned. Add telemetry on 'custom' rate per merge — anything > 30% is a sign the preset shape is wrong.
Single-accordion expansion may frustrate users comparing two settings. Mitigation: ⌘click opens a row in a side popover without collapsing the current one.
Discoverability of folded children: a user looking for "shadows" must find it. Search has to index folded children, not just parent labels.
Tier 4 read-only block: should it be in the spine at all, or in a separate /ops route? Lean: spine, with strong divider. Confirm with SRE.
Per-user tier-2 reveal: do we infer from usage, or require explicit opt-in? Lean: opt-in toggle in the spine itself (a tier-1 row labelled "Show advanced settings: off / on").
13Success metrics
Metric	Today	Target (90 days post-launch)
Visible controls at first open	~205	≤ 40
Median time to find named setting (usability test)	~38s	≤ 12s
Settings touched per session (engagement, not noise)	2.1	3.5+
Support tickets tagged "can't find setting"	baseline	−60%
Telemetry: rows opened that read 'custom'	n/a	< 30% per merge
PU adoption of GitHub sync (WIRE)	0	40% of PU pubkeys
