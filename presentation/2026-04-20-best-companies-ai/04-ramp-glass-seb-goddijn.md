# We Built Every Employee at Ramp Their Own AI Coworker

**Author:** Seb Goddijn — [@sebgoddijn](https://x.com/sebgoddijn) (runs internal AI at Ramp)
**Canonical URL:** <https://x.com/sebgoddijn/status/2042285915435937816>
**Date:** April 2026 (≈1M views within a week)

---

## The Models are Good Enough, The Harness Isn't

At Ramp, we hit 99% adoption of AI tools across the company. And then we noticed something concerning: most people were stuck.

It wasn't that the models weren't good enough or that people lacked ambition — they just had no idea how to improve their set up. Terminal windows, npm installs, and MCP configurations were too much for most people to grok, and the few who pushed through had wildly different setups, with no way to share what they'd learned. We'd created urgency without providing enough infrastructure, and it limited the true upside of AI to people who already knew how to configure it.

So we decided to build our own AI productivity suite to make every employee an AI power-user without the pain of having to configure their environment. We've called it **Glass**.

## Everyone Can Be An AI Power User

The models are already exceptional, but most people use them like driving a Ferrari with the handbrake on. Not because they aren't smart, or lack ambition — they've just never seen what a well-configured environment looks like or what it can do.

To solve this problem we aligned around three core principles for Glass:

1. **Don't limit anyone's upside.** The default approach for non-technical users is to simplify: put the product on rails, offer fewer options, and make it dummy-proof. We couldn't disagree more. At Ramp, power users thrive on multi-window workflows, deep integrations, scheduled automations, persistent memory, and reusable skills. The goal isn't to remove complexity, but to make it invisible while preserving full capability.
2. **One person's breakthrough should become everyone's baseline.** The biggest failure mode wasn't that people couldn't figure things out. It was that everyone had to figure things out alone. A workflow discovered by one person didn't help anyone else. Glass needed to compound wins into organizational capability: shared skills, propagated best practices, and a floor that rises with every discovery.
3. **The product is the enablement.** Becoming an effective AI user is a skill. People improve through repetition and experimentation, but the product can accelerate that curve by suggesting the right skill at the right time, and showing what "good" looks like in the moment. No amount of workshops can match a targeted nudge while you're already doing the work.

## Everything connects on day one

Glass comes auto-configured on install. People sign in once via their Okta SSO, and all Ramp's tools become available to them with a one-click setup. This also includes home-grown products like **Ramp Research**, **Ramp Inspect**, and our newly released **Ramp CLI**.

This is the unsexy foundation that makes everything else possible. When a sales rep asks Glass to pull context from a Gong call, enrich it with Salesforce data, and draft a follow-up — it just works, because everything is already connected.

## We Distribute Reusable Skills Through Our Dojo

The easiest way to share learnings across the organization is through skills. These are markdown files that teach your agent exactly how to perform a specific task, and we've built out a marketplace for them called **Dojo**.

Now, when someone on the sales team figures out the best way to analyze Gong calls, break down competitive mentions, and draft battlecards, they can package it as a skill, and give that superpower to every rep on the team. A CX engineer builds a Zendesk investigation workflow that pulls ticket history, checks account health, and suggests resolution paths, and through Dojo the entire support team levels up overnight.

Over **350 skills** have been shared company-wide. They're Git-backed, versioned, and reviewed like code. The marketplace is the flywheel: every skill shared raises the floor for everyone.

To help people find the right skills, Dojo includes a built-in AI guide we call the **Sensei**. It looks at which tools you've connected, what role you're in, and what you've been working on, and recommends the skills most likely to be useful to you. A new account manager doesn't need to browse a catalog of 350 skills — the Sensei surfaces the five that matter most on day one. It's another example of the product doing the enablement work: rather than expecting people to know what's available, Glass meets them where they are.

## It Remembers Who You Are, And What You're Working On

When users first open Glass, we build a full memory system based on the connections they've authenticated. This gives every chat session context on the people they work with and their active projects, along with references to relevant Slack channels, Notion documents, Linear tickets, and more. As a result, the agent spends less time searching, entering each conversation with the context the user expects.

Under the hood, we also run a synthesis and cleanup pipeline every 24 hours, mining users' previous sessions and connected tools like Slack, Notion, and Calendar for updates. This means Glass can adapt to their world without them having to re-explain things every session.

## It Works While You Don't

Glass turns your laptop into a server. You can schedule automations that run daily, weekly, or on custom cron, and post results directly to Slack. A finance team lead pulls yesterday's spend anomalies every morning at 8 am and posts a summary to the team channel with a simple prompt that takes a few minutes to set up.

You can also create Slack-native assistants that listen and respond in channels using your full Glass setup, including your integrations, memory, and skills. An ops team built one that answers vendor policy questions by pulling from Notion docs and Snowflake data in an afternoon.

For long-running tasks, Glass has a **headless mode**: kick off a task, walk away, and approve permission requests from your phone. The results are waiting when you get back.

## It's a Workspace, Not a Chat Window

Most AI products give you a single conversation thread. Glass gives you a full workspace. The interface is built around split panes, allowing you to tile multiple chat sessions side by side, or open documents, data files, and code alongside your conversations. It works like a code editor: drag tabs to rearrange, split horizontally or vertically, and keep context visible while you work.

This matters because real work isn't linear. You might be drafting a Slack message in one pane, reviewing a Snowflake query result in another, and reading a PDF in a third. Glass renders markdown, HTML, CSVs, images, and code with syntax highlighting inline as tabs. When Claude creates or edits a file, it opens automatically so you can see the result without switching windows.

The layout persists across sessions. When you come back tomorrow, your workspace is exactly how you left it — panes, tabs, and all.

## Owning This Infrastructure Is A Competitive Advantage

The obvious question is why not just buy this. There are three reasons we built it in house.

1. **Internal productivity is a moat.** Using AI well is now a core business need. The companies that make every employee effective with AI will move faster, serve customers better, and compound advantages their competitors cannot match. That makes internal AI infrastructure part of your moat, and you do not hand your moat to a vendor.
2. **Speed.** When you own the tool, you see exactly where people get stuck. You can ship fixes the same day someone reports a problem. We have a Slack channel where users report issues, and our team triages them into tickets automatically, with most resolved in hours. You cannot do that while waiting on a vendor's roadmap.
3. **It directly informs our external product.** Ramp is an AI-first company building products for finance teams, and many of the problems we solve for internal users translate directly to customers. How do you build memory that actually helps? How do you enable people to build, distribute, and maintain effective skills? How do you surface functionality through usage? Solving these problems internally gives us conviction about what works before we ship it. Glass gives us reps on the hardest AI product problems without those reps happening at customers' expense.

In short, owning the stack helps us learn faster, build better AI-native products, and deliver better outcomes for customers.

## What We Learned

The single most important thing we learned building Glass: the people who got the most value weren't the ones who attended our training sessions. They were the ones who installed a skill on day one and immediately got a result. The product taught them faster than we ever could.

That realization reframed how we think about the entire project. Every feature in Glass is secretly a lesson. Skills show you what great AI output looks like before you know how to ask for it yourself. Memory shows you that context is the difference between a generic answer and a useful one. Self-healing integrations show you that errors aren't your fault — the system has your back. None of this was designed as education. But it turns out that when you hand someone a tool that just works, they learn by doing. And they learn fast.

This is what excites me most about what we're building. Not the product itself, but what happens to an organization when the floor rises for everyone at once. When a CX team lead shares a skill and sixty reps level up overnight. When a new hire's first session in Glass already knows their team, their projects, and their tools. When someone who's never opened a terminal is running scheduled automations that would have required an engineer six months ago. The compounding is real, and we're only at the beginning of it.

> **We don't believe in lowering the ceiling. We believe in raising the floor.**

---

*See also: [04b-ramp-glass-shane-buchan-how-we-built-it.md](04b-ramp-glass-shane-buchan-how-we-built-it.md) — Shane Buchan's follow-up on the vibe-coded engineering that actually produced Glass.*
