# Project Rules — Claude Code

@AGENTS.md

## Claude Code only

- Memory tools are `mcp__claude-flow__memory_*`. The codebase-memory MCP (project `home-devuser-workspace-project`) is available for structural queries — callers/callees, architecture, diff impact on large changes.
- Web search: `/ceramic-search` first, `/perplexity-research` for authoritative/academic sources, built-in WebSearch as fallback.
- KG grounding, SPARQL, governed writeback: `/ontology-augment`. Personal email: `/email-search`.
- Unsure which skill: `/route [task]`.
