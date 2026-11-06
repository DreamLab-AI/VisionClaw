/**
 * useGraphSelection — selection state, camera fly-to, and search/deselect event wiring.
 *
 * Extracted from GraphManager.tsx (Phase B1 modularisation).
 */
import { useState, useEffect, useRef } from 'react'
import * as THREE from 'three'
import type { GraphData, Node as GraphNode } from '../managers/graphDataManager'
import {
  highlightMask as sharedMask, type HighlightMask,
  HIGHLIGHT_SELECTED, HIGHLIGHT_FOCUS_NEIGHBOUR, HIGHLIGHT_SEARCH_HIT,
} from '../state/highlightMask'
import { SOURCE, writeEmphasis } from '../explore/filterRegistry'

export interface GraphSelectionOptions {
  graphData: GraphData
  nodeIdToIndexMap: Map<string, number>
  nodePositionsRef: React.MutableRefObject<Float32Array | null>
  connectionCountMap: Map<string, number>
  camera: THREE.Camera
  /** The highlight mask to emphasise on (tests inject their own). */
  mask?: HighlightMask
}

/**
 * Every node whose label matches a search query, by the same three tiers the
 * fly-to target is chosen with (prefix, substring, all words). Tiers are unioned,
 * so the emphasis shows every hit, not only the one the camera flies to.
 */
export function searchHitIds(nodes: readonly GraphNode[], query: string): string[] {
  const lq = query.trim().toLowerCase()
  if (!lq) return []
  const words = lq.includes(' ') ? lq.split(/\s+/).filter(w => w.length > 1) : []
  const hits: string[] = []
  for (const n of nodes) {
    const label = (n.label || '').toLowerCase()
    if (label.includes(lq) || (words.length > 0 && words.every(w => label.includes(w)))) hits.push(String(n.id))
  }
  return hits
}

export interface GraphSelectionReturn {
  selectedNodeId: string | null
  setSelectedNodeId: React.Dispatch<React.SetStateAction<string | null>>
  /** Camera destination for the eased fly-to (node position + size-scaled standoff). */
  flyToTargetRef: React.MutableRefObject<THREE.Vector3 | null>
  /** The node centre the camera + controls should look at during the fly-to. */
  flyToLookAtRef: React.MutableRefObject<THREE.Vector3 | null>
  flyToProgressRef: React.MutableRefObject<number>
}

/**
 * Camera standoff distance scaled by a node's apparent visual size. Bigger nodes
 * (more connections / larger metadata.size) get a proportionally larger standoff
 * so the framing is consistent regardless of node scale. Bounded to a sane
 * world-unit range. Mirrors the KG scaling shape (base + sqrt(degree)) without
 * pulling the full computeNodeScale dependency graph into the selection hook.
 */
function computeStandoff(node: GraphNode, degree: number): number {
  const sizeHint = Number(node.metadata?.size) || 1
  const scale = Math.max(1, sizeHint + Math.sqrt(Math.max(0, degree)) * 0.8)
  return Math.min(120, Math.max(14, scale * 6))
}

/**
 * Extract ADR-049 assertion-version attribution from node metadata, tolerating
 * both camelCase and the backend snake_case. Returns undefined unless the core
 * identity fields (agent did + activity URN) are present, so provenance-less
 * nodes dispatch no attribution. The agent identity is the authenticated
 * principal recorded server-side; the client never synthesises it.
 */
function extractAttribution(
  metadata: Record<string, any> | undefined
): { didNostr: string; activityUrn: string; generatedAtTime: string; signatureValid: boolean } | undefined {
  if (!metadata) return undefined
  const didNostr = metadata.didNostr ?? metadata.did_nostr ?? metadata.agentDid ?? metadata.agent_did
  const activityUrn = metadata.activityUrn ?? metadata.activity_urn
  if (typeof didNostr !== 'string' || typeof activityUrn !== 'string') return undefined
  const generatedAtTime =
    metadata.generatedAtTime ?? metadata.generated_at_time ?? metadata.generatedAt ?? ''
  const signatureValid =
    metadata.signatureValid ?? metadata.signature_valid ?? false
  return {
    didNostr,
    activityUrn,
    generatedAtTime: String(generatedAtTime),
    signatureValid: Boolean(signatureValid),
  }
}

export function useGraphSelection(opts: GraphSelectionOptions): GraphSelectionReturn {
  const { graphData, nodeIdToIndexMap, nodePositionsRef, connectionCountMap, camera } = opts
  const mask = opts.mask ?? sharedMask

  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null)
  const flyToTargetRef  = useRef<THREE.Vector3 | null>(null)
  const flyToLookAtRef  = useRef<THREE.Vector3 | null>(null)
  const flyToProgressRef = useRef(0)
  // Guards the click-to-focus effect so it fires once per selection CHANGE, not
  // on every graphData tick (the position stream churns graphData constantly).
  const lastFocusedIdRef = useRef<string | null>(null)

  /** Resolve a node's current world position: live SAB first, static fallback. */
  const resolveNodePos = (node: GraphNode): THREE.Vector3 | null => {
    const idx = nodeIdToIndexMap.get(String(node.id))
    const positions = nodePositionsRef.current
    if (idx !== undefined && positions && idx * 3 + 2 < positions.length) {
      return new THREE.Vector3(positions[idx * 3], positions[idx * 3 + 1], positions[idx * 3 + 2])
    }
    if (node.position) return new THREE.Vector3(node.position.x, node.position.y, node.position.z)
    return null
  }

  /** Start an eased camera fly-to that looks at `node` and approaches it along
   *  the current view direction, standing off by a size-scaled distance. */
  const focusCameraOnNode = (node: GraphNode): void => {
    const targetPos = resolveNodePos(node)
    if (!targetPos) return
    const degree = connectionCountMap.get(String(node.id)) || 0
    const standoff = computeStandoff(node, degree)
    // Keep the current viewing direction: approach from where the camera already
    // is, just closer. Falls back to a default offset when the camera sits on top
    // of the node (degenerate zero-length direction).
    const dir = new THREE.Vector3().subVectors(camera.position, targetPos)
    if (dir.lengthSq() < 1e-6) dir.set(0, 0.3, 1)
    dir.normalize().multiplyScalar(standoff)
    flyToLookAtRef.current = targetPos.clone()
    flyToTargetRef.current = targetPos.clone().add(dir)
    flyToProgressRef.current = 0
  }

  // Dispatch visionclaw:node-selected when selection changes
  useEffect(() => {
    if (!selectedNodeId) {
      window.dispatchEvent(new CustomEvent('visionclaw:node-selected', { detail: null }))
      return
    }
    const node = graphData.nodes.find(n => String(n.id) === selectedNodeId)
    if (!node) return

    const neighborIds = new Set<string>()
    graphData.edges.forEach(edge => {
      const src = String(edge.source)
      const tgt = String(edge.target)
      if (src === selectedNodeId) neighborIds.add(tgt)
      if (tgt === selectedNodeId) neighborIds.add(src)
    })
    const neighbors = Array.from(neighborIds).map(nid => {
      const n = graphData.nodes.find(nd => String(nd.id) === nid)
      return { id: nid, label: n?.label || nid }
    })

    window.dispatchEvent(new CustomEvent('visionclaw:node-selected', {
      detail: {
        nodeId: selectedNodeId,
        label: node.label,
        metadata: node.metadata || {},
        connectionCount: connectionCountMap.get(selectedNodeId) || neighborIds.size,
        neighbors,
        // ADR-049 attribution, when the node carries assertion-version provenance
        // metadata. Undefined otherwise — the panel renders the section only when
        // present AND the provenance.showAttribution flag is on.
        attribution: extractAttribution(node.metadata),
      },
    }))
  }, [selectedNodeId, graphData.nodes, graphData.edges, connectionCountMap])

  // ADR-2183: selection and its neighbourhood emphasise through the highlight mask.
  // writeEmphasis skips unchanged sets, so graphData churn costs no recomposition.
  useEffect(() => {
    if (!selectedNodeId) {
      writeEmphasis(SOURCE.SELECTION, HIGHLIGHT_SELECTED, null, mask)
      writeEmphasis(SOURCE.NEIGHBOURS, HIGHLIGHT_FOCUS_NEIGHBOUR, null, mask)
      return
    }
    const neighbours: string[] = []
    for (const edge of graphData.edges) {
      const src = String(edge.source)
      const tgt = String(edge.target)
      if (src === selectedNodeId) neighbours.push(tgt)
      else if (tgt === selectedNodeId) neighbours.push(src)
    }
    writeEmphasis(SOURCE.SELECTION, HIGHLIGHT_SELECTED, [selectedNodeId], mask)
    writeEmphasis(SOURCE.NEIGHBOURS, HIGHLIGHT_FOCUS_NEIGHBOUR, neighbours, mask)
  }, [selectedNodeId, graphData.edges, mask])

  // Click-to-focus: when the selection changes (node click, neighbour click, or
  // search), ease the camera to look at + approach the node. Guarded by
  // lastFocusedIdRef so the constant graphData churn from the position stream
  // does not re-trigger a fly every frame.
  useEffect(() => {
    if (!selectedNodeId) { lastFocusedIdRef.current = null; return }
    if (selectedNodeId === lastFocusedIdRef.current) return
    lastFocusedIdRef.current = selectedNodeId
    const node = graphData.nodes.find(n => String(n.id) === selectedNodeId)
    if (node) focusCameraOnNode(node)
  }, [selectedNodeId, graphData.nodes, nodeIdToIndexMap, connectionCountMap, camera])

  // Search and deselect event listeners
  useEffect(() => {
    const handleSearch = (event: Event) => {
      const { query, nodeId } = (event as CustomEvent).detail || {}
      let targetNode: GraphNode | undefined

      // Search hits → SEARCH_HIT emphasis: every label match for a text query, or
      // the one requested node.
      const hits = query ? searchHitIds(graphData.nodes, String(query)) : nodeId ? [String(nodeId)] : []
      writeEmphasis(SOURCE.SEARCH, HIGHLIGHT_SEARCH_HIT, hits, mask)

      if (nodeId) {
        targetNode = graphData.nodes.find(n => String(n.id) === nodeId)
      }
      if (!targetNode && query) {
        const lq = query.toLowerCase()
        targetNode = graphData.nodes.find(n => n.label.toLowerCase().startsWith(lq))
        if (!targetNode) targetNode = graphData.nodes.find(n => n.label.toLowerCase().includes(lq))
        if (!targetNode && lq.includes(' ')) {
          const words = lq.split(/\s+/).filter((w: string) => w.length > 1)
          targetNode = graphData.nodes.find(n => {
            const label = n.label.toLowerCase()
            return words.every((w: string) => label.includes(w))
          })
        }
      }
      if (!targetNode) return

      // Selecting the node triggers the click-to-focus effect above, which
      // performs the size-scaled eased fly-to. If the same node is re-searched
      // (selection unchanged), fly explicitly since the effect would no-op.
      if (String(targetNode.id) === selectedNodeId) {
        focusCameraOnNode(targetNode)
      } else {
        setSelectedNodeId(String(targetNode.id))
      }
    }

    const handleDeselect = () => {
      setSelectedNodeId(null)
      writeEmphasis(SOURCE.SEARCH, HIGHLIGHT_SEARCH_HIT, null, mask)
    }

    window.addEventListener('visionclaw:search', handleSearch)
    window.addEventListener('visionclaw:node-deselect', handleDeselect)
    return () => {
      window.removeEventListener('visionclaw:search', handleSearch)
      window.removeEventListener('visionclaw:node-deselect', handleDeselect)
    }
  }, [graphData.nodes, nodeIdToIndexMap, connectionCountMap, camera, nodePositionsRef, selectedNodeId, mask])

  return { selectedNodeId, setSelectedNodeId, flyToTargetRef, flyToLookAtRef, flyToProgressRef }
}
