#!/bin/bash
# VisionClaw Pipeline Context Generator
# Extracts all files involved in GitHub → Neo4j → GPU → Client pipeline
# Output: context.txt with location metadata

OUTPUT_FILE="/mnt/mldata/githubs/AR-AI-Knowledge-Graph/context.txt"
PROJECT_ROOT="/mnt/mldata/githubs/AR-AI-Knowledge-Graph"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}VisionClaw Pipeline Context Generator${NC}"
echo -e "${BLUE}======================================${NC}\n"

# Clear output file
> "$OUTPUT_FILE"

# Header
cat >> "$OUTPUT_FILE" << 'EOF'
================================================================================
VISIONCLAW COMPLETE PIPELINE CONTEXT
GitHub → Markdown Parse → Oxigraph → Metadata/Edges → GPU → Graph → Client
================================================================================

Generated: $(date)
Purpose: Complete source code context for LLM analysis of the VisionClaw pipeline

TABLE OF CONTENTS:
1. GitHub Interaction (Download & Fetch)
2. Markdown Parsing & Analysis
3. Oxigraph Persistence Interaction (ADR-11)
4. Metadata & Edge Building
5. GPU Computation (CUDA)
6. Graph Structure Building
7. Client Communication (WebSocket + HTTP)

================================================================================

EOF

# Function to add file with metadata
add_file() {
    local filepath="$1"
    local category="$2"
    local description="$3"

    if [ ! -f "$filepath" ]; then
        echo -e "${YELLOW}⚠️  File not found: $filepath${NC}"
        return
    fi

    local relative_path="${filepath#$PROJECT_ROOT/}"
    local line_count=$(wc -l < "$filepath")
    local file_size=$(stat -c%s "$filepath" 2>/dev/null || stat -f%z "$filepath" 2>/dev/null)

    echo -e "${GREEN}✓${NC} $relative_path ($line_count lines)"

    cat >> "$OUTPUT_FILE" << EOF

################################################################################
# FILE: $relative_path
# CATEGORY: $category
# DESCRIPTION: $description
# LINES: $line_count
# SIZE: $file_size bytes
################################################################################

EOF

    cat "$filepath" >> "$OUTPUT_FILE"
    echo -e "\n" >> "$OUTPUT_FILE"
}

# 1. GITHUB INTERACTION
echo -e "\n${BLUE}[1/7] GitHub Interaction Files${NC}"
cat >> "$OUTPUT_FILE" << 'EOF'
================================================================================
SECTION 1: GITHUB INTERACTION (Download & Fetch)
================================================================================

EOF

add_file "$PROJECT_ROOT/src/services/github/api.rs" "GitHub" "GitHub API client wrapper"
add_file "$PROJECT_ROOT/src/services/github/config.rs" "GitHub" "GitHub configuration and authentication"
add_file "$PROJECT_ROOT/src/services/github/content_enhanced.rs" "GitHub" "Enhanced content fetching API"
add_file "$PROJECT_ROOT/src/services/github/pr.rs" "GitHub" "Pull request API"
add_file "$PROJECT_ROOT/src/services/github/types.rs" "GitHub" "GitHub type definitions"
add_file "$PROJECT_ROOT/src/services/github/mod.rs" "GitHub" "GitHub module entry point"
add_file "$PROJECT_ROOT/src/services/github_sync_service.rs" "GitHub" "Main GitHub sync orchestration"
add_file "$PROJECT_ROOT/src/services/streaming_sync_service.rs" "GitHub" "Streaming sync for large repos"
add_file "$PROJECT_ROOT/src/services/local_markdown_sync.rs" "GitHub" "Local markdown synchronization"
add_file "$PROJECT_ROOT/src/handlers/admin_sync_handler.rs" "GitHub" "Admin endpoint for triggering sync"

# 2. MARKDOWN PARSING
echo -e "\n${BLUE}[2/7] Markdown Parsing & Analysis Files${NC}"
cat >> "$OUTPUT_FILE" << 'EOF'
================================================================================
SECTION 2: MARKDOWN PARSING & ANALYSIS
================================================================================

EOF

add_file "$PROJECT_ROOT/src/services/parsers/ontology_parser.rs" "Parsing" "Parse markdown for OWL ontologies"
add_file "$PROJECT_ROOT/src/services/parsers/knowledge_graph_parser.rs" "Parsing" "Parse markdown for graph structures"
add_file "$PROJECT_ROOT/src/ontology/parser/parser.rs" "Parsing" "Core ontology parsing logic"
add_file "$PROJECT_ROOT/src/ontology/parser/converter.rs" "Parsing" "Convert parsed data to OWL"
add_file "$PROJECT_ROOT/src/ontology/parser/assembler.rs" "Parsing" "Assemble OWL components"
add_file "$PROJECT_ROOT/src/services/semantic_analyzer.rs" "Parsing" "Extract semantic features from markdown"
add_file "$PROJECT_ROOT/src/services/owl_extractor_service.rs" "Parsing" "Extract OWL axioms from markdown"
add_file "$PROJECT_ROOT/src/services/ontology_enrichment_service.rs" "Parsing" "Enrich ontology with inferred data"
add_file "$PROJECT_ROOT/src/inference/owl_parser.rs" "Parsing" "Parse OWL syntax in markdown"
add_file "$PROJECT_ROOT/src/services/ontology_pipeline_service.rs" "Parsing" "Complete ontology processing pipeline"
add_file "$PROJECT_ROOT/src/services/ontology_reasoner.rs" "Parsing" "Reasoning over parsed ontologies"
add_file "$PROJECT_ROOT/src/services/ontology_reasoning_service.rs" "Parsing" "Reasoning service coordination"
add_file "$PROJECT_ROOT/src/services/ontology_converter.rs" "Parsing" "Convert between ontology formats"
add_file "$PROJECT_ROOT/src/reasoning/horned_integration.rs" "Parsing" "Horned-OWL reasoner integration"
add_file "$PROJECT_ROOT/src/reasoning/custom_reasoner.rs" "Parsing" "Custom reasoning logic"

# 3. OXIGRAPH PERSISTENCE (ADR-11)
echo -e "\n${BLUE}[3/7] Oxigraph Persistence Files${NC}"
cat >> "$OUTPUT_FILE" << 'EOF'
================================================================================
SECTION 3: OXIGRAPH PERSISTENCE INTERACTION (ADR-11)
================================================================================

EOF

add_file "$PROJECT_ROOT/src/adapters/oxigraph_graph_repository.rs" "Oxigraph" "Graph persistence adapter"
add_file "$PROJECT_ROOT/src/adapters/oxigraph_ontology_repository.rs" "Oxigraph" "Ontology CRUD operations"
add_file "$PROJECT_ROOT/src/adapters/oxigraph_settings_repository.rs" "Oxigraph" "Settings persistence"
add_file "$PROJECT_ROOT/src/ports/ontology_repository.rs" "Ports" "Ontology repository trait"
add_file "$PROJECT_ROOT/src/ports/knowledge_graph_repository.rs" "Ports" "Graph repository trait"
add_file "$PROJECT_ROOT/src/ports/graph_repository.rs" "Ports" "Generic graph port"
add_file "$PROJECT_ROOT/src/repositories/query_builder.rs" "Oxigraph" "Dynamic query construction"
add_file "$PROJECT_ROOT/src/cqrs/handlers/ontology_handlers.rs" "Oxigraph" "Ontology command handlers"
add_file "$PROJECT_ROOT/src/cqrs/queries/ontology_queries.rs" "Oxigraph" "Ontology query handlers"
add_file "$PROJECT_ROOT/src/cqrs/handlers/graph_handlers.rs" "Oxigraph" "Graph command handlers"
add_file "$PROJECT_ROOT/src/cqrs/queries/graph_queries.rs" "Oxigraph" "Graph query handlers"

# 4. METADATA & EDGES
echo -e "\n${BLUE}[4/7] Metadata & Edge Building Files${NC}"
cat >> "$OUTPUT_FILE" << 'EOF'
================================================================================
SECTION 4: METADATA & EDGE BUILDING
================================================================================

EOF

add_file "$PROJECT_ROOT/src/models/metadata.rs" "Metadata" "Metadata data structures"
add_file "$PROJECT_ROOT/src/services/file_service.rs" "Metadata" "File metadata extraction"
add_file "$PROJECT_ROOT/src/actors/metadata_actor.rs" "Metadata" "Metadata processing actor"
add_file "$PROJECT_ROOT/src/services/edge_generation.rs" "Edges" "Generate edges from relationships"
add_file "$PROJECT_ROOT/src/services/edge_classifier.rs" "Edges" "Classify edge types"
add_file "$PROJECT_ROOT/src/models/edge.rs" "Edges" "Edge data model"
add_file "$PROJECT_ROOT/src/models/graph.rs" "Graph" "Graph data structures"
add_file "$PROJECT_ROOT/src/models/node.rs" "Graph" "Node data structures"
add_file "$PROJECT_ROOT/src/services/graph_serialization.rs" "Graph" "Serialize graph for transfer"

# 5. GPU COMPUTATION
echo -e "\n${BLUE}[5/7] GPU Computation Files${NC}"
cat >> "$OUTPUT_FILE" << 'EOF'
================================================================================
SECTION 5: GPU COMPUTATION (CUDA)
================================================================================

EOF

# GPU Actors
add_file "$PROJECT_ROOT/src/actors/gpu/gpu_manager_actor.rs" "GPU" "GPU resource orchestration"
add_file "$PROJECT_ROOT/src/actors/gpu/gpu_resource_actor.rs" "GPU" "GPU memory management"
add_file "$PROJECT_ROOT/src/actors/gpu/cuda_stream_wrapper.rs" "GPU" "CUDA stream wrapper"
add_file "$PROJECT_ROOT/src/actors/gpu/force_compute_actor.rs" "GPU" "Physics force computation"
add_file "$PROJECT_ROOT/src/actors/gpu/clustering_actor.rs" "GPU" "GPU clustering algorithms"
add_file "$PROJECT_ROOT/src/actors/gpu/anomaly_detection_actor.rs" "GPU" "Anomaly detection"
add_file "$PROJECT_ROOT/src/actors/gpu/constraint_actor.rs" "GPU" "Constraint enforcement"
add_file "$PROJECT_ROOT/src/actors/gpu/ontology_constraint_actor.rs" "GPU" "Ontology constraints to forces"
add_file "$PROJECT_ROOT/src/actors/gpu/stress_majorization_actor.rs" "GPU" "Stress majorization layout"

# Core GPU
add_file "$PROJECT_ROOT/src/utils/unified_gpu_compute.rs" "GPU" "PRIMARY: Unified GPU interface"
add_file "$PROJECT_ROOT/src/gpu/memory_manager.rs" "GPU" "GPU memory allocation"
add_file "$PROJECT_ROOT/src/gpu/dynamic_buffer_manager.rs" "GPU" "Dynamic buffer management"
add_file "$PROJECT_ROOT/src/gpu/streaming_pipeline.rs" "GPU" "Streaming GPU pipeline"
add_file "$PROJECT_ROOT/src/gpu/semantic_forces.rs" "GPU" "Semantic force computation"

# CUDA Kernels
add_file "$PROJECT_ROOT/src/utils/visionclaw_unified.cu" "CUDA" "Main unified kernel"
add_file "$PROJECT_ROOT/src/utils/visionclaw_unified_stability.cu" "CUDA" "Stability-enhanced kernel"
add_file "$PROJECT_ROOT/src/utils/dynamic_grid.cu" "CUDA" "Dynamic spatial grid"
add_file "$PROJECT_ROOT/src/utils/gpu_clustering_kernels.cu" "CUDA" "K-means, DBSCAN clustering"
add_file "$PROJECT_ROOT/src/utils/sssp_compact.cu" "CUDA" "Shortest path SSSP"
add_file "$PROJECT_ROOT/src/utils/gpu_landmark_apsp.cu" "CUDA" "All-pairs shortest path"
add_file "$PROJECT_ROOT/src/utils/ontology_constraints.cu" "CUDA" "Ontology constraint forces"
add_file "$PROJECT_ROOT/src/utils/semantic_forces.cu" "CUDA" "Semantic attraction/repulsion"
add_file "$PROJECT_ROOT/src/utils/stress_majorization.cu" "CUDA" "Stress majorization"

# GPU Utilities
add_file "$PROJECT_ROOT/src/utils/gpu_memory.rs" "GPU" "GPU memory utilities"
add_file "$PROJECT_ROOT/src/utils/gpu_diagnostics.rs" "GPU" "GPU diagnostics"
add_file "$PROJECT_ROOT/src/utils/gpu_safety.rs" "GPU" "GPU safety checks"
add_file "$PROJECT_ROOT/src/utils/cuda_error_handling.rs" "GPU" "CUDA error handling"
add_file "$PROJECT_ROOT/src/utils/ptx.rs" "GPU" "PTX loading utilities"

# GPU Adapters
add_file "$PROJECT_ROOT/src/adapters/gpu_semantic_analyzer.rs" "GPU" "GPU semantic analysis adapter"
add_file "$PROJECT_ROOT/src/adapters/actix_physics_adapter.rs" "GPU" "Physics actor adapter"
add_file "$PROJECT_ROOT/src/ports/gpu_physics_adapter.rs" "GPU" "GPU physics port"

# Constraint Translation (single path: src/physics/ontology_constraints.rs)
add_file "$PROJECT_ROOT/src/physics/ontology_constraints.rs" "Physics" "Physics constraint definitions"
add_file "$PROJECT_ROOT/src/physics/semantic_constraints.rs" "Physics" "Semantic constraint logic"

# 6. GRAPH STRUCTURE BUILDING
echo -e "\n${BLUE}[6/7] Graph Structure Building Files${NC}"
cat >> "$OUTPUT_FILE" << 'EOF'
================================================================================
SECTION 6: GRAPH STRUCTURE BUILDING
================================================================================

EOF

add_file "$PROJECT_ROOT/src/actors/graph_state_actor.rs" "Graph" "Central graph state management"
add_file "$PROJECT_ROOT/src/actors/physics_orchestrator_actor.rs" "Graph" "Physics orchestration"
add_file "$PROJECT_ROOT/src/actors/semantic_processor_actor.rs" "Graph" "Semantic processing"
add_file "$PROJECT_ROOT/src/actors/ontology_actor.rs" "Graph" "Ontology state management"
add_file "$PROJECT_ROOT/src/actors/client_coordinator_actor.rs" "Graph" "Client coordination"
add_file "$PROJECT_ROOT/src/application/knowledge_graph/queries.rs" "Graph" "Knowledge graph queries"
add_file "$PROJECT_ROOT/src/application/knowledge_graph/directives.rs" "Graph" "Knowledge graph directives"
add_file "$PROJECT_ROOT/src/application/graph/queries.rs" "Graph" "Generic graph queries"
add_file "$PROJECT_ROOT/src/physics/mod.rs" "Physics" "Physics module entry"
add_file "$PROJECT_ROOT/src/physics/stress_majorization.rs" "Physics" "Stress majorization layout"

# 7. CLIENT COMMUNICATION
echo -e "\n${BLUE}[7/7] Client Communication Files${NC}"
cat >> "$OUTPUT_FILE" << 'EOF'
================================================================================
SECTION 7: CLIENT COMMUNICATION (WebSocket + HTTP)
================================================================================

EOF

# WebSocket Handlers
add_file "$PROJECT_ROOT/src/handlers/socket_flow_handler.rs" "WebSocket" "PRIMARY: Main graph WebSocket"
add_file "$PROJECT_ROOT/src/handlers/realtime_websocket_handler.rs" "WebSocket" "Real-time updates"
add_file "$PROJECT_ROOT/src/handlers/websocket_settings_handler.rs" "WebSocket" "Settings WebSocket"
add_file "$PROJECT_ROOT/src/handlers/multi_mcp_websocket_handler.rs" "WebSocket" "Multi-agent visualization"
add_file "$PROJECT_ROOT/src/handlers/bots_visualization_handler.rs" "WebSocket" "Bot visualization WebSocket"
add_file "$PROJECT_ROOT/src/handlers/speech_socket_handler.rs" "WebSocket" "Speech interface WebSocket"

# Protocol & Messages
add_file "$PROJECT_ROOT/src/utils/socket_flow_messages.rs" "Protocol" "Binary graph protocol messages"
add_file "$PROJECT_ROOT/src/utils/standard_websocket_messages.rs" "Protocol" "Standard WebSocket messages"
add_file "$PROJECT_ROOT/src/handlers/websocket_utils.rs" "Protocol" "WebSocket utilities"
add_file "$PROJECT_ROOT/src/utils/websocket_heartbeat.rs" "Protocol" "WebSocket heartbeat"
add_file "$PROJECT_ROOT/src/protocols/binary_settings_protocol.rs" "Protocol" "Binary settings protocol"

# HTTP API
add_file "$PROJECT_ROOT/src/handlers/api_handler/graph/mod.rs" "HTTP" "Graph REST API"
add_file "$PROJECT_ROOT/src/handlers/api_handler/ontology/mod.rs" "HTTP" "Ontology REST API"
add_file "$PROJECT_ROOT/src/handlers/api_handler/analytics/mod.rs" "HTTP" "Analytics REST API"
add_file "$PROJECT_ROOT/src/handlers/api_handler/analytics/websocket_integration.rs" "HTTP" "Analytics WebSocket bridge"
add_file "$PROJECT_ROOT/src/handlers/graph_state_handler.rs" "HTTP" "Graph state HTTP endpoints"
add_file "$PROJECT_ROOT/src/handlers/graph_export_handler.rs" "HTTP" "Export graph data"

# Analytics
add_file "$PROJECT_ROOT/src/handlers/api_handler/analytics/clustering.rs" "Analytics" "Clustering results"
add_file "$PROJECT_ROOT/src/handlers/api_handler/analytics/anomaly.rs" "Analytics" "Anomaly detection results"
add_file "$PROJECT_ROOT/src/handlers/api_handler/analytics/community.rs" "Analytics" "Community detection"

# Footer
cat >> "$OUTPUT_FILE" << 'EOF'

================================================================================
END OF VISIONCLAW PIPELINE CONTEXT
================================================================================

PIPELINE FLOW SUMMARY:
1. GitHub API fetches markdown → src/services/github_sync_service.rs
2. Markdown parsed for OWL + semantics → src/services/ontology_parser.rs
3. Data stored in Oxigraph → src/adapters/oxigraph_ontology_repository.rs
4. Edges + metadata generated → src/services/edge_generation.rs
5. OWL axioms → GPU forces → src/physics/ontology_constraints.rs
6. CUDA kernels compute physics → src/utils/unified_gpu_compute.rs
7. Graph state built → src/actors/graph_state_actor.rs
8. WebSocket streams to client → src/handlers/socket_flow_handler.rs

Total pipeline files: ~120
Primary entry points: 8 (listed above)

================================================================================
EOF

echo -e "\n${GREEN}✓ Context file generated: $OUTPUT_FILE${NC}"
echo -e "${BLUE}File size: $(du -h "$OUTPUT_FILE" | cut -f1)${NC}"
echo -e "${BLUE}Total lines: $(wc -l < "$OUTPUT_FILE")${NC}\n"
