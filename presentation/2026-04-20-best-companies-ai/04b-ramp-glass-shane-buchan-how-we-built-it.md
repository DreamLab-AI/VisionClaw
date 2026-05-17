# Ramp's Glass — How We Actually Built It

**Author:** Shane Buchan (engineer, Ramp)
**Platform:** LinkedIn article, announced via X — <https://x.com/buchan_sm/status/2044526740299526511>
**Context:** Follow-up to Seb Goddijn's April 2026 thread (see `04-ramp-glass-seb-goddijn.md`).

---

Last week Seb shared how we built every employee at Ramp their own AI coworker. The response was overwhelming: nearly a million views, hundreds of messages, and one question that kept coming up. How did you actually build this?

The honest answer is stranger than most people expect. Glass was predominantly vibe coded by a three-person core team (a product manager, an engineer, and an IT engineer) and the story of how we went from a messy prototype to a product used by half the company is, I think, the more interesting thing to talk about. The real lesson isn't that vibe coding works. It's that vibe coding is a skill, and like any skill, there's a version that produces great results and a version that produces a mess. We learned the difference the hard way.

## Glass Had Users Before It Had a Team

Jay Sobel kicked things off as a weekend project. He wanted to see if wrapping a coding agent in a desktop app could make it useful for people who'd never open a terminal, while still giving engineers something better than a raw CLI.

A month later Seb picked it up because he saw the potential for non-engineers. He's a PM with an engineering background, and he had a theory: the reason AI adoption had stalled at Ramp wasn't the models, it was the environment. People needed a preconfigured workspace, not a tutorial. So he started vibe coding and built out core integrations to things like Slack and Notion, connected Dojo, and polished the core chat UX. Within a few weeks Glass had about 20 daily users asking for more.

The demand came first. People were already using it, and we had to keep up. **Glass had users before it had a team.**

## So We Vibed Harder. But With Structure.

As Glass gained traction, Seb pulled me in to help build faster. And this is where it got interesting.

The codebase was growing quickly, but it was growing outward. Every new feature in its own file, its own patterns, its own styling approach. Components doing almost the same thing in slightly different ways. Utility functions reimplemented because the agent didn't know they already existed somewhere else. No shared design system, so each feature looked a little different from the last. Documentation hadn't kept up, so the agent couldn't even reference what the product already did.

The product still worked. But things were slowing down. Bugs showed up at the seams between features. PRs got harder to review because you couldn't tell whether code was intentionally different or just inconsistently generated. The codebase was getting bigger faster than it was getting more coherent.

None of this is unique to vibe coding, by the way. It's the same entropy that hits any fast-moving team. It just happens faster when the AI can write code faster than anyone can keep track of the whole.

We could have stopped and rewritten everything. But we had users who needed the product to keep getting better, and there were only three of us on the core team: Seb, me, and Cam. So instead of fighting the vibe coding, we figured out how to do it better.

### Teaching the codebase to maintain itself

- **Defrag.** Calvin Kipperman built a skill that scans the codebase for fragmentation: duplicated components, inconsistent patterns, files that should be consolidated, logic that should be shared. It doesn't just flag problems, it fixes them. We run it regularly, and every pass makes the codebase more coherent for the next thing the agent builds.
- **A real design system.** I built a shared component library and token system so that every new UI element inherits the same visual language. Instead of generating CSS from scratch each time, the agent references what already exists. Huge unlock. When the agent knows what components exist, it reuses them instead of reinventing them.
- **Document validation.** We added a validation step to the PR pipeline. If you add a feature, the docs need to describe it. If you modify a skill, the skill's documentation needs to be current. Annoying to set up, but it made the biggest difference. The agent builds on top of existing capabilities instead of accidentally duplicating or contradicting them.
- **Pre-commit quality gates.** Defrag checks, linting, type checking, and doc validation run automatically before anything merges. Fragmentation gets caught at the door instead of accumulating.

None of this is fancy. It's just normal software engineering, pointed at a vibe-coded codebase. But that's kind of the whole point: **the engineering discipline doesn't go away just because the AI is writing the code. If anything, you need more of it.**

## Going One Level Deeper: How the Core Systems Work

### Connections That Don't Make You Wait

Glass connects to Slack, Salesforce, Notion, Linear, Gong, Datadog, and a dozen other tools through MCP servers. The straightforward approach is connecting to each one when you open a chat. With 13+ integrations, that's about 45 seconds of handshake latency before you can send your first message.

I built a proxy layer that connects to all external services once at app launch and keeps those connections alive. Each chat session gets a fresh lightweight wrapper around the shared connection. One persistent pipe per service, unlimited sessions on top. Startup went from 45 seconds to about 2.

### Dojo: A Git Repo That Non-Engineers Can Contribute To

Dojo, our skill marketplace, is backed by a Git repo. Skills are markdown files. No code, no deployment pipeline, no infrastructure. When someone creates a skill in Glass, it writes a markdown file. When they publish it, Glass handles the Git commit, the pull request, and the review workflow behind the scenes. The user never sees GitHub.

We needed non-technical people to contribute their expertise without learning version control. A CX lead who builds a Zendesk investigation workflow shouldn't need to know what a pull request is. But we also needed skills to be versioned, reviewable, and auditable, which meant Git was the right backend.

The trick is making Git invisible. Glass creates the branch, writes the file, opens the PR, and handles the merge once it's approved. The contributor sees "publish" and "published." The engineering team sees a clean Git history with proper review trails. Over 350 skills have been shared this way.

### Memory: A Cron Job That Builds Context

Glass's memory system runs as a background pipeline. Every 24 hours, it mines the user's previous sessions and connected integrations (Slack, Notion, Calendar) and synthesizes an updated profile: who they work with, what they're working on, which resources are relevant. Stale entries get cleaned up automatically.

We made memory write-once-read-many. The synthesis pipeline writes memory files. Every new session reads them at startup. The agent never modifies memory during a conversation, it just uses whatever context exists. You know exactly what the agent knows, because it's all in files you can inspect.

### Integrations: Pre-Packaged and Self-Healing

Every Glass install comes with our internal CLIs pre-bundled (Ramp CLI, data tools, Google Workspace tooling) so the agent can use them without the user installing anything. MCP connections are configured centrally and authenticate through Okta SSO, so connecting to Slack or Salesforce is a single click, not a config file.

When integrations break (tokens expire, services go down) Glass detects the failure and either auto-refreshes or prompts the user to re-authenticate in plain language. A non-technical user should never see a cryptic error. The system either fixes itself or tells you exactly what to do.

### Distribution and Auth: Harder Than It Sounds

Getting Glass onto everyone's machine and keeping them logged in turned out to be one of the hardest problems. Okta SSO, certificate management, auto-updates across a managed fleet, MCP token refresh flows that don't break mid-session. **Cameron Leavenworth** owned this, and most users have no idea how much invisible work keeps their "single click to connect" actually working. Cam is writing the deep dive on that one next.

## The Development Loop Today

Here's what building Glass looks like now:

1. Someone identifies a need. User feedback, a Slack message, something they noticed using the product themselves.
2. They describe the feature in Glass. Glass builds it, referencing the design system, reusing existing components, following established patterns because the documentation tells it what's already there.
3. The PR goes through automated validation: defrag checks, type checking, doc completeness.
4. A human reviews the diff. It ships.

We go from idea to production in hours. Not because we're skipping steps, but because Glass automates the ones that used to be manual. The agent writes the code. The pipeline checks the quality. The human makes the call: is this the right thing to build, and does it actually work?

Jay, who started the repo as that original weekend project, joined us later and said he was *"shook by how much we shipped and the relatively fewer bugs."* Honestly, it's not because we're that good. The system just catches its own mistakes before they pile up.

## What If Every Feature Was Just a Conversation?

We're pushing Glass further.

The pattern we keep seeing is that every form, every settings panel, every modal in the app can become a conversation. Why click through a connection setup when you can just say "connect my Notion"? Why navigate to settings when you can say "turn on dark mode"? Why fill out a bug report when Glass can notice you're stuck, offer to file it for you, and spin up an agent to start investigating?

We're building toward something where the UI is a reference, not a requirement. Everything you can do through a menu, you can do through a message. And Glass keeps getting better at it because it can read its own blueprint, it has tools to modify itself, and it knows your role, your tools, and the organization around you.

## So... Is This Vibe Coding?

Yeah, kind of. And I get why that makes people uncomfortable.

There's a narrative in tech right now that vibe coding is either the future of software or a toy that produces garbage. Both are wrong. **Vibe coding is a skill. Like any skill, you can do it well or you can do it poorly, and the difference isn't talent, it's discipline.**

The model still writes the code. But now the codebase teaches the model how to write it well, and catches it when it doesn't.

I don't know if that's "the future of software" or whatever. But the teams that figure out the meta-game — not "how do I get the model to write good code" but "how do I keep the codebase healthy as the model writes a lot of code very fast" — are going to build things that weren't possible before. We stumbled into it, and I think more teams will too.
