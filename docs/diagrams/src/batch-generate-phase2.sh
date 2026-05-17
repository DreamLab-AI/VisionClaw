#!/bin/bash
# Batch-generate VisionClaw Phase 2 diagrams (06-11) via Nano Banana Pro
set -e
export PATH="$HOME/.bun/bin:$PATH"
cd /home/devuser/.claude/skills/art/tools

RENDERED=/home/devuser/workspace/project/docs/diagrams/rendered
UPGRADED=/home/devuser/workspace/project/docs/diagrams/upgraded

AESTHETIC='Professional dark-mode technical diagram in VisionClaw brand aesthetic. Deep midnight-navy background (#0A1020) with subtle volumetric atmospheric haze. Crystalline nodes with soft inner luminescence and thin bright borders. Violet #8B5CF6 glow for governance/human-judgment elements, cyan #00D4FF for orchestration/agents/reasoning, emerald #10B981 for discovery/knowledge/ingestion, amber #F59E0B accents for trust hubs and central foci, red #EF4444 for orphaned/risk elements. Clean sans-serif typography (Inter-style) in off-white #E8F4FC. Thin directional energy filaments for connections with gradient glow. Cinematic sci-fi UI concept art fused with engineering blueprint precision. NO hand-drawn wobble, NO cartoon, NO watercolour, NO decorative clutter. Crisp geometric clarity. Preserve the structural layout of the reference diagram exactly including all text labels verbatim. Add depth via subtle drop shadows and rim lighting. Ensure every label from the reference is legible in the final image. 16:9 composition.'

gen() {
  local name=$1
  local intent=$2
  local aspect=${3:-16:9}
  echo "=== Generating $name ==="
  bun run generate-image.ts \
    --model nano-banana-pro \
    --prompt "${AESTHETIC} INTENT: ${intent}" \
    --reference-image "${RENDERED}/${name}.png" \
    --size 2K \
    --aspect-ratio "$aspect" \
    --output "${UPGRADED}/${name}.png" 2>&1 | tail -3
}

gen "06-migration-event" \
  'The Migration Event — a seven-stage horizontal journey from a Logseq note to a governed OWL class. Six labelled stages left-to-right with glowing arrows: 1 AUTHORING (emerald, a Logseq note with public::true flag, wikilink to Smart Contract, owl:class declaration), 2 DETECTION (cyan, 8-signal sigmoid scorer), 3 REVIEW (violet, Judgment Broker inbox with DecisionCanvas diff), 4 APPROVAL (amber, GitHub PR with Whelk consistency check), 5 MERGE (violet, live OWL Class at vc:bc/smart-contract with BRIDGE_TO promoted), 6 PHYSICS (emerald, graph re-settles, bridge filament, Nostr provenance bead). A dashed return arrow from stage 6 back to stage 1 labelled "new notes observe the canon". This is the hero diagram showing the full lifecycle as a single timeline.'

gen "07-dual-tier-identity" \
  'Dual-Tier Identity — two horizontal tiers stacked vertically. TOP TIER labelled "KG TIER (public:: true Logseq notes)" in emerald glow containing 4 nodes: Smart Contract (vc:bc/smart-contract, 14 wikilinks), Policy (vc:mv/policy, maturity:mature), Orphan Note (vc:ai/orphan, no ontology anchor — shown in red/alert colour), Agent Pattern (vc:ai/agent-pattern, authority:0.82). BOTTOM TIER labelled "ONTOLOGY TIER (OWL Classes)" in violet glow containing 4 nodes: SmartContract (bc:SmartContract, promoted), Policy (bc:Policy, colocated), AgentPattern (ai:AgentPattern, candidate), plus a central amber Thing (owl:Thing, root anchor). BRIDGE_TO filaments connect matched tiers with labels: bold promoted filament (Smart Contract → SmartContract, confidence 0.94), bold colocated filament (Policy → Policy), dashed candidate filament (Agent Pattern → AgentPattern, 0.73), and a dashed NO-BRIDGE arrow from Orphan Note drifting toward the owl:Thing marked "drifts to orphan zone". All 3 ontology classes inherit from Thing. Preserve all labels.'

gen "08-scoring-radar" \
  'Eight-Signal Scoring Radar — a central amber hexagonal hub labelled "SCORE: sigmoid(a=12, bias=0.42), threshold >= 0.60". Eight cyan signal nodes radiate outward from the hub in a radar pattern: S1 WikilinkToOntology (w=0.20), S2 SemanticCooccurrence (w=0.15), S3 ExplicitOwlDeclaration (w=0.15), S4 AgentProposal (w=0.20), S5 MaturityMarker (w=0.10), S6 CentralityInKG (w=0.10), S7 AuthoringRecency (w=0.05), S8 AuthorityScore (w=0.05). Each signal has a thin glowing filament feeding into the central hub. Two output arrows leave the hub: a bold arrow going up-right to a violet SURFACES TO BROKER INBOX node (labelled "crosses 0.60"), and a dashed arrow going down-right to a red AUTO-EXPIRE node (labelled "below 0.35 after 3 days"). Preserve every label verbatim. Radial composition.'

gen "09-physics-outcome" \
  'Physics Layer Outcome — a vertical layered composition showing how metadata becomes visual structure. From top to bottom: 1 AUTHORITATIVE CORE (inner Z) in amber containing "Stable Canon" (maturity: authoritative, high authority-score) — this is the anchored centre. 2 ROLE BANDS (horizontal) containing three labelled clusters arranged side-by-side: Concepts (cyan), Processes (violet), Agents (emerald). 3 PHYSICALITY CLUSTERS with two side-by-side clusters: Abstract (cyan), Concrete (emerald). 4 DRAFT ORBITAL (outer Z) in blue-grey containing "Active Drafts, maturity:draft, drifting". 5 UNCLAIMED ZONE (off to the side, distinct) in red: "Orphan Notes, no BRIDGE_TO, high visibility". Thin arrows show the force topology: canon anchors the core and pulls outward; drafts orbit outward; orphans repel from all clusters. Show the labels and the visual grouping clearly.'

gen "10-prior-art-quadrant" \
  'Prior-Art Positioning Quadrant — a 2x2 scatter plot with X-axis labelled "Formality (low → high)" and Y-axis labelled "Interactivity (low → high)". Nine named tools plotted at specific positions: Obsidian/Logseq at lower-left-to-mid (informal but interactive), Roam Research near them, NotebookLM in lower-middle (informal, moderately interactive), Glean near centre, Neo4j Bloom in centre, Palantir Foundry in upper-mid-right, TopBraid Composer and Protege in far-right-lower (formal but static), and VisionClaw prominently in the UPPER-RIGHT QUADRANT (high formality AND high interactivity — the genuine novelty position, the only tool in that quadrant). Each tool has a small labelled dot. VisionClaw dot is distinctly highlighted in amber or violet with a glow, visually emphasising its unique position. Background quadrant labels: "Formal + Static" (bottom-right), "Formal + Interactive" (top-right), "Informal + Static" (bottom-left), "Informal + Interactive" (top-left). Professional scientific scatter plot aesthetic, crisp grid lines.'

gen "11-dual-ingestion-loops" \
  'Dual Ingestion Loops — two pentagons side by side, both instantiating the same 5-beat mesh pattern (Discover → Codify → Validate → Integrate → Amplify → back to Discover). LEFT PENTAGON: "WORKFLOW INGESTION LOOP" with the 5 stages for workflows. RIGHT PENTAGON: "KNOWLEDGE MIGRATION LOOP" with the 5 stages for knowledge units (Detection=8-signal scoring, Codification=Draft OWL delta via ontology_propose, Validation=Broker inbox DecisionCanvas, Integration=PR merged BRIDGE_TO promoted, Amplification=Physics re-settles canon expands). Each pentagon is drawn as a proper cyclic flow with clear directional arrows. Below or between them: a central amber "Mesh Pattern" callout showing the abstract pattern Discover→Codify→Validate→Integrate→Amplify. Both pentagons have dashed lines connecting to the central pattern showing they share the same structure. Colour-code stages consistently (emerald for Discovery/Amplification, cyan for Codification/Integration, violet for Validation). Make it clear that this is two INSTANCES of the same pattern — the WORKFLOW variant and the KNOWLEDGE variant.'

echo ""
echo "=== ALL GENERATED ==="
ls -lh ${UPGRADED}/0[6-9]*.png ${UPGRADED}/1[01]*.png 2>/dev/null
