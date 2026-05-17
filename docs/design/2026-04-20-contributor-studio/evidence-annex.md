---
title: Evidence Annex — Contributor AI Support Stratum
description: Industry research evidence backing PRD-003 and ADR-057. Ten load-bearing claims extracted from PwC 2026, McKinsey Manifesto, a16z, Ramp Glass, and Anthropic Skill Creator v2.
category: design
tags: [research, evidence, contributor, enablement, industry-research]
updated-date: 2026-04-20
---

# Evidence Annex — Contributor AI Support Stratum

## Purpose

This annex is the single cite-ready source of external evidence backing the Contributor AI Support Stratum work (PRD-003, ADR-057, the DDD explanation, and the four design specs under `docs/design/2026-04-20-contributor-studio/`). It exists because VisionClaw already has a strong technical substrate (graph, ontology, pods, agents, GPU physics) and a strong management mesh (Judgment Broker workbench, Workflow lifecycle, KPI, Policy Engine), but the contributor-facing harness — the daily workspace where a human and their agents do knowledge work together — is missing. Before writing PRD-003 we did a corpus read of the 2026-04-20 *AI Daily Brief* long-read episode plus the Anthropic `skill-creator` v2 release, and distilled ten load-bearing claims. Downstream authors should cite this file rather than re-reading the corpus; when the claim you need isn't here, extend the annex rather than smuggling uncited assertions into the PRD.

## Source map

| # | Source | Type | Date | Canonical location | Fidelity |
|---|--------|------|------|--------------------|----------|
| S1 | PwC 2026 AI Performance Study | Research report (press release + PDF) | 13 Apr 2026 | `presentation/2026-04-20-best-companies-ai/01-pwc-2026-ai-performance-study.md` | High — numbers from PwC press release (via Consultancy.me mirror), cross-checked against PwC's own `roi-from-ai.pdf` and multiple 13–20 April 2026 secondary outlets. pwc.com returned HTTP 403 to programmatic fetches. |
| S2 | McKinsey AI Transformation Manifesto (`Rewired`, 2nd ed. excerpt) | Research paper / book excerpt | Apr 2026 | `presentation/2026-04-20-best-companies-ai/02-mckinsey-ai-transformation-manifesto.md` | High — 12 themes reconstructed from podcast narration plus QuantumBlack/Brian Heger, Victorino Group, and Kriaris substack analyses. McKinsey PDF gated behind client wall. |
| S3 | a16z — *Institutional AI vs Individual AI* (George Sivulka) | Essay | 12 Mar 2026 | `presentation/2026-04-20-best-companies-ai/03-a16z-institutional-vs-individual-ai.md` | High — fetched in full from `a16z.news`. |
| S4 | Ramp Glass — Seb Goddijn X thread ("We Built Every Employee at Ramp Their Own AI Coworker") | Blog post / long-form X thread | Apr 2026 | `presentation/2026-04-20-best-companies-ai/04-ramp-glass-seb-goddijn.md` | Medium — original thread paywalled to automated fetches; markdown reconstructed from transcript verbatim quotes plus Midas Tools and Department of Product secondary coverage. |
| S5 | Ramp Glass — Shane Buchan LinkedIn follow-up ("How We Actually Built It") | Engineering retrospective | Apr 2026 | `presentation/2026-04-20-best-companies-ai/04b-ramp-glass-shane-buchan-how-we-built-it.md` | High — LinkedIn article text preserved. |
| S6 | Anthropic Skill Creator v2 | Tooling release + community coverage | Mar–Apr 2026 | `presentation/2026-04-20-best-companies-ai/90-anthropic-skill-creator-v2.md` | High — `anthropics/skills` GitHub repo plus Dev.to, Tessl, Medium, Tool Nerd coverage and a YouTube walkthrough transcript quote. |

The `README.md` in the source folder (`presentation/2026-04-20-best-companies-ai/README.md`) records these fidelity notes in more detail; read it before extending the annex.

## The ten load-bearing claims

Each claim is numbered C1..C10 and carries: the claim, the source(s), a direct quote or close paraphrase (marked as such), and an implication for the VisionClaw Contributor Studio. Citations elsewhere in the documentation set should reference these identifiers (e.g. "Evidence Annex C6").

### C1 — Leading firms build AI foundations, not just buy tools

**Source:** S1 (PwC 2026 AI Performance Study).

**Evidence.** PwC's AI Fitness Index measures *60 management and investment practices* split between "AI use" and "AI foundations" (strategy, governance, data, talent, technology, innovation). The top 20% of firms capture roughly 74% of AI-driven economic value, achieve 7.2× higher AI-driven financial performance, and a +4 percentage-point profit-margin gap over peers. Leaders are **3× more likely to have formal processes to scale AI innovations** and **2× as likely to redesign workflows around AI** rather than just adding AI tools. Joe Atkinson (PwC global chief AI officer) is quoted directly:

> "Many companies are busy rolling out AI pilots, but only a minority are converting that activity into measurable financial returns. The leaders stand out because they point AI at growth, not just cost reduction, and back that ambition with the foundations that make AI scalable and reliable." — Joe Atkinson, PwC, 13 April 2026.

**Implication for Contributor Studio.** VisionClaw's substrate (graph, ontology, pods, agent mesh, GPU physics) and management mesh (broker, workflow, KPI, policy) already satisfy the "AI foundations" half of the Fitness Index. The gap is the "AI use" half — the contributor-facing harness that turns foundations into daily practice. Contributor Studio is the vehicle for that half; without it, VisionClaw has a laggard's adoption curve on top of a leader's substrate.

### C2 — Enduring capabilities (not tools) drive compounding advantage; data must be productized

**Source:** S2 (McKinsey *AI Transformation Manifesto*, themes 1, 7, 8).

**Evidence.** McKinsey theme 1: *"Technology alone doesn't create advantage; enduring capabilities do."* Theme 7: *"Tech platforms are strategic assets; invest in them that way … Leading companies manage their platforms strategically with dedicated teams, road maps, budget, target service levels, and users whose needs shape how the platform evolves."* Theme 8, quoting David Baker (2024 Nobel laureate): *"AI needs masses of high-quality data to be useful."* McKinsey then frames the work as **productizing data** so it is *"easy to discover, access, and consume across many AI-powered applications"* followed by **data enrichment** for context and uniqueness. McKinsey's studied cohort of 20 AI leaders delivered an average 20% EBITDA uplift, 1–2 year breakeven, and $3 of incremental EBITDA per $1 invested.

**Implication for Contributor Studio.** The Contributor AI Support Stratum is where VisionClaw converts its existing data products (Neo4j knowledge graph, OWL ontology, Solid Pod memory, agent episodic store) into *everyday contributor consumption*. The stratum is explicitly a **platform investment with users whose needs shape how the platform evolves** — which matches the McKinsey theme-7 definition. Sprint 1's goal (the Studio shell + pod context + ontology rail) is the minimum unit that makes graph/ontology/pod data *"easy to discover, access, and consume"* for a non-specialist contributor.

### C3 — Institutional AI differs from individual AI; individual productivity does not compound

**Source:** S3 (Sivulka, a16z).

**Evidence.** Sivulka's opening thesis:

> "AI has made individuals ~10× more productive, yet no company has become 10× more valuable as a result. Where did the productivity go?"

His analogue is the **1890s electricity paradox**: US textile mills electrified their motors but saw no output gain for thirty years. Output only arrived when mills redesigned the factory floor around electric power. Sivulka's core claim (direct quote): *"productive individuals do not make productive firms."* Individual AI tools create an illusion of progress — *"productivity-maxxing that is largely self-congratulatory with minimal organisational impact."*

**Implication for Contributor Studio.** This claim is the *raison d'être* of the Contributor AI Support Stratum. VisionClaw today has exactly the shape Sivulka warns about: lots of individual AI activity (contributors using Logseq, MCP, CLI, palette, pods independently) and no compounding. The stratum must be designed as the "redesigned factory floor" — a shared workspace, shared skill mesh, shared memory, shared ontology guidance — not as yet another individual tool. ADR-057's decision text should frame the scope as *institutional* enablement.

### C4 — The harness matters more than the model

**Source:** S4 (Goddijn, Ramp Glass).

**Evidence.** The most-quoted line from the Ramp Glass thread is the section header itself: *"The Models are Good Enough, The Harness Isn't."* Goddijn reports **99% adoption of AI tools across Ramp** and then notes people were still stuck:

> "It wasn't that the models weren't good enough or that people lacked ambition — they just had no idea how to improve their set up. Terminal windows, npm installs, and MCP configurations were too much for most people to grok, and the few who pushed through had wildly different setups, with no way to share what they'd learned."

And:

> "The models are already exceptional, but most people use them like driving a Ferrari with the handbrake on."

**Implication for Contributor Studio.** VisionClaw's contributor base is in exactly the "Ferrari with the handbrake on" state: MCP servers exist, skills exist, pods exist, the graph is rich — but the daily UX is fragmented across Logseq, CLI, MCP, palette, and pods. PRD-003's product statement should lead with "harness, not model" — the goal is not a better agent, it is a workspace that makes the existing agents useful to non-specialists. This is the single strongest external precedent for ADR-057's existence.

### C5 — One person's breakthrough must become everyone's baseline (raise the floor, don't lower the ceiling)

**Source:** S4 (Goddijn, Ramp Glass).

**Evidence.** Goddijn's second core principle, verbatim:

> "One person's breakthrough should become everyone's baseline. The biggest failure mode wasn't that people couldn't figure things out. It was that everyone had to figure things out alone. A workflow discovered by one person didn't help anyone else. Glass needed to compound wins into organizational capability: shared skills, propagated best practices, and a floor that rises with every discovery."

The thread ends with: *"We don't believe in lowering the ceiling. We believe in raising the floor."* Ramp reports **over 350 skills shared company-wide**, Git-backed, versioned, and reviewed like code.

**Implication for Contributor Studio.** This is the precedent for the Skill Dojo (BC19 Skill Lifecycle) and the three-state share model (Private → Team → Mesh). Sprint 2's Skill Dojo needs explicit "share to team" and "propose to mesh" actions so that a single contributor's discovery promotes into a team default and eventually a mesh-reviewed baseline via the Judgment Broker. The 350-skill Ramp figure is the only external baseline we have for what compounding looks like in practice; PRD-003 should cite it as a directional target, not a contract.

### C6 — Memory-by-default is table stakes

**Source:** S4 (Goddijn, Ramp Glass) and S5 (Buchan engineering retrospective).

**Evidence.** Goddijn: *"When users first open Glass, we build a full memory system based on the connections they've authenticated. This gives every chat session context on the people they work with and their active projects, along with references to relevant Slack channels, Notion documents, Linear tickets."* He then describes a **synthesis and cleanup pipeline every 24 hours, mining users' previous sessions and connected tools.**

Buchan's engineering follow-up makes the architecture explicit:

> "Glass's memory system runs as a background pipeline. Every 24 hours, it mines the user's previous sessions and connected integrations (Slack, Notion, Calendar) and synthesizes an updated profile … We made memory write-once-read-many. The synthesis pipeline writes memory files. Every new session reads them at startup. The agent never modifies memory during a conversation, it just uses whatever context exists. You know exactly what the agent knows, because it's all in files you can inspect."

**Implication for Contributor Studio.** ADR-030 (Agent Memory Pods) and ADR-052 (Pod Default WAC + Public Container) already give VisionClaw the storage model for a user-legible, file-based memory system. What Ramp proves is that (a) the synthesis should run as a **background cron-style pipeline** rather than being invoked per-session, (b) memory writes should be **write-once-read-many with an auditable file view** — the user must be able to open the file and see exactly what the agent knows, and (c) memory must be **on by default**, not a feature users have to configure. Design spec 03 (`03-pod-context-memory-and-sharing.md`) inherits this directly.

### C7 — Workspace, not chat window

**Source:** S4 (Goddijn, Ramp Glass).

**Evidence.** Direct quote:

> "Most AI products give you a single conversation thread. Glass gives you a full workspace. The interface is built around split panes, allowing you to tile multiple chat sessions side by side, or open documents, data files, and code alongside your conversations. It works like a code editor: drag tabs to rearrange, split horizontally or vertically, and keep context visible while you work. This matters because real work isn't linear."

Glass additionally renders *"markdown, HTML, CSVs, images, and code with syntax highlighting inline as tabs"* and layout persists across sessions — *"when you come back tomorrow, your workspace is exactly how you left it — panes, tabs, and all."*

**Implication for Contributor Studio.** Design spec 01 (`01-contributor-studio-surface.md`) inherits this as its primary interaction model: split-pane workspace with the 3D graph, markdown/editor work lane, AI partner lane, ontology guidance rail, and skill dojo as tileable panes. Layout must persist per-contributor (via Solid Pod under `/private/contributor-profile/`). This also constrains ADR-057 against any "one-chat-at-a-time" framing; the Studio is a *desktop-class workspace* rendered in the browser.

### C8 — Unprompted / proactive AI is the missing move

**Source:** S3 (Sivulka, a16z, pillar 7).

**Evidence.** Pillar 7 of the seven pillars of institutional intelligence is *"Unprompted — individual AI responds to human prompts; institutional AI acts unprompted."* Sivulka's decisive formulation:

> "Prompting an AGI is like hooking an electric motor into a power loom. It's fundamentally, irrevocably constrained by the weakest link — us."

And:

> "AI should find the risk that nobody flagged, the counterparty nobody thought of, and the sales pipeline that nobody knew was there."

Ramp's Goddijn echoes this architecturally with Glass's **Sensei** — *"a built-in AI guide … It looks at which tools you've connected, what role you're in, and what you've been working on, and recommends the skills most likely to be useful to you."*

**Implication for Contributor Studio.** The Ontology Sensei pillar of the stratum (pod `/private/agent-memory/episodic/` synthesis plus proactive "3 relevant skills for current context" nudging via `ontology_discover` MCP) is the VisionClaw instantiation of Sivulka's pillar 7. Phase 4 of the rollout (Evals / Automations / Proactive Sensei) is explicitly about moving from reactive-to-prompt to unprompted-discovery. PRD-003 §12 can cite C8 as the external rationale; ADR-057 Context can use it as the decision driver for including a proactive pane rather than deferring it.

### C9 — Skills are software assets with a lifecycle (evals, benchmarks, regression loop)

**Source:** S6 (Anthropic Skill Creator v2).

**Evidence.** Skill Creator v2 adds four modes — Create, Eval, Improve, Benchmark — backed by four sub-agents (Executor, Grader, Comparator, Analyzer). The practical workflow, in Anthropic's own framing:

> "Eval results are tied to specific published versions, so you know exactly how v1.2.0 performs versus v1.1.0."

The Tessl coverage (Baptiste Fernandez, 4 March 2026) reframes skills explicitly as software: *"versioning, testing, distribution, and lifecycle management."* Debs O'Brien (Microsoft DevRel, Dev.to) reports a README Wizard skill going from **81 → 97.5** (+15.7pp) after one optimisation pass, and concludes: *"the problem with a skill is not the logic inside it — it's that the description is not specific enough."* Tessl-registry numbers include Cisco software-security 84% (1.78×), ElevenLabs TTS 93% (1.32×), Hugging Face tool-builder 81% (1.63×).

Skill Creator v2 also formalises a capability-vs-preference distinction: **capability skills** extend what the model can do and risk obsolescence; **preference skills** encode organisational style and rarely become redundant.

**Implication for Contributor Studio.** BC19 (Skill Lifecycle) — `SkillPackage`, `SkillVersion`, `SkillEvalSuite`, `SkillBenchmark`, `SkillDistribution` — is a direct instantiation of the Anthropic lifecycle. The description-optimisation loop (20-query eval set, 60/40 train/test split, held-out test score selection) is the concrete mechanism PRD-003 and design spec 02 should adopt for the Sprint 4 eval surface. The capability-vs-preference split is what justifies BC19 carrying *both* a benchmark service (capability skills) and a style-validation service (preference skills) rather than a single generic evaluator.

### C10 — Skill retirement discipline prevents mesh redundancy

**Source:** S6 (Anthropic Skill Creator v2).

**Evidence.** The retirement mechanism is explicit in v2. Quoting the YouTube walkthrough transcript (*"Anthropic Just Dropped Claude Code Skills 2.0"*):

> "The Skill Creator skill can help you determine whether you should get rid of that skill or not because the base model capability has caught up."

The operational test is that benchmarking compares the skill-enabled run against a **baseline run with the skill disabled on the same model**; if baseline matches or beats the skill, the skill is redundant and should be retired. This is not advisory — Skill Creator v2 flags it as a first-class recommendation.

**Implication for Contributor Studio.** Without explicit retirement, VisionClaw's skill mesh will accumulate redundant capability skills and degrade the Sensei's recommendation quality. BC19 must carry a `SkillBenchmark` aggregate that compares against a disabled-skill baseline on the current base model; the Judgment Broker (ADR-041) should route retire-recommendations into the same review workflow as promote-to-mesh recommendations. A "redundant skill retirement rate" KPI (listed in the strategic brief's BC15 KPI extensions) is what makes this observable. C10 is therefore the evidence for why BC19 is not just about publishing skills — it is about keeping the catalogue honest as the base model improves.

## Product precedents

Ramp Glass is the strongest single product precedent for Contributor Studio. The shape-for-shape mapping between what Ramp shipped and what the Contributor AI Support Stratum proposes is close enough to warrant a dedicated summary.

### What Ramp Glass shipped (S4 + S5)

- **Desktop workspace** (Electron-class), not a chat window. Split panes, tileable tabs, persistent layout across sessions, inline rendering of markdown / HTML / CSVs / images / code.
- **Auto-configured on install.** Okta SSO sign-in propagates to all integrations; *"everything connects on day one."* Buchan's proxy layer reduced startup from ~45s of MCP handshakes to ~2s by connecting to all services at app launch and keeping connections warm.
- **Dojo** — a skill marketplace backed by a Git repo. Markdown-file skills, non-engineers contribute via PRs they never see; Glass creates the branch, opens the PR, and handles the merge. Over 350 skills shared company-wide.
- **Sensei** — an in-product AI guide that recommends the 5 most relevant skills for a new user based on role, connected tools, and recent work.
- **Memory** — a 24-hour background synthesis pipeline; write-once-read-many memory files; user can inspect the files to see exactly what the agent knows.
- **Scheduled automations** — cron-driven jobs that post to Slack; Slack-native assistants; **headless mode** for long-running tasks (kick off, walk away, approve permission requests from phone).
- **Self-healing integrations** — token refresh, plain-language re-auth prompts, no cryptic errors for non-technical users.
- **"Every feature is secretly a lesson"** — skills teach what good output looks like, memory teaches that context matters, self-healing teaches that errors aren't the user's fault. Goddijn's single-biggest learning, verbatim:

> "The people who got the most value weren't the ones who attended our training sessions. They were the ones who installed a skill on day one and immediately got a result. The product taught them faster than we ever could."

### What the Ramp team says enables compounding

Goddijn's three-reason argument for building in-house rather than buying, quoted:

1. **"Internal productivity is a moat.** Using AI well is now a core business need. The companies that make every employee effective with AI will move faster, serve customers better, and compound advantages their competitors cannot match. That makes internal AI infrastructure part of your moat, and you do not hand your moat to a vendor."
2. **"Speed.** When you own the tool, you see exactly where people get stuck. You can ship fixes the same day someone reports a problem."
3. **"It directly informs our external product."** (Ramp-specific, but the pattern transfers to VisionClaw: the Stratum is also the reference implementation for VisionClaw's external enterprise story.)

Buchan's meta-lesson on the engineering mode is equally relevant:

> "Vibe coding is a skill. Like any skill, you can do it well or you can do it poorly, and the difference isn't talent, it's discipline … the engineering discipline doesn't go away just because the AI is writing the code. If anything, you need more of it."

Buchan names four disciplines that made the Glass codebase self-maintaining: a `Defrag` skill (scans for fragmentation and fixes it), a real design system with a shared component library and tokens, a document-validation PR gate, and pre-commit quality gates. All four are reproducible patterns that inform VisionClaw's own Studio repo governance.

### Where Ramp Glass is not a one-to-one precedent

Ramp is a single-tenant, Okta-authenticated, internal-only product. VisionClaw's Contributor Studio must work across (a) independent contributors identified by Nostr keys, (b) team pods with WAC groups, and (c) enterprise tenants under ADR-040 / ADR-048. The **Sovereign Workspace** pillar (NIP-07 auth transparently propagating to Solid Pod + MCP, deep-linking graph selection → agent context) is where VisionClaw diverges from Glass — we inherit Glass's UX shape but require a cross-tenant, pod-sovereign identity model underneath. The three-state share model (Private → Team → Mesh) is the VisionClaw-native construct that Glass does not need because Glass has no mesh.

## Applicability matrix

| Claim | Directly applicable to VisionClaw? | Why | Specific Studio / stratum feature it motivates |
|---|---|---|---|
| C1 — AI foundations | Yes (inverse direction) | VisionClaw already has foundations; the missing piece is the contributor "use" half | Sprint 1 Studio shell; PRD-003 framing as "convert foundations to practice" |
| C2 — Enduring capabilities + data productization | Yes | Graph/ontology/pod are candidate data products; Studio is the consumption surface | Ontology Sensei pillar; design spec 03 (pod context / memory / sharing) |
| C3 — Institutional vs individual AI | Yes — the primary motivating claim | VisionClaw currently ships individual AI across fragmented tools | ADR-057 Decision scope; the Contributor AI Support Stratum itself |
| C4 — Harness > model | Yes | VisionClaw agents exist; harness is missing | PRD-003 product statement; Sprint 1 MVP |
| C5 — One person's breakthrough becomes everyone's baseline | Yes | Directly translates to the three-state share model | Skill Dojo; share-to-mesh funnel; BC19 `SkillDistribution` |
| C6 — Memory by default | Yes | ADR-030 Agent Memory Pods already exists; Ramp proves the *UX model* | Background synthesis pipeline; pod `/private/agent-memory/` auto-enabled; design spec 03 |
| C7 — Workspace, not chat window | Yes | New route `/#/studio` is a split-pane workspace, not a chat | Design spec 01 (Contributor Studio surface); persistent layout in `/private/contributor-profile/` |
| C8 — Unprompted AI | Yes, but Phase 4 | Ontology Sensei proactive nudging is the VisionClaw analogue of Glass's Sensei | Phase 4 Proactive Sensei; `ontology_discover` MCP tool |
| C9 — Skills as software with evals | Yes | BC19 Skill Lifecycle is a direct instantiation | `SkillEvalSuite`, `SkillBenchmark`, description-optimisation loop; design spec 02 |
| C10 — Skill retirement | Yes | Without retirement, the mesh degrades | Judgment Broker retire-review workflow; BC15 "redundant skill retirement rate" KPI |

No claim in the corpus is non-applicable; every one of the ten has a direct VisionClaw feature. Claims C3 (institutional), C4 (harness), and C5 (baseline-raising) are the three that PRD-003's opening section should lead with — they are the tightest fit and the easiest to defend in review.

## Gaps in the evidence

The corpus is strong on the *shape* of what to build but thin on five dimensions that matter for VisionClaw and that PRD-003 / ADR-057 authors must flag as open questions rather than smuggle in as assumed facts.

1. **Decentralised / pod-sovereign identity.** Ramp Glass assumes Okta SSO and a single-tenant enterprise boundary. None of S1–S6 address Solid Pod WebIDs, NIP-07 Nostr auth, or cross-tenant mesh sharing under a WAC permission model. VisionClaw's Sovereign Workspace pillar is therefore not externally validated; it is a VisionClaw-native bet and should be marked as such.
2. **OWL / ontology-driven guidance.** The corpus does not describe any product that uses a formal ontology (OWL2 DL TBox, subClassOf reasoning) to drive proactive guidance. Ramp's Sensei and Glass's memory use unstructured context — Slack threads, Notion, Linear tickets. The Ontology Sensei pillar is therefore novel and needs internal validation (e.g. does `ontology_discover` actually improve skill-recommendation precision vs a vector-only baseline?).
3. **Graph-native workspace surfaces.** VisionClaw's 3D graph and deep-linking (graph selection → agent context) have no analogue in the corpus. Glass is a tiled text/code workspace; VisionClaw adds a spatial workspace pane. Whether that extra pane improves contributor outcomes or just distracts is an open question.
4. **Quantified contributor activation / time-to-first-result.** PwC reports financial outcomes (7.2× performance, +4pp margin) and Ramp reports adoption (99%) and skill count (350+), but neither gives a *contributor activation* metric (time from first sign-in to first useful output). The BC15 KPI extensions in the strategic brief (`contributor activation`, `time-to-first-result`, `skill reuse`, `share-to-mesh conversion`, `ontology guidance hit rate`, `redundant skill retirement rate`) are therefore defined against *no external benchmark*. PRD-003 should call these "target metrics to establish" rather than "targets to hit."
5. **Sustained institutional outcomes of skill marketplaces.** Ramp has shipped 350 skills in a short window; there is no longitudinal data on (a) how many stayed useful, (b) how many were retired, (c) how the marketplace quality distribution aged. C10 (retirement discipline) is supported by the Anthropic tooling but not by a measured multi-year outcome. ADR-057 should treat the Dojo + retirement loop as a hypothesis, not a settled solution, and build the evals in early (Sprint 2, not Sprint 4) so that VisionClaw itself produces the evidence we can't cite.

These gaps are also the agenda for the Phase 4 proactive / evals work: every gap is answerable by instrumenting the Studio from Sprint 1 onward.

## Citation guide for downstream authors

This annex is the single cite-ready source for the contributor-enablement work. When writing PRD-003, ADR-057, the DDD explanation, or any of the four design specs, cite this annex by section and claim identifier rather than re-quoting the corpus.

**Preferred citation forms:**

- PRD-003 §12 (Supporting Research): *"See `docs/design/2026-04-20-contributor-studio/evidence-annex.md` §C3, §C4, §C5 for the institutional-AI, harness-over-model, and baseline-raising claims that motivate this PRD."*
- ADR-057 Context: *"Industry evidence is collated in `docs/design/2026-04-20-contributor-studio/evidence-annex.md`; see in particular C1 (foundations gap), C3 (institutional AI), C6 (memory by default), and C9 (skill lifecycle)."*
- DDD explanation (`docs/explanation/ddd-contributor-enablement-context.md`): *"BC19's aggregates derive from the skill lifecycle formalised by Anthropic Skill Creator v2; see Evidence Annex C9 and C10."*
- Design specs 01–04: cite specific claims inline — e.g. spec 01 references C7 (workspace, not chat window) for the split-pane requirement, spec 02 references C9 and C10 for the eval / retirement loop, spec 03 references C6 for the memory model, spec 04 can reference C5 and C8 for the promotion / proactive acceptance tests.

**Source corpus citation form.** When a design document needs to quote the underlying source directly (rather than the annex's extracted claim), cite the corpus file path:

- `presentation/2026-04-20-best-companies-ai/01-pwc-2026-ai-performance-study.md`
- `presentation/2026-04-20-best-companies-ai/02-mckinsey-ai-transformation-manifesto.md`
- `presentation/2026-04-20-best-companies-ai/03-a16z-institutional-vs-individual-ai.md`
- `presentation/2026-04-20-best-companies-ai/04-ramp-glass-seb-goddijn.md`
- `presentation/2026-04-20-best-companies-ai/04b-ramp-glass-shane-buchan-how-we-built-it.md`
- `presentation/2026-04-20-best-companies-ai/90-anthropic-skill-creator-v2.md`

Include the fidelity caveat from the source map (e.g. Ramp markdown is transcript-reconstructed, PwC numbers come via Consultancy.me mirror) when the numeric claim is load-bearing.

**Extending the annex.** If a new claim or source arrives (e.g. later a16z essays, a McKinsey follow-up, a Ramp engineering deep-dive on distribution/auth), extend this file in place with additional C-identifiers (C11, C12, ...) and update the source map. Do not branch the evidence into parallel files — the annex is the single canonical reference for downstream citations, and its value depends on staying that way.
