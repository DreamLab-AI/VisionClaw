#!/bin/bash
# Batch-generate VisionClaw diagrams via Nano Banana Pro
set -e
export PATH="$HOME/.bun/bin:$PATH"
cd /home/devuser/.claude/skills/art/tools

RENDERED=/home/devuser/workspace/project/docs/diagrams/rendered
UPGRADED=/home/devuser/workspace/project/docs/diagrams/upgraded

AESTHETIC='Professional dark-mode technical diagram in VisionClaw brand aesthetic. Deep midnight-navy background (#0A1020) with subtle volumetric atmospheric haze. Crystalline nodes with soft inner luminescence and thin bright borders. Violet #8B5CF6 glow for governance/human-judgment elements, cyan #00D4FF for orchestration/agents/reasoning, emerald #10B981 for discovery/knowledge/ingestion, amber #F59E0B accents for trust hubs. Clean sans-serif typography (Inter-style) in off-white #E8F4FC. Thin directional energy filaments for connections with gradient glow. Cinematic sci-fi UI concept art fused with engineering blueprint precision. NO hand-drawn wobble, NO cartoon, NO watercolour, NO decorative clutter. Crisp geometric clarity. Preserve the structural layout of the reference diagram exactly including all text labels verbatim. Add depth via subtle drop shadows and rim lighting. Ensure every label from the reference is legible in the final image. 16:9 composition.'

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

# Diagram 02: Insight Ingestion Cycle
gen "02-insight-ingestion-cycle" \
  'The Insight Ingestion Loop showing how shadow workflows become sanctioned organisational intelligence. Five stages arranged left-to-right with a central hub: 1 DISCOVERY (emerald) passive agent monitoring detects shadow pattern; 2 CODIFICATION (cyan) IRIS maps DAG OWL 2 formalisation Nostr provenance bead; 3 VALIDATION (violet) Judgment Broker reviews strategic fit bias check Decision Canvas; 4 INTEGRATION (cyan) promoted to live mesh with SLAs ownership quality; 5 AMPLIFICATION (emerald) pattern propagates across applicable teams. Bold arrows connect each stage sequentially. A dashed back-arrow from 5 to 1 labelled "new shadow patterns emerge" closes the loop. A central amber hub labelled "The Governed Mesh: OWL 2, Nostr, CUDA" connects to all five stages via dashed lines showing the mesh orchestrates every stage. This is a cyclical feedback loop diagram.'

# Diagram 03: Four-Plane Voice
gen "03-four-plane-voice" \
  'Four-Plane Voice Architecture showing private versus public audio routing. Top half PRIVATE PLANES (1:1 agent dialogue, emerald glow): PLANE 1 User Mic to Whisper STT to Agent labelled "private thoughts PTT held"; PLANE 2 Agent to Kokoro TTS to User Ear labelled "private replies always-on". Bottom half PUBLIC PLANES (shared spatial audio, violet glow): PLANE 3 User Mic to LiveKit SFU to All Users labelled "room voice PTT released"; PLANE 4 Agent TTS to LiveKit to All Users labelled "announce configured public". Central elements: USER node (Browser/Quest 3, blue), AGENT node (Nostr Identity/Claude-Flow Skill, amber), LIVEKIT SFU node (Opus 48kHz HRTF Spatial, cyan, central hub). Flow arrows connect user mic through all four planes with HRTF panned return to user. Show all plane labels verbatim.'

# Diagram 04: MCP Tools Radial Hub
gen "04-mcp-tools-radial" \
  'Seven MCP Tools radiating from a central OWL 2 reasoning core. CENTRAL HUB (amber glow, hexagonal panel): WHELK-RS OWL 2 EL Reasoner, EL++ Subsumption, Consistency Checking. SEVEN TOOLS arranged around the hub (cyan glow): ontology_discover (semantic search plus Whelk expansion), ontology_read (enriched note plus axioms context), ontology_query (validated Cypher schema-aware), ontology_traverse (BFS graph walk from IRI), ontology_propose (amend then check then GitHub PR), ontology_validate (consistency check against reasoner), ontology_status (service health plus statistics). Above: 83 CLAUDE-FLOW AGENTS panel (emerald) with sub-labels Research/Creative/Governance and Identity/Spatial/Ops, connected to all seven tools via MCP arrows. Below/right: Neo4j plus in-mem repo data store (violet), GitHub Logseq repo data store (violet). Tool 5 (propose) has a dashed arrow to GitHub labelled "PR on mutation". Central Whelk has bidirectional arrow to Neo4j store. Show all tool names and descriptions verbatim.'

# Diagram 05: Hexagonal Architecture
gen "05-architecture-hexagonal" \
  'Complete VisionClaw system architecture. Five main regions arranged top-to-bottom-left-to-right: BROWSER CLIENT (React 19 + WebGPU, emerald) containing React Three Fiber (desktop graph), Babylon.js XR (immersive VR Quest 3 hand tracking), WASM scene-effects (drawer-fx sparklines zero-copy Float32Array). TRANSPORT layer (amber) containing Binary V5 (9-byte header plus 36 bytes/node) and REST plus NIP-98 (Nostr Schnorr auth plus Bearer session). RUST BACKEND (hexagonal architecture) containing Ports/Trait Boundaries subgroup (cyan: GraphRepository, InferenceEngine, GpuPhysicsAdapter, SolidPodRepository, VectorRepository), Adapters/Implementations subgroup (violet: Neo4jAdapter, WhelkAdapter, CUDA Physics, JSS Pod, RuVectorAdapter), and 21 Actix Actors (supervised concurrency, GraphSupervisor then PhysicsOrch then ForceCompute). DATA PLANE (violet) with Neo4j 5.13, RuVector pgvector (1.17M HNSW 384d), Solid Pods. GPU CUDA 13.1 (emerald) with 92 Kernel Functions (force semantic stress) and Analytics (K-Means Louvain PageRank LOF). AGENT MESH (amber) with 83 Skills (Claude-Flow RAFT) and Nostr Identities (NIP-98 delegation). Arrows: Client bidirectional with Transport, Transport bidirectional with Actors, Actors to Ports implemented by Adapters to Data Plane, Actors bidirectional GPU, Mesh MCP bidirectional Actors, Mesh dashed arrow "PR on mutation" to Neo4j. All labels verbatim.'

echo ""
echo "=== ALL GENERATED ==="
ls -lh ${UPGRADED}/
