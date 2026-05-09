/**
 * @deprecated DORMANT SERVICE -- registered in InnovationManager but never
 * imported or called by any UI component, hook, or other module outside of
 * InnovationManager.initialize(). 677 lines of unused code (graph diff,
 * node matching, structural similarity analysis). Consider removing in
 * the next dead-code cleanup pass.  Audited 2026-05-09.
 */

import { Vector3, Color } from 'three';
import { createLogger } from '../../../utils/loggerConfig';
import type { GraphData, Node as KGNode } from '../managers/graphDataManager';

const logger = createLogger('GraphComparison');

export interface NodeMatch {
  logseqNodeId: string;
  visionflowNodeId: string;
  confidence: number;
  matchType: 'exact' | 'semantic' | 'structural' | 'fuzzy';
  similarity: {
    name: number;
    type: number;
    connections: number;
    metadata: number;
  };
}

export interface RelationshipBridge {
  id: string;
  sourceGraphId: string;
  targetGraphId: string;
  sourceNodeId: string;
  targetNodeId: string;
  bridgeType: 'matched' | 'related' | 'similar';
  strength: number;
  visualStyle: {
    color: Color;
    opacity: number;
    thickness: number;
    pattern: 'solid' | 'dashed' | 'dotted';
  };
}

export interface GraphDifference {
  onlyInLogseq: KGNode[];
  onlyInVisionflow: KGNode[];
  commonNodes: NodeMatch[];
  structuralDifferences: {
    logseqClusters: NodeCluster[];
    visionflowClusters: NodeCluster[];
    uniquePatterns: Pattern[];
  };
}

export interface NodeCluster {
  id: string;
  nodes: string[];
  centerPosition: Vector3;
  characteristics: {
    averageConnections: number;
    dominantType: string;
    density: number;
  };
}

export interface Pattern {
  id: string;
  type: 'hub' | 'chain' | 'cluster' | 'bridge' | 'isolate';
  nodes: string[];
  strength: number;
  description: string;
}

export interface SimilarityAnalysis {
  overallSimilarity: number;
  structuralSimilarity: number;
  semanticSimilarity: number;
  topologicalSimilarity: number;
  recommendations: string[];
}

export class GraphComparison {
  private static instance: GraphComparison;
  private nodeMatches: Map<string, NodeMatch> = new Map();
  private relationshipBridges: Map<string, RelationshipBridge> = new Map();

  private constructor() {}

  public static getInstance(): GraphComparison {
    if (!GraphComparison.instance) {
      GraphComparison.instance = new GraphComparison();
    }
    return GraphComparison.instance;
  }

  
  public async findNodeMatches(
    logseqGraph: GraphData,
    visionflowGraph: GraphData,
    options: {
      exactMatchWeight: number;
      semanticMatchWeight: number;
      structuralMatchWeight: number;
      minimumConfidence: number;
    } = {
      exactMatchWeight: 0.4,
      semanticMatchWeight: 0.3,
      structuralMatchWeight: 0.3,
      minimumConfidence: 0.6
    }
  ): Promise<NodeMatch[]> {
    logger.info('Finding node matches between graphs');

    const matches: NodeMatch[] = [];
    this.nodeMatches.clear();

    for (const logseqNode of logseqGraph.nodes) {
      for (const visionflowNode of visionflowGraph.nodes) {
        const match = this.calculateNodeMatch(
          logseqNode,
          visionflowNode,
          logseqGraph,
          visionflowGraph,
          options
        );

        if (match.confidence >= options.minimumConfidence) {
          matches.push(match);
          this.nodeMatches.set(`${match.logseqNodeId}-${match.visionflowNodeId}`, match);
        }
      }
    }

    
    matches.sort((a, b) => b.confidence - a.confidence);
    return this.resolveMatchConflicts(matches);
  }

  
  private calculateNodeMatch(
    logseqNode: KGNode,
    visionflowNode: KGNode,
    logseqGraph: GraphData,
    visionflowGraph: GraphData,
    options: any
  ): NodeMatch {
    
    const nameSimilarity = this.calculateStringSimilarity(
      logseqNode.label || logseqNode.id,
      visionflowNode.label || visionflowNode.id
    );

    
    const typeSimilarity = this.calculateTypeSimilarity(
      logseqNode.metadata?.type,
      visionflowNode.metadata?.type
    );

    
    const logseqConnections = logseqGraph.edges.filter(
      e => e.source === logseqNode.id || e.target === logseqNode.id
    ).length;
    const visionflowConnections = visionflowGraph.edges.filter(
      e => e.source === visionflowNode.id || e.target === visionflowNode.id
    ).length;
    const connectionSimilarity = 1 - Math.abs(logseqConnections - visionflowConnections) / 
      Math.max(logseqConnections, visionflowConnections, 1);

    
    const metadataSimilarity = this.calculateMetadataSimilarity(
      logseqNode.metadata,
      visionflowNode.metadata
    );

    
    const confidence = 
      nameSimilarity * options.exactMatchWeight +
      (typeSimilarity + connectionSimilarity) * options.structuralMatchWeight +
      metadataSimilarity * options.semanticMatchWeight;

    
    let matchType: NodeMatch['matchType'] = 'fuzzy';
    if (nameSimilarity > 0.9 && typeSimilarity > 0.8) matchType = 'exact';
    else if (metadataSimilarity > 0.7) matchType = 'semantic';
    else if (connectionSimilarity > 0.7) matchType = 'structural';

    return {
      logseqNodeId: logseqNode.id,
      visionflowNodeId: visionflowNode.id,
      confidence,
      matchType,
      similarity: {
        name: nameSimilarity,
        type: typeSimilarity,
        connections: connectionSimilarity,
        metadata: metadataSimilarity
      }
    };
  }

  
  public createRelationshipBridges(
    matches: NodeMatch[],
    logseqGraph: GraphData,
    visionflowGraph: GraphData
  ): RelationshipBridge[] {
    logger.info('Creating relationship bridges');

    const bridges: RelationshipBridge[] = [];
    this.relationshipBridges.clear();

    matches.forEach((match, index) => {
      const bridgeId = `bridge-${index}`;
      const strength = match.confidence;
      
      
      const visualStyle = this.getBridgeVisualStyle(match);

      const bridge: RelationshipBridge = {
        id: bridgeId,
        sourceGraphId: 'logseq',
        targetGraphId: 'visionflow',
        sourceNodeId: match.logseqNodeId,
        targetNodeId: match.visionflowNodeId,
        bridgeType: this.getBridgeType(match),
        strength,
        visualStyle
      };

      bridges.push(bridge);
      this.relationshipBridges.set(bridgeId, bridge);
    });

    return bridges;
  }

  
  public analyzeDifferences(
    logseqGraph: GraphData,
    visionflowGraph: GraphData,
    matches: NodeMatch[]
  ): GraphDifference {
    logger.info('Analyzing graph differences');

    const matchedLogseqIds = new Set(matches.map(m => m.logseqNodeId));
    const matchedVisionflowIds = new Set(matches.map(m => m.visionflowNodeId));

    const onlyInLogseq = logseqGraph.nodes.filter(node => !matchedLogseqIds.has(node.id));
    const onlyInVisionflow = visionflowGraph.nodes.filter(node => !matchedVisionflowIds.has(node.id));

    
    const logseqClusters = this.detectClusters(logseqGraph);
    const visionflowClusters = this.detectClusters(visionflowGraph);

    
    const uniquePatterns = this.detectUniquePatterns(logseqGraph, visionflowGraph, matches);

    return {
      onlyInLogseq,
      onlyInVisionflow,
      commonNodes: matches,
      structuralDifferences: {
        logseqClusters,
        visionflowClusters,
        uniquePatterns
      }
    };
  }

  
  public performSimilarityAnalysis(
    logseqGraph: GraphData,
    visionflowGraph: GraphData,
    matches: NodeMatch[]
  ): SimilarityAnalysis {
    logger.info('Performing similarity analysis');

    
    const structuralSimilarity = this.calculateStructuralSimilarity(logseqGraph, visionflowGraph);
    const semanticSimilarity = this.calculateSemanticSimilarity(matches);
    const topologicalSimilarity = this.calculateTopologicalSimilarity(logseqGraph, visionflowGraph);

    
    const overallSimilarity = (
      structuralSimilarity * 0.4 +
      semanticSimilarity * 0.3 +
      topologicalSimilarity * 0.3
    );

    
    const recommendations = this.generateRecommendations(
      logseqGraph,
      visionflowGraph,
      matches,
      { structuralSimilarity, semanticSimilarity, topologicalSimilarity }
    );

    return {
      overallSimilarity,
      structuralSimilarity,
      semanticSimilarity,
      topologicalSimilarity,
      recommendations
    };
  }

  
  public getDifferenceHighlighting(differences: GraphDifference): {
    logseqHighlights: Map<string, { color: Color; intensity: number }>;
    visionflowHighlights: Map<string, { color: Color; intensity: number }>;
  } {
    const logseqHighlights = new Map();
    const visionflowHighlights = new Map();

    
    differences.onlyInLogseq.forEach(node => {
      logseqHighlights.set(node.id, {
        color: new Color('#ff4444'), 
        intensity: 0.8
      });
    });

    differences.onlyInVisionflow.forEach(node => {
      visionflowHighlights.set(node.id, {
        color: new Color('#ff4444'), 
        intensity: 0.8
      });
    });

    
    differences.commonNodes.forEach(match => {
      const confidence = match.confidence;
      const color = new Color().lerpColors(
        new Color('#ffaa00'), 
        new Color('#44ff44'), 
        confidence
      );

      logseqHighlights.set(match.logseqNodeId, {
        color,
        intensity: confidence
      });

      visionflowHighlights.set(match.visionflowNodeId, {
        color,
        intensity: confidence
      });
    });

    return { logseqHighlights, visionflowHighlights };
  }

  

  private calculateStringSimilarity(str1: string, str2: string): number {
    const longer = str1.length > str2.length ? str1 : str2;
    const shorter = str1.length > str2.length ? str2 : str1;
    
    if (longer.length === 0) return 1.0;
    
    return (longer.length - this.levenshteinDistance(longer, shorter)) / longer.length;
  }

  private levenshteinDistance(str1: string, str2: string): number {
    const matrix = [];
    for (let i = 0; i <= str2.length; i++) {
      matrix[i] = [i];
    }
    for (let j = 0; j <= str1.length; j++) {
      matrix[0][j] = j;
    }
    for (let i = 1; i <= str2.length; i++) {
      for (let j = 1; j <= str1.length; j++) {
        if (str2.charAt(i - 1) === str1.charAt(j - 1)) {
          matrix[i][j] = matrix[i - 1][j - 1];
        } else {
          matrix[i][j] = Math.min(
            matrix[i - 1][j - 1] + 1,
            matrix[i][j - 1] + 1,
            matrix[i - 1][j] + 1
          );
        }
      }
    }
    return matrix[str2.length][str1.length];
  }

  private calculateTypeSimilarity(type1?: string, type2?: string): number {
    if (!type1 || !type2) return 0;
    if (type1 === type2) return 1;
    
    
    const typeHierarchy: Record<string, string[]> = {
      'file': ['document', 'text', 'code'],
      'folder': ['directory', 'container', 'namespace'],
      'function': ['method', 'procedure', 'operation'],
      'class': ['type', 'interface', 'structure'],
      'variable': ['property', 'field', 'attribute']
    };

    
    for (const [parent, children] of Object.entries(typeHierarchy)) {
      if ((type1 === parent && children.includes(type2)) ||
          (type2 === parent && children.includes(type1)) ||
          (children.includes(type1) && children.includes(type2))) {
        return 0.7;
      }
    }

    return 0;
  }

  private calculateMetadataSimilarity(meta1?: any, meta2?: any): number {
    if (!meta1 || !meta2) return 0;
    
    const keys1 = Object.keys(meta1);
    const keys2 = Object.keys(meta2);
    const allKeys = new Set([...keys1, ...keys2]);
    
    let similarities = 0;
    let totalKeys = allKeys.size;
    
    for (const key of allKeys) {
      if (meta1[key] && meta2[key]) {
        if (typeof meta1[key] === 'string' && typeof meta2[key] === 'string') {
          similarities += this.calculateStringSimilarity(meta1[key], meta2[key]);
        } else if (meta1[key] === meta2[key]) {
          similarities += 1;
        }
      }
    }
    
    return totalKeys > 0 ? similarities / totalKeys : 0;
  }

  private resolveMatchConflicts(matches: NodeMatch[]): NodeMatch[] {
    const usedLogseqNodes = new Set<string>();
    const usedVisionflowNodes = new Set<string>();
    const resolvedMatches: NodeMatch[] = [];

    for (const match of matches) {
      if (!usedLogseqNodes.has(match.logseqNodeId) && !usedVisionflowNodes.has(match.visionflowNodeId)) {
        resolvedMatches.push(match);
        usedLogseqNodes.add(match.logseqNodeId);
        usedVisionflowNodes.add(match.visionflowNodeId);
      }
    }

    return resolvedMatches;
  }

  private getBridgeVisualStyle(match: NodeMatch): RelationshipBridge['visualStyle'] {
    const confidence = match.confidence;
    
    
    const colorMap = {
      exact: new Color('#00ff00'),
      semantic: new Color('#0088ff'),
      structural: new Color('#ff8800'),
      fuzzy: new Color('#ff00ff')
    };

    return {
      color: colorMap[match.matchType],
      opacity: confidence * 0.8 + 0.2,
      thickness: confidence * 2 + 0.5,
      pattern: confidence > 0.8 ? 'solid' : confidence > 0.6 ? 'dashed' : 'dotted'
    };
  }

  private getBridgeType(match: NodeMatch): RelationshipBridge['bridgeType'] {
    if (match.matchType === 'exact') return 'matched';
    if (match.confidence > 0.7) return 'related';
    return 'similar';
  }

  private detectClusters(graph: GraphData): NodeCluster[] {
    
    const clusters: NodeCluster[] = [];
    const visited = new Set<string>();

    for (const node of graph.nodes) {
      if (visited.has(node.id)) continue;

      const cluster = this.exploreCluster(node.id, graph, visited);
      if (cluster.nodes.length > 1) {
        clusters.push(cluster);
      }
    }

    return clusters;
  }

  private exploreCluster(startNodeId: string, graph: GraphData, visited: Set<string>): NodeCluster {
    const clusterNodes: string[] = [];
    const queue = [startNodeId];
    
    while (queue.length > 0) {
      const nodeId = queue.shift()!;
      if (visited.has(nodeId)) continue;
      
      visited.add(nodeId);
      clusterNodes.push(nodeId);
      
      
      const connectedNodes = graph.edges
        .filter(edge => edge.source === nodeId || edge.target === nodeId)
        .map(edge => edge.source === nodeId ? edge.target : edge.source)
        .filter(id => !visited.has(id));
      
      queue.push(...connectedNodes);
    }

    
    const positions = clusterNodes
      .map(id => graph.nodes.find(n => n.id === id)?.position)
      .filter(pos => pos) as Array<{ x: number; y: number; z: number }>;

    const centerPosition = new Vector3(
      positions.reduce((sum, pos) => sum + pos.x, 0) / positions.length,
      positions.reduce((sum, pos) => sum + pos.y, 0) / positions.length,
      positions.reduce((sum, pos) => sum + pos.z, 0) / positions.length
    );

    const connections = clusterNodes.reduce((sum, nodeId) => {
      return sum + graph.edges.filter(e => e.source === nodeId || e.target === nodeId).length;
    }, 0);

    const types = clusterNodes.map(id => 
      graph.nodes.find(n => n.id === id)?.metadata?.type || 'unknown'
    );
    const dominantType = this.getMostCommon(types);

    return {
      id: `cluster-${startNodeId}`,
      nodes: clusterNodes,
      centerPosition,
      characteristics: {
        averageConnections: connections / clusterNodes.length,
        dominantType,
        density: connections / (clusterNodes.length * (clusterNodes.length - 1) / 2)
      }
    };
  }

  private detectUniquePatterns(
    logseqGraph: GraphData,
    visionflowGraph: GraphData,
    matches: NodeMatch[]
  ): Pattern[] {
    const patterns: Pattern[] = [];
    
    
    const logseqHubs = this.detectHubs(logseqGraph);
    const visionflowHubs = this.detectHubs(visionflowGraph);
    
    patterns.push(...logseqHubs, ...visionflowHubs);
    
    return patterns;
  }

  private detectHubs(graph: GraphData): Pattern[] {
    const connectionCounts = new Map<string, number>();
    
    graph.edges.forEach(edge => {
      connectionCounts.set(edge.source, (connectionCounts.get(edge.source) || 0) + 1);
      connectionCounts.set(edge.target, (connectionCounts.get(edge.target) || 0) + 1);
    });

    const averageConnections = Array.from(connectionCounts.values())
      .reduce((sum, count) => sum + count, 0) / connectionCounts.size;

    const hubs: Pattern[] = [];
    connectionCounts.forEach((count, nodeId) => {
      if (count > averageConnections * 2) {
        hubs.push({
          id: `hub-${nodeId}`,
          type: 'hub',
          nodes: [nodeId],
          strength: count / averageConnections,
          description: `Hub node with ${count} connections`
        });
      }
    });

    return hubs;
  }

  private calculateStructuralSimilarity(graph1: GraphData, graph2: GraphData): number {
    const nodes1 = graph1.nodes.length;
    const nodes2 = graph2.nodes.length;
    const edges1 = graph1.edges.length;
    const edges2 = graph2.edges.length;

    const nodeSimilarity = 1 - Math.abs(nodes1 - nodes2) / Math.max(nodes1, nodes2);
    const edgeSimilarity = 1 - Math.abs(edges1 - edges2) / Math.max(edges1, edges2);

    return (nodeSimilarity + edgeSimilarity) / 2;
  }

  private calculateSemanticSimilarity(matches: NodeMatch[]): number {
    if (matches.length === 0) return 0;
    
    return matches.reduce((sum, match) => sum + match.confidence, 0) / matches.length;
  }

  private calculateTopologicalSimilarity(graph1: GraphData, graph2: GraphData): number {
    
    const metrics1 = this.calculateGraphMetrics(graph1);
    const metrics2 = this.calculateGraphMetrics(graph2);

    const densitySim = 1 - Math.abs(metrics1.density - metrics2.density);
    const clusteringSim = 1 - Math.abs(metrics1.clustering - metrics2.clustering);

    return (densitySim + clusteringSim) / 2;
  }

  private calculateGraphMetrics(graph: GraphData): { density: number; clustering: number } {
    const n = graph.nodes.length;
    const m = graph.edges.length;
    const maxEdges = n * (n - 1) / 2;
    
    const density = maxEdges > 0 ? m / maxEdges : 0;
    
    
    const clustering = 0; 

    return { density, clustering };
  }

  private generateRecommendations(
    logseqGraph: GraphData,
    visionflowGraph: GraphData,
    matches: NodeMatch[],
    similarities: { structuralSimilarity: number; semanticSimilarity: number; topologicalSimilarity: number }
  ): string[] {
    const recommendations: string[] = [];

    if (similarities.structuralSimilarity < 0.5) {
      recommendations.push('Consider restructuring one graph to better match the other');
    }

    if (similarities.semanticSimilarity < 0.6) {
      recommendations.push('Review node naming and typing conventions for consistency');
    }

    if (matches.length < Math.min(logseqGraph.nodes.length, visionflowGraph.nodes.length) * 0.3) {
      recommendations.push('Low node matching detected - consider adding more metadata or improving labeling');
    }

    if (similarities.topologicalSimilarity < 0.4) {
      recommendations.push('Graph structures are quite different - consider identifying key structural patterns');
    }

    return recommendations;
  }

  private getMostCommon<T>(array: T[]): T {
    const counts = new Map<T, number>();
    let maxCount = 0;
    let mostCommon = array[0];

    for (const item of array) {
      const count = (counts.get(item) || 0) + 1;
      counts.set(item, count);
      if (count > maxCount) {
        maxCount = count;
        mostCommon = item;
      }
    }

    return mostCommon;
  }

  
  public dispose(): void {
    this.nodeMatches.clear();
    this.relationshipBridges.clear();
    logger.info('Graph comparison disposed');
  }
}

// Export singleton instance
export const graphComparison = GraphComparison.getInstance();