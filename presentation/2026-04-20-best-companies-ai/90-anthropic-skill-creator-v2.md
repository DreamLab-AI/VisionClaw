# Anthropic Skill Creator 2.0 — Evals for User-Defined Skills

*Adjacent research, not from the 2026-04-20 AI Daily Brief episode.*

**Released:** March–April 2026 (v2 update)
**Official repo:** <https://github.com/anthropics/skills/tree/main/skills/skill-creator>
**SKILL.md source:** <https://github.com/anthropics/skills/blob/main/skills/skill-creator/SKILL.md>
**Claude plugin page:** <https://claude.com/plugins/skill-creator>
**Official announcement:** No stand-alone Anthropic blog post — the upgrade landed in the `anthropics/skills` GitHub repo and rolled out via the Claude Code plugin marketplace.

## What changed

Anthropic's `skill-creator` used to write SKILL.md files. **v2 adds evals, benchmarks, A/B comparators, and a regression loop** — so user-defined skills can be tested, scored, and kept current as Claude's base model evolves.

The skill now operates in four modes:

1. **Create** — draft a new skill (unchanged from v1).
2. **Eval** — create test prompts + assertions, run the skill vs a baseline, grade results.
3. **Improve** — analyse failures, propose targeted edits, re-test.
4. **Benchmark** — compare runs quantitatively (pass rate, timing, tokens) across versions.

## The four composable sub-agents

Skill Creator v2 is itself an orchestrator over four specialised sub-agents:

| Agent | Role |
|---|---|
| **Executor** | Runs the skill against each eval prompt; captures outputs and timing. |
| **Grader** | Evaluates outputs against the assertions in `eval_metadata.json`; produces `grading.json` with `passed`/`evidence`. |
| **Comparator** | Performs blind A/B comparison between two skill versions — evaluator doesn't see which output came from which version. |
| **Analyzer** | Reads aggregate benchmark data and surfaces *why* one version beat another, not just that it did. |

## Keeping skills up to date — the actual mechanism

Skill Creator v2 does **not** auto-detect SDK drift or model releases. It keeps skills current by giving them tests you can re-run whenever the base model ships an update:

> "Eval results are tied to specific published versions, so you know exactly how v1.2.0 performs versus v1.1.0."

The practical workflow:

1. Re-run your eval set after Claude ships a new base model.
2. Benchmark compares the skill-enabled run vs a **baseline run with the skill disabled** on the same model.
3. If the baseline now matches or beats the skill, the skill is **redundant** — the model has caught up — and Skill Creator explicitly recommends retiring it:
   > *"The Skill Creator skill can help you determine whether you should get rid of that skill or not because the base model capability has caught up."*
   > — YouTube walkthrough, *"Anthropic Just Dropped Claude Code Skills 2.0"* (<https://www.youtube.com/watch?v=qXWz-V_XMOc>)
4. If the skill still outperforms baseline, the "Improve" mode proposes edits driven by failure transcripts.

## Capability skills vs preference skills

The v2 docs and coverage formalise a distinction that wasn't explicit before:

- **Capability skills** extend what the model can do — e.g. PDF extraction, form completion, Zendesk investigation. These are the skills that risk obsolescence as base models improve.
- **Preference skills** encode organisational style — formatting, tone, doc structure. These rarely become redundant.

Skill Creator v2's triage pushes you to evaluate capability skills aggressively, because they're the ones where the model-catches-up failure mode applies.

## The anatomy of a skill (unchanged fundamentals)

```
skill-name/
├── SKILL.md (required)
│   ├── YAML frontmatter (name + description required)
│   └── Markdown instructions
└── Bundled Resources (optional)
    ├── scripts/      — executable code
    ├── references/   — docs loaded into context as needed
    └── assets/       — files used in output
```

Progressive disclosure is preserved:
1. **Metadata** (name + description, ~100 words) always in context.
2. **SKILL.md body** (<500 lines ideal) loads when the skill triggers.
3. **Bundled resources** load on demand.

## The description-optimisation loop (new)

Skill Creator v2 also adds a **triggering-accuracy** optimiser. A skill's description is what Claude sees to decide whether to consult it, so v2 generates a 20-query eval set — roughly half should-trigger, half should-not-trigger — and iterates the description:

```bash
python -m scripts.run_loop \
  --eval-set <path-to-trigger-eval.json> \
  --skill-path <path-to-skill> \
  --model <model-id> \
  --max-iterations 5 \
  --verbose
```

The loop splits the eval set 60/40 train/test, proposes description rewrites based on failures, and selects `best_description` by the held-out test score (to avoid overfitting).

## Community reactions and case studies

- **Debs O'Brien (Microsoft DevRel), Dev.to — "I used Skill Creator v2 to improve one of my agent skills in VS Code"** (<https://dev.to/debs_obrien/i-used-skill-creator-v2-to-improve-one-of-my-agent-skills-in-vs-code-fhd>): Her `README Wizard` skill went from **81 → 97.5** (15.7 pp improvement) after a single optimisation pass. Her headline takeaway: *"the problem with a skill is not the logic inside it — it's that the description is not specific enough."*
- **Baptiste Fernandez, Tessl — "Anthropic brings evals to skill-creator. Here's why that's a big deal"** (<https://tessl.io/blog/anthropic-brings-evals-to-skill-creator-heres-why-thats-a-big-deal/>, 4 March 2026): Reframes skills as software that needs *"versioning, testing, distribution, and lifecycle management."* Challenges the ETH Zurich finding that context files improved agent performance by only ~4% — argues the real problem is **"unvalidated context is useless"** and evals fix it. Cites Tessl-registry benchmarks: Cisco software-security skill at 84% (1.78× improvement), ElevenLabs TTS at 93% (1.32×), Hugging Face tool-builder at 81% (1.63×).
- **Joe Njenga, Medium — "Anthropic (New) Skill-Creator Measures If Your Agent Skills Work (No More Guesswork)"** (<https://medium.com/ai-software-engineer/anthropic-new-skill-creator-measures-if-your-agent-skills-work-no-more-guesswork-840a108e505f>).
- **Mohit Aggarwal, Medium — "I Tested Anthropic's Skill-Creator Plugin on My Own Skills — Here's What I Found"** (<https://medium.com/all-about-claude/i-tested-anthropics-skill-creator-plugin-on-my-own-skills-here-s-what-i-found-23ad406b0825>).
- **Tool Nerd — "Anthropic Skill Creator 2.0 Update"** (<https://www.thetoolnerd.com/p/anthropic-skill-creator-20-update>): Their demo ended with *"Claude wrote the skill, designed the test cases, ran six parallel agents, graded the outputs, opened the review UI, and packaged the result. The only human input after the initial prompt was a single word: 'approved.'"*
- **Tool Mesh News** — *"Anthropic Skill-Creator Upgrade: New Evaluation System"* (<https://www.toolmesh.ai/news/anthropic-skill-creator-major-upgrade-evaluation>).
- **AI Disruption substack** — *"Anthropic's skill-creator update: No more 'band-aid' skills"* (<https://open.substack.com/pub/aidisruption/p/anthropics-skill-creator-update-no>).
- **Community skill marketplace** — <https://skillsmp.com> (Agent Skills Marketplace for Claude / Codex / ChatGPT).

## Environment-specific caveats

The v2 SKILL.md itself contains explicit branches for different hosts:

- **Claude.ai** — no subagents available; run test cases yourself, skip baseline runs and quantitative benchmarking, present qualitative feedback inline. Description optimisation requires the `claude` CLI (Claude Code only).
- **Claude Code** — full multi-agent workflow with browser-based eval viewer.
- **Cowork** — subagents available but no display; use `--static <output_path>` to render the eval viewer as a standalone HTML file, and generate it **before** inspecting outputs yourself so the human can see results ASAP.

## Updating an existing skill

v2 adds specific guidance for in-place updates:
- Preserve the original `name` field in frontmatter.
- Copy the skill to a writeable location before editing (e.g. `/tmp/skill-name/`).
- Package from the copy — `package_skill.py` requires Python + filesystem.
