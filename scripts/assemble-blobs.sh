#!/bin/bash
# VisionClaw Codebase Blob Assembler
# Assembles client and server code into concatenated text files
# for 1M token context window audit
#
# Output: /tmp/visionclaw-blobs/
#   - server-blob.txt    (~560K tokens)
#   - client-blob.txt    (~306K tokens)
#   - spec-blob.txt      (~83K tokens)
#   - manifest.json      (metadata index)

set -uo pipefail

PROJECT="/home/devuser/workspace/project"
OUTPUT="/tmp/visionclaw-blobs"
mkdir -p "$OUTPUT"

# ============================================================
# UTILITY: Add file with metadata header
# ============================================================
add_file() {
    local blob_file="$1"
    local src_file="$2"
    local category="${3:-source}"

    if [ ! -f "$src_file" ]; then
        return
    fi

    local rel_path="${src_file#$PROJECT/}"
    local lines=$(wc -l < "$src_file")

    echo "" >> "$blob_file"
    echo "// ================================================================" >> "$blob_file"
    echo "// FILE: $rel_path" >> "$blob_file"
    echo "// LINES: $lines | CATEGORY: $category" >> "$blob_file"
    echo "// ================================================================" >> "$blob_file"
    cat "$src_file" >> "$blob_file"
    echo "" >> "$blob_file"
}

# ============================================================
# UTILITY: Strip #[cfg(test)] blocks from Rust files
# ============================================================
add_rust_file_no_tests() {
    local blob_file="$1"
    local src_file="$2"
    local category="${3:-source}"

    if [ ! -f "$src_file" ]; then
        return
    fi

    local rel_path="${src_file#$PROJECT/}"

    # Strip cfg(test) modules using awk
    local content
    content=$(awk '
    BEGIN { skip=0; brace_count=0 }
    /^#\[cfg\(test\)\]/ { skip=1; next }
    skip==1 && /\{/ { brace_count++; next }
    skip==1 && /\}/ { brace_count--; if(brace_count<=0) { skip=0; brace_count=0 }; next }
    skip==1 { next }
    { print }
    ' "$src_file")

    local lines=$(echo "$content" | wc -l)

    echo "" >> "$blob_file"
    echo "// ================================================================" >> "$blob_file"
    echo "// FILE: $rel_path" >> "$blob_file"
    echo "// LINES: $lines | CATEGORY: $category (tests stripped)" >> "$blob_file"
    echo "// ================================================================" >> "$blob_file"
    echo "$content" >> "$blob_file"
    echo "" >> "$blob_file"
}

# ============================================================
# SERVER BLOB
# ============================================================
echo "=== Assembling Server Blob ==="
SERVER_BLOB="$OUTPUT/server-blob.txt"
cat > "$SERVER_BLOB" << 'HEADER'
================================================================================
VISIONCLAW SERVER CODEBASE (Rust/Actix-web)
================================================================================
Architecture: Actix-web + Actor system + CQRS + Hexagonal + GPU (CUDA)
Database: Oxigraph SPARQL triple-store (ADR-11)
Auth: Nostr NIP-98 + session tokens
Physics: GPU-accelerated force-directed layout with CUDA kernels
Features: OWL ontology reasoning, semantic analysis, graph algorithms,
          community detection, anomaly detection, pathfinding, stress majorization
Total files: ~427 Rust source files
================================================================================

HEADER

# 1. Entry point and app state
echo "  [1/12] Entry points..."
add_rust_file_no_tests "$SERVER_BLOB" "$PROJECT/src/main.rs" "entry"
add_rust_file_no_tests "$SERVER_BLOB" "$PROJECT/src/lib.rs" "entry"
add_rust_file_no_tests "$SERVER_BLOB" "$PROJECT/src/app_state.rs" "entry"
add_rust_file_no_tests "$SERVER_BLOB" "$PROJECT/src/openapi.rs" "entry"

# 2. Models
echo "  [2/12] Models..."
find "$PROJECT/src/models/" -name "*.rs" -type f 2>/dev/null | sort | while read f; do
    add_rust_file_no_tests "$SERVER_BLOB" "$f" "model"
done

# 3. Ports (interfaces)
echo "  [3/12] Ports..."
find "$PROJECT/src/ports/" -name "*.rs" -type f 2>/dev/null | sort | while read f; do
    add_rust_file_no_tests "$SERVER_BLOB" "$f" "port"
done

# 4. Errors
echo "  [4/12] Errors..."
find "$PROJECT/src/errors/" -name "*.rs" -type f 2>/dev/null | sort | while read f; do
    add_rust_file_no_tests "$SERVER_BLOB" "$f" "error"
done

# 5. Adapters (excluding test files)
echo "  [5/12] Adapters..."
find "$PROJECT/src/adapters/" -name "*.rs" -type f ! -path "*/tests/*" 2>/dev/null | sort | while read f; do
    add_rust_file_no_tests "$SERVER_BLOB" "$f" "adapter"
done

# 6. CQRS layer
echo "  [6/12] CQRS..."
find "$PROJECT/src/cqrs/" -name "*.rs" -type f 2>/dev/null | sort | while read f; do
    add_rust_file_no_tests "$SERVER_BLOB" "$f" "cqrs"
done

# 7. Application services
echo "  [7/12] Application..."
find "$PROJECT/src/application/" -name "*.rs" -type f ! -path "*/tests/*" 2>/dev/null | sort | while read f; do
    add_rust_file_no_tests "$SERVER_BLOB" "$f" "application"
done

# 8. Services
echo "  [8/12] Services..."
find "$PROJECT/src/services/" -name "*.rs" -type f ! -name "*stub*" 2>/dev/null | sort | while read f; do
    add_rust_file_no_tests "$SERVER_BLOB" "$f" "service"
done

# 9. Actors
echo "  [9/12] Actors..."
find "$PROJECT/src/actors/" -name "*.rs" -type f 2>/dev/null | sort | while read f; do
    add_rust_file_no_tests "$SERVER_BLOB" "$f" "actor"
done

# 10. Handlers
echo "  [10/12] Handlers..."
find "$PROJECT/src/handlers/" -name "*.rs" -type f ! -path "*/tests/*" 2>/dev/null | sort | while read f; do
    add_rust_file_no_tests "$SERVER_BLOB" "$f" "handler"
done

# 11. GPU + Physics
echo "  [11/12] GPU & Physics..."
find "$PROJECT/src/gpu/" -name "*.rs" -type f 2>/dev/null | sort | while read f; do
    add_rust_file_no_tests "$SERVER_BLOB" "$f" "gpu"
done
find "$PROJECT/src/physics/" -name "*.rs" -type f ! -name "*integration_test*" 2>/dev/null | sort | while read f; do
    add_rust_file_no_tests "$SERVER_BLOB" "$f" "physics"
done

# 12. Infrastructure (config, middleware, utils, events, constraints, types, etc)
echo "  [12/12] Infrastructure..."
for dir in config middleware utils events constraints types inference ontology telemetry protocols settings validation; do
    find "$PROJECT/src/$dir/" -name "*.rs" -type f ! -path "*/tests/*" ! -name "*test*" 2>/dev/null | sort | while read f; do
        add_rust_file_no_tests "$SERVER_BLOB" "$f" "$dir"
    done 2>/dev/null || true
done

# Add Cargo.toml for dependency context
echo "" >> "$SERVER_BLOB"
echo "// ================================================================" >> "$SERVER_BLOB"
echo "// FILE: Cargo.toml" >> "$SERVER_BLOB"
echo "// CATEGORY: build-config" >> "$SERVER_BLOB"
echo "// ================================================================" >> "$SERVER_BLOB"
cat "$PROJECT/Cargo.toml" >> "$SERVER_BLOB"

SERVER_LINES=$(wc -l < "$SERVER_BLOB")
echo "  Server blob: $SERVER_LINES lines"

# ============================================================
# CLIENT BLOB
# ============================================================
echo ""
echo "=== Assembling Client Blob ==="
CLIENT_BLOB="$OUTPUT/client-blob.txt"
cat > "$CLIENT_BLOB" << 'HEADER'
================================================================================
VISIONCLAW CLIENT CODEBASE (TypeScript/React/Three.js)
================================================================================
Framework: React 19 + Three.js/R3F + Zustand + TailwindCSS 4
3D: @react-three/fiber + @react-three/drei + @react-three/xr
WASM: scene-effects crate (Rust compiled to WebAssembly)
State: Zustand stores with Immer
UI: Radix UI primitives + Lucide icons + Framer Motion
WebSocket: Binary protocol for real-time position updates
Features: 3D graph visualization, XR/VR mode, voice commands,
          AI integrations (RAGFlow, Perplexity, OpenAI, PocketTts TTS, Whisper STT)
================================================================================

HEADER

# Client files by feature area (excluding tests)
echo "  [1/7] Core app files..."
find "$PROJECT/client/src" -maxdepth 1 \( -name "*.ts" -o -name "*.tsx" \) 2>/dev/null | sort | while read f; do
    add_file "$CLIENT_BLOB" "$f" "core"
done

echo "  [2/7] Features (graph, settings, etc)..."
find "$PROJECT/client/src/features/" \( -name "*.ts" -o -name "*.tsx" \) 2>/dev/null | grep -v "__tests__" | grep -v ".test." | grep -v ".spec." | sort | while read f; do
    add_file "$CLIENT_BLOB" "$f" "feature"
done

echo "  [3/7] Components..."
find "$PROJECT/client/src/components/" \( -name "*.ts" -o -name "*.tsx" \) 2>/dev/null | grep -v "__tests__" | grep -v ".test." | grep -v ".spec." | sort | while read f; do
    add_file "$CLIENT_BLOB" "$f" "component"
done

echo "  [4/7] Hooks & Store..."
find "$PROJECT/client/src/hooks/" "$PROJECT/client/src/store/" \( -name "*.ts" -o -name "*.tsx" \) 2>/dev/null | grep -v "__tests__" | grep -v ".test." | sort | while read f; do
    add_file "$CLIENT_BLOB" "$f" "hook-store"
done

echo "  [5/7] Services & API..."
find "$PROJECT/client/src/services/" "$PROJECT/client/src/api/" -name "*.ts" 2>/dev/null | grep -v "__tests__" | grep -v ".test." | sort | while read f; do
    add_file "$CLIENT_BLOB" "$f" "service"
done

echo "  [6/7] Types, Utils, Config, Settings..."
for dir in types utils config settings; do
    if [ -d "$PROJECT/client/src/$dir/" ]; then
        find "$PROJECT/client/src/$dir/" -name "*.ts" 2>/dev/null | grep -v "__tests__" | grep -v ".test." | sort | while read f; do
            add_file "$CLIENT_BLOB" "$f" "types-utils"
        done
    fi
done

echo "  [7/7] Workers, WASM bridge..."
find "$PROJECT/client/src/workers/" "$PROJECT/client/src/wasm/" -name "*.ts" 2>/dev/null | grep -v ".test." | sort | while read f; do
    add_file "$CLIENT_BLOB" "$f" "worker-wasm"
done

# WASM Rust crate
echo "" >> "$CLIENT_BLOB"
echo "// ================================================================" >> "$CLIENT_BLOB"
echo "// WASM SCENE EFFECTS CRATE (Rust -> WebAssembly)" >> "$CLIENT_BLOB"
echo "// ================================================================" >> "$CLIENT_BLOB"
find "$PROJECT/client/crates/" -name "*.rs" 2>/dev/null | sort | while read f; do
    add_file "$CLIENT_BLOB" "$f" "wasm-rust"
done

# Package.json for dependency context
echo "" >> "$CLIENT_BLOB"
echo "// ================================================================" >> "$CLIENT_BLOB"
echo "// FILE: client/package.json" >> "$CLIENT_BLOB"
echo "// CATEGORY: build-config" >> "$CLIENT_BLOB"
echo "// ================================================================" >> "$CLIENT_BLOB"
cat "$PROJECT/client/package.json" >> "$CLIENT_BLOB"

CLIENT_LINES=$(wc -l < "$CLIENT_BLOB")
echo "  Client blob: $CLIENT_LINES lines"

# ============================================================
# SPEC BLOB (Diagrams + Dense Overview)
# ============================================================
echo ""
echo "=== Assembling Spec Blob ==="
SPEC_BLOB="$OUTPUT/spec-blob.txt"
cat > "$SPEC_BLOB" << 'HEADER'
================================================================================
VISIONCLAW FUNCTIONAL SPECIFICATION & ARCHITECTURE DIAGRAMS
================================================================================
Contains: Dense system overview, Mermaid architecture diagrams,
          data flow diagrams, component diagrams, API reference
All diagrams are Mermaid notation (diagrams-as-code)
================================================================================

HEADER

# Dense overview first (most valuable single doc)
echo "  [1/4] Dense overview..."
add_file "$SPEC_BLOB" "$PROJECT/docs/denseOverview.md" "spec"

# Architecture overview
echo "  [2/4] Architecture docs..."
add_file "$SPEC_BLOB" "$PROJECT/docs/architecture.md" "spec"
add_file "$SPEC_BLOB" "$PROJECT/docs/api-reference.md" "spec"

# Dedicated diagram files (high-density architecture diagrams)
echo "  [3/4] Architecture diagrams..."
find "$PROJECT/docs/diagrams/" -name "*.md" -type f 2>/dev/null | sort | while read f; do
    add_file "$SPEC_BLOB" "$f" "diagram"
done

# Key architecture explanation docs
echo "  [4/4] Architecture explanations..."
for f in \
    "$PROJECT/docs/explanation/system-overview.md" \
    "$PROJECT/docs/explanation/architecture/data-flow.md" \
    "$PROJECT/docs/explanation/architecture/system-architecture.md" \
    "$PROJECT/docs/explanation/architecture/server/overview.md" \
    "$PROJECT/docs/explanation/architecture/client/overview.md" \
    "$PROJECT/docs/explanation/architecture/components/websocket-protocol.md" \
    "$PROJECT/docs/explanation/architecture/gpu/communication-flow.md" \
    "$PROJECT/docs/explanation/architecture/event-driven-architecture.md" \
    "$PROJECT/docs/explanation/architecture/pipeline-sequence-diagrams.md" \
    "$PROJECT/docs/explanation/concepts/architecture/core/server.md" \
    "$PROJECT/docs/explanation/concepts/architecture/core/client.md" \
    "$PROJECT/docs/explanation/concepts/actor-model.md" \
    "$PROJECT/docs/explanation/concepts/physics-engine.md" \
    "$PROJECT/docs/explanation/concepts/hexagonal-architecture.md" \
    "$PROJECT/docs/reference/protocols/binary-websocket.md" \
    "$PROJECT/docs/reference/api/websocket-endpoints.md" \
    "$PROJECT/docs/reference/database/ontology-schema-v2.md" \
    "$PROJECT/docs/reference/database/schemas.md" \
; do
    add_file "$SPEC_BLOB" "$f" "architecture-doc" 2>/dev/null || true
done

SPEC_LINES=$(wc -l < "$SPEC_BLOB")
echo "  Spec blob: $SPEC_LINES lines"

# ============================================================
# MANIFEST
# ============================================================
echo ""
echo "=== Generating Manifest ==="
cat > "$OUTPUT/manifest.json" << EOF
{
  "project": "VisionClaw AI Multi-Agent Knowledge Graph Visualisation",
  "generated": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "blobs": {
    "server-blob.txt": {
      "lines": $SERVER_LINES,
      "estimated_tokens": $(echo "$SERVER_LINES * 3.5 / 1" | bc),
      "language": "Rust",
      "framework": "Actix-web + CUDA",
      "contents": "All server source code (tests stripped), Cargo.toml"
    },
    "client-blob.txt": {
      "lines": $CLIENT_LINES,
      "estimated_tokens": $(echo "$CLIENT_LINES * 3.5 / 1" | bc),
      "language": "TypeScript/TSX",
      "framework": "React 19 + Three.js + R3F",
      "contents": "All client source code (tests excluded), WASM crate, package.json"
    },
    "spec-blob.txt": {
      "lines": $SPEC_LINES,
      "estimated_tokens": $(echo "$SPEC_LINES * 2.5 / 1" | bc),
      "language": "Markdown + Mermaid",
      "contents": "Dense overview, architecture diagrams, API reference, key docs"
    }
  },
  "total_estimated_tokens": $(echo "($SERVER_LINES * 3.5 + $CLIENT_LINES * 3.5 + $SPEC_LINES * 2.5) / 1" | bc),
  "target_context_window": 1000000
}
EOF

echo ""
echo "=== DONE ==="
echo "Files in $OUTPUT/:"
ls -lh "$OUTPUT/"
echo ""
echo "Token estimates:"
cat "$OUTPUT/manifest.json" | python3 -m json.tool 2>/dev/null || cat "$OUTPUT/manifest.json"
