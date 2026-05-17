# Social Posts & Related Links

## Eric Glyman — Ramp co-founder/CEO

**URL:** <https://x.com/eglyman/status/2043362828178841860>
**Date:** April 2026

> 99% of Ramp uses AI daily. But we noticed most people were stuck — not because the models weren't good enough, but because the setup was too painful and unintuitive for most. Terminal configs, MCP servers, everyone figuring it out alone.
>
> So we built **Glass**. Every employee gets a fully configured AI workspace on day one. Integrations connected via SSO, a marketplace of 350+ reusable skills built by colleagues, persistent memory, scheduled automations. When one person on a team figures out a better workflow, everyone on that team gets it and gets more productive.
>
> The companies that make every employee effective with AI will compound advantages their competitors can't match. Most are waiting for vendors to solve this. We decided to own it.

**Related Ramp posts (older):**
- Ramp blog, 2024-07-31 — *Eric Glyman on using AI to radically boost internal productivity* — <https://ramp.com/blog/ai-for-internal-productivity>
- Ramp case study (Notion) — <https://www.notion.com/customers/ramp>

---

## Ryan Carson — "Code Factory" thread

**Author:** Ryan Carson — [@ryancarson](https://x.com/ryancarson) (founder, 20+ years building companies; runs <https://www.ryancarson.com/>)
**Canonical Code Factory post:** <https://x.com/ryancarson/status/2023767406511116584>
**Related posts:**
- Scheme for a Code Factory migration — <https://x.com/ryancarson/status/2023468856220807539>
- "Get your Code Factory set up because the next step is the Company Factory" — <https://x.com/ryancarson/status/2024436111964004461>
- Community implementation: <https://github.com/lemmylabh/Ryan-Carson> — "A simple task management system for managing AI dev agents"

**Context in the episode:** Carson wrote the following after Claude Code launched scheduled tasks ~one month before the episode (March 2026):

> It's exciting seeing all the big model labs launch automations. It's exciting to see everyone moving towards code factories. I think we'll probably have complete solutions from all the big players by the end of the year that allow you to create an end-to-end code factory for your startup without having to jump around to different tools or duct-taping things together.

**Supporting Anthropic docs:** <https://code.claude.com/docs/en/desktop-scheduled-tasks>

---

## Guillermo Rauch — Vercel Open Agents announcement

**Author:** Guillermo Rauch — [@rauchg](https://x.com/rauchg), CEO, Vercel
**Canonical announcement tweet:** <https://x.com/rauchg/status/2043869656931529034>
**Project:** Open Agents — <https://open-agents.dev>
**Date:** April 2026

> Today we're open sourcing **open-agents.dev**, a reference platform for cloud coding agents. You've heard that companies like Stripe (Minions), Ramp (Inspect), Spotify (Honk), Block (Goose), and others are building their own "AI software factories". Why?
>
> 1️⃣ On a technical level, off-the-shelf coding agents don't perform well with huge monorepos, don't have your institutional knowledge, integrations, and custom workflows.
> 2️⃣ Competitive advantage is moving from code itself to production systems: *"The alpha is in your factory"* — the means by which software is created.

### Open Agents — architecture

Three-layer system:

1. **Web interface** — user interaction layer.
2. **Long-running agent workflow** — reasoning and orchestration, running outside the sandbox.
3. **Sandboxed execution environment** — isolated code execution managing files, shell commands, and git operations.

The separation allows independent management of execution, state, and infrastructure. The project is meant to be **forked and adapted** to a team's codebase, not used off-the-shelf.

### Coverage & commentary

- Tessl write-up: <https://tessl.io/blog/vercel-open-sources-open-agents-to-help-companies-build-their-own-ai-coding-agents/>
- TechCrunch — Vercel IPO readiness / agents fuel revenue: <https://techcrunch.com/2026/04/13/vercel-ceo-guillermo-rauch-signals-ipo-readiness-as-ai-agents-fuel-revenue-surge/>
- Signal/IndiaNIC — Vercel "Agent Skills" announcement (Jan 2026, earlier standardisation move): <https://signal.indianic.com/vercel-debuts-agent-skills-a-strategic-move-to-standardize-the-ai-coding-ecosystem/>
- Rauch podcast interview — Every's "What Comes After Coding": <https://every.to/podcast/vercel-s-guillermo-rauch-on-what-comes-after-coding>

### Trade-off noted in coverage

Open Agents gives companies greater control over costs, rate limits, and feature implementation, but requires them to maintain and integrate their own systems. Anthropic's *Claude Managed Agents* takes the opposite approach — infrastructure-as-a-service.

### In-house AI software factories referenced

| Company | Internal agent platform |
|---|---|
| Stripe | Minions |
| Ramp | Inspect (and Glass — see `04-ramp-glass-seb-goddijn.md`) |
| Spotify | Honk |
| Block | Goose |

---

## Other referenced names and sources

- **Ethan Mollick** — mentioned in passing as a commentator who argues against outsourcing AI transformation to consultants. Canonical: <https://oneusefulthing.org/>.
- **George Sivulka** — author of the a16z essay (see `03-a16z-institutional-vs-individual-ai.md`); also founder/CEO of Hebbia.
- **McKinsey Rewired** — the book from which the manifesto is excerpted, 2nd edition — Wiley, 2026.
