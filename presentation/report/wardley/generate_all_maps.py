#!/usr/bin/env python3
"""
Generate all 5 Wardley maps for The Coordination Collapse thesis.
Uses the wardley-maps skill generator.
"""

import sys
sys.path.insert(0, '/home/devuser/.claude/skills/wardley-maps/tools')
from generate_wardley_map import WardleyMapGenerator

import os

OUTPUT_DIR = '/home/devuser/workspace/project/presentation/report/wardley'
os.makedirs(OUTPUT_DIR, exist_ok=True)

def save_map(name, components, dependencies, title, width=1200, height=800):
    gen = WardleyMapGenerator(width=width, height=height)
    html = gen.create_map(components, dependencies)
    # Inject title into the HTML
    html = html.replace('<body>', f'<body><h2 style="text-align:center;font-family:sans-serif;color:#1B2A4A">{title}</h2>')
    path = os.path.join(OUTPUT_DIR, f'{name}.html')
    with open(path, 'w') as f:
        f.write(html)
    print(f'  Saved: {path}')

# ============================================================
# MAP 1: The Coordination Value Chain
# ============================================================
print("Map 1: Coordination Value Chain")

map1_components = [
    # Top of value chain (visible to user)
    {"name": "Strategic Outcomes", "visibility": 0.95, "evolution": 0.15, "type": "user"},
    {"name": "Strategic Direction", "visibility": 0.85, "evolution": 0.20, "type": "default"},
    {"name": "Value Alignment", "visibility": 0.78, "evolution": 0.25, "type": "default"},
    {"name": "Ethical Adjudication", "visibility": 0.72, "evolution": 0.18, "type": "default"},
    # The boundary - Judgment Broker
    {"name": "JUDGMENT BROKER", "visibility": 0.62, "evolution": 0.35, "type": "highlight"},
    {"name": "Jagged Frontier\nNavigation", "visibility": 0.55, "evolution": 0.22, "type": "default"},
    {"name": "Cross-Functional\nCoordination", "visibility": 0.58, "evolution": 0.50, "type": "default"},
    # Moving to commodity
    {"name": "Information Routing", "visibility": 0.45, "evolution": 0.75, "type": "default"},
    {"name": "Status Aggregation", "visibility": 0.38, "evolution": 0.80, "type": "default"},
    {"name": "Task Assignment", "visibility": 0.30, "evolution": 0.72, "type": "default"},
    {"name": "Context Synthesis", "visibility": 0.35, "evolution": 0.70, "type": "default"},
    # Commodity layer
    {"name": "AI Agent\nOrchestration", "visibility": 0.20, "evolution": 0.65, "type": "default"},
    {"name": "Individual Execution", "visibility": 0.12, "evolution": 0.85, "type": "default"},
]

map1_deps = [
    ("Strategic Outcomes", "Strategic Direction"),
    ("Strategic Direction", "Value Alignment"),
    ("Value Alignment", "JUDGMENT BROKER"),
    ("Ethical Adjudication", "JUDGMENT BROKER"),
    ("JUDGMENT BROKER", "Cross-Functional\nCoordination"),
    ("JUDGMENT BROKER", "Jagged Frontier\nNavigation"),
    ("Cross-Functional\nCoordination", "Information Routing"),
    ("Cross-Functional\nCoordination", "Status Aggregation"),
    ("Information Routing", "AI Agent\nOrchestration"),
    ("Status Aggregation", "AI Agent\nOrchestration"),
    ("Task Assignment", "AI Agent\nOrchestration"),
    ("Context Synthesis", "AI Agent\nOrchestration"),
    ("AI Agent\nOrchestration", "Individual Execution"),
]

save_map("01-coordination-value-chain", map1_components, map1_deps,
         "Map 1: The Coordination Value Chain — Where AI Commoditises and Where Judgment Remains")


# ============================================================
# MAP 2: Three Models Compared
# ============================================================
print("Map 2: Three Models Compared")

map2_components = [
    # Shared user need
    {"name": "Organisational\nCoordination", "visibility": 0.95, "evolution": 0.40, "type": "user"},

    # Block model - pushes everything to commodity
    {"name": "[Block]\nWorld Model", "visibility": 0.70, "evolution": 0.70, "type": "default"},
    {"name": "[Block]\nIntelligence Layer", "visibility": 0.58, "evolution": 0.65, "type": "default"},
    {"name": "[Block]\nICs / DRIs", "visibility": 0.45, "evolution": 0.80, "type": "default"},

    # Every model - keeps coordination in custom
    {"name": "[Every]\nPersonal Agents", "visibility": 0.70, "evolution": 0.45, "type": "default"},
    {"name": "[Every]\nCultural Norms", "visibility": 0.58, "evolution": 0.25, "type": "default"},
    {"name": "[Every]\nEmergent\nCoordination", "visibility": 0.45, "evolution": 0.30, "type": "default"},

    # DreamLab model - structured boundary
    {"name": "[Mesh]\nDiscovery Engine", "visibility": 0.75, "evolution": 0.50, "type": "highlight"},
    {"name": "[Mesh]\nJudgment Broker", "visibility": 0.62, "evolution": 0.35, "type": "highlight"},
    {"name": "[Mesh]\nDeclarative\nGovernance", "visibility": 0.50, "evolution": 0.55, "type": "highlight"},
    {"name": "[Mesh]\nDAG Orchestration", "visibility": 0.38, "evolution": 0.65, "type": "highlight"},
]

map2_deps = [
    ("Organisational\nCoordination", "[Block]\nWorld Model"),
    ("Organisational\nCoordination", "[Every]\nPersonal Agents"),
    ("Organisational\nCoordination", "[Mesh]\nDiscovery Engine"),
    ("[Block]\nWorld Model", "[Block]\nIntelligence Layer"),
    ("[Block]\nIntelligence Layer", "[Block]\nICs / DRIs"),
    ("[Every]\nPersonal Agents", "[Every]\nCultural Norms"),
    ("[Every]\nCultural Norms", "[Every]\nEmergent\nCoordination"),
    ("[Mesh]\nDiscovery Engine", "[Mesh]\nJudgment Broker"),
    ("[Mesh]\nJudgment Broker", "[Mesh]\nDeclarative\nGovernance"),
    ("[Mesh]\nDeclarative\nGovernance", "[Mesh]\nDAG Orchestration"),
]

save_map("02-three-models-compared", map2_components, map2_deps,
         "Map 2: Three Organisational Models — Block (Commodity) vs Every (Custom) vs Mesh (Structured Boundary)")


# ============================================================
# MAP 3: Middle Manager Evolution
# ============================================================
print("Map 3: Middle Manager Evolution")

map3_components = [
    {"name": "Organisational\nEffectiveness", "visibility": 0.95, "evolution": 0.40, "type": "user"},

    # OLD role - moving to commodity (marked with evolution arrows)
    {"name": "Information\nRouting ➜", "visibility": 0.70, "evolution": 0.72, "type": "default"},
    {"name": "Status\nReporting ➜", "visibility": 0.62, "evolution": 0.78, "type": "default"},
    {"name": "Task\nDelegation ➜", "visibility": 0.55, "evolution": 0.75, "type": "default"},
    {"name": "Meeting\nFacilitation ➜", "visibility": 0.48, "evolution": 0.82, "type": "default"},
    {"name": "Performance\nMonitoring ➜", "visibility": 0.42, "evolution": 0.70, "type": "default"},

    # NEW role - remains in genesis/custom
    {"name": "Jagged Frontier\nNavigation", "visibility": 0.78, "evolution": 0.15, "type": "highlight"},
    {"name": "Mesh Coherence\nManagement", "visibility": 0.72, "evolution": 0.22, "type": "highlight"},
    {"name": "Ethical\nAdjudication", "visibility": 0.65, "evolution": 0.12, "type": "highlight"},
    {"name": "Cross-Functional\nJudgment", "visibility": 0.58, "evolution": 0.28, "type": "highlight"},
    {"name": "Compound Loop\nCuration", "visibility": 0.50, "evolution": 0.20, "type": "highlight"},

    # AI layer enabling the shift
    {"name": "AI Agent\nCoordination", "visibility": 0.25, "evolution": 0.65, "type": "default"},
]

map3_deps = [
    ("Organisational\nEffectiveness", "Jagged Frontier\nNavigation"),
    ("Organisational\nEffectiveness", "Information\nRouting ➜"),
    ("Jagged Frontier\nNavigation", "Mesh Coherence\nManagement"),
    ("Mesh Coherence\nManagement", "Cross-Functional\nJudgment"),
    ("Ethical\nAdjudication", "Cross-Functional\nJudgment"),
    ("Cross-Functional\nJudgment", "Compound Loop\nCuration"),
    ("Information\nRouting ➜", "AI Agent\nCoordination"),
    ("Status\nReporting ➜", "AI Agent\nCoordination"),
    ("Task\nDelegation ➜", "AI Agent\nCoordination"),
    ("Meeting\nFacilitation ➜", "AI Agent\nCoordination"),
    ("Performance\nMonitoring ➜", "AI Agent\nCoordination"),
]

save_map("03-middle-manager-evolution", map3_components, map3_deps,
         "Map 3: Middle Manager Evolution — OLD Role (➜ Commodity) vs NEW Role (Genesis/Custom)")


# ============================================================
# MAP 4: Governance Landscape
# ============================================================
print("Map 4: Governance Landscape")

map4_components = [
    {"name": "Trustworthy AI\nOperations", "visibility": 0.95, "evolution": 0.35, "type": "user"},

    # Genesis/Custom - human judgment
    {"name": "Edge Case\nJudgment", "visibility": 0.82, "evolution": 0.12, "type": "highlight"},
    {"name": "Trust\nCalibration", "visibility": 0.75, "evolution": 0.18, "type": "highlight"},
    {"name": "Value Vector\nAlignment", "visibility": 0.70, "evolution": 0.22, "type": "default"},

    # Product - structured governance
    {"name": "Bias Detection", "visibility": 0.62, "evolution": 0.48, "type": "default"},
    {"name": "Trust Variance\nMonitoring", "visibility": 0.55, "evolution": 0.45, "type": "default"},
    {"name": "HITL Precision\nTracking", "visibility": 0.50, "evolution": 0.50, "type": "default"},

    # Moving to product via declarative governance
    {"name": "Declarative\nPolicy Enforcement", "visibility": 0.42, "evolution": 0.58, "type": "default"},
    {"name": "Ontological\nProvenance", "visibility": 0.35, "evolution": 0.40, "type": "default"},

    # Commodity
    {"name": "Compliance\nAudit Trail", "visibility": 0.25, "evolution": 0.82, "type": "default"},
    {"name": "Access Control", "visibility": 0.18, "evolution": 0.88, "type": "default"},
    {"name": "Data Encryption", "visibility": 0.12, "evolution": 0.92, "type": "default"},
]

map4_deps = [
    ("Trustworthy AI\nOperations", "Edge Case\nJudgment"),
    ("Trustworthy AI\nOperations", "Trust\nCalibration"),
    ("Edge Case\nJudgment", "Value Vector\nAlignment"),
    ("Trust\nCalibration", "Bias Detection"),
    ("Trust\nCalibration", "Trust Variance\nMonitoring"),
    ("Bias Detection", "Declarative\nPolicy Enforcement"),
    ("Trust Variance\nMonitoring", "HITL Precision\nTracking"),
    ("HITL Precision\nTracking", "Declarative\nPolicy Enforcement"),
    ("Declarative\nPolicy Enforcement", "Ontological\nProvenance"),
    ("Declarative\nPolicy Enforcement", "Compliance\nAudit Trail"),
    ("Compliance\nAudit Trail", "Access Control"),
    ("Access Control", "Data Encryption"),
]

save_map("04-governance-landscape", map4_components, map4_deps,
         "Map 4: The Governance Landscape — From Human Judgment (Genesis) to Embedded Policy (Commodity)")


# ============================================================
# MAP 5: VisionClaw Technology Stack
# ============================================================
print("Map 5: VisionClaw Technology Stack")

map5_components = [
    {"name": "Agentic Mesh\nCapability", "visibility": 0.95, "evolution": 0.35, "type": "user"},

    # Custom/Genesis - differentiating
    {"name": "OWL 2 Ontology\n(Whelk-rs EL++)", "visibility": 0.82, "evolution": 0.25, "type": "highlight"},
    {"name": "GPU Visualisation\n(92 CUDA kernels)", "visibility": 0.70, "evolution": 0.30, "type": "highlight"},
    {"name": "Nostr Identity\n(NIP-98)", "visibility": 0.60, "evolution": 0.18, "type": "highlight"},

    # Product - structured capabilities
    {"name": "Claude-Flow\nOrchestration", "visibility": 0.75, "evolution": 0.55, "type": "default"},
    {"name": "RuVector Memory\n(1.17M embeddings)", "visibility": 0.50, "evolution": 0.50, "type": "default"},
    {"name": "WebXR Frontend\n(Babylon.js + R3F)", "visibility": 0.65, "evolution": 0.48, "type": "default"},

    # Product/Commodity
    {"name": "83+ Agent Skills", "visibility": 0.42, "evolution": 0.65, "type": "default"},
    {"name": "Neo4j Graph DB", "visibility": 0.30, "evolution": 0.72, "type": "default"},

    # Commodity - infrastructure
    {"name": "PostgreSQL +\npgvector", "visibility": 0.20, "evolution": 0.85, "type": "default"},
    {"name": "NVIDIA GPU\nCompute", "visibility": 0.12, "evolution": 0.80, "type": "default"},
]

map5_deps = [
    ("Agentic Mesh\nCapability", "OWL 2 Ontology\n(Whelk-rs EL++)"),
    ("Agentic Mesh\nCapability", "Claude-Flow\nOrchestration"),
    ("OWL 2 Ontology\n(Whelk-rs EL++)", "Neo4j Graph DB"),
    ("OWL 2 Ontology\n(Whelk-rs EL++)", "Claude-Flow\nOrchestration"),
    ("Claude-Flow\nOrchestration", "83+ Agent Skills"),
    ("Claude-Flow\nOrchestration", "RuVector Memory\n(1.17M embeddings)"),
    ("GPU Visualisation\n(92 CUDA kernels)", "WebXR Frontend\n(Babylon.js + R3F)"),
    ("GPU Visualisation\n(92 CUDA kernels)", "NVIDIA GPU\nCompute"),
    ("Nostr Identity\n(NIP-98)", "OWL 2 Ontology\n(Whelk-rs EL++)"),
    ("RuVector Memory\n(1.17M embeddings)", "PostgreSQL +\npgvector"),
    ("WebXR Frontend\n(Babylon.js + R3F)", "GPU Visualisation\n(92 CUDA kernels)"),
]

save_map("05-visionclaw-tech-stack", map5_components, map5_deps,
         "Map 5: VisionClaw Technology Stack — Verified Components (April 2026)")

print("\n✓ All 5 maps generated successfully")
