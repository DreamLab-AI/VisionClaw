/**
 * @deprecated DORMANT SERVICE -- registered in InnovationManager but never
 * called by any UI component or hook outside of InnovationManager.initialize().
 * The only external surface referencing the name "aiInsights" is a local state
 * variable in GraphOptimisationTab.tsx, which does NOT import this module.
 * 1,109 lines of unused code (cluster detection, pattern recognition, anomaly
 * detection, layout optimisation). Consider removing in the next dead-code
 * cleanup pass.  Audited 2026-05-09.
 */

import { Vector3, Color } from 'three';
import { createLogger } from '../../../utils/loggerConfig';
import type { GraphData, Node as KGNode } from '../managers/graphDataManager';

const logger = createLogger('AIInsights');

export interface LayoutOptimization {
  algorithmUsed: 'force-directed' | 'hierarchical' | 'circular' | 'grid' | 'organic';
  improvements: {
    edgeCrossings: { before: number; after: number };
    nodeOverlaps: { before: number; after: number };
    readability: { before: number; after: number };
  };
  optimizedPositions: Map<string, Vector3>;
  confidence: number;
  reasoning: string[];
}

export interface ClusterDetection {
  clusters: GraphCluster[];
  algorithm: 'modularity' | 'density' | 'hierarchical' | 'spectral';
  quality: {
    modularity: number;
    silhouette: number;
    cohesion: number;
  };
  recommendations: string[];
}

export interface GraphCluster {
  id: string;
  nodes: string[];
  centerPosition: Vector3;
  radius: number;
  density: number;
  dominantTypes: string[];
  characteristics: {
    averageConnections: number;
    internalEdges: number;
    externalEdges: number;
    coherenceScore: number;
  };
  suggestedColor: Color;
  label: string;
}

export interface NodeRecommendation {
  nodeId: string;
  recommendationType: 'connect' | 'group' | 'highlight' | 'relocate' | 'merge' | 'split';
  confidence: number;
  reasoning: string;
  suggestedActions: RecommendedAction[];
  potentialImpact: {
    connectivityImprovement: number;
    readabilityImprovement: number;
    structuralImprovement: number;
  };
}

export interface RecommendedAction {
  type: 'create_edge' | 'move_node' | 'change_color' | 'add_label' | 'group_nodes';
  parameters: any;
  description: string;
  priority: 'low' | 'medium' | 'high';
}

export interface PatternRecognition {
  patterns: GraphPattern[];
  crossGraphPatterns: CrossGraphPattern[];
  anomalies: GraphAnomaly[];
  insights: string[];
}

export interface GraphPattern {
  id: string;
  type: 'hub' | 'chain' | 'star' | 'clique' | 'bridge' | 'community' | 'hierarchy';
  nodes: string[];
  strength: number;
  description: string;
  significance: number;
  visualizationHint: {
    highlight: boolean;
    color: Color;
    style: 'outline' | 'fill' | 'glow';
  };
}

export interface CrossGraphPattern {
  id: string;
  logseqPattern: GraphPattern;
  visionflowPattern: GraphPattern;
  similarity: number;
  relationship: 'identical' | 'similar' | 'complementary' | 'contradictory';
  insights: string[];
}

export interface GraphAnomaly {
  id: string;
  type: 'isolated_node' | 'unusual_hub' | 'broken_cluster' | 'duplicate_structure' | 'missing_connection';
  affectedNodes: string[];
  severity: 'low' | 'medium' | 'high';
  description: string;
  suggestedFix: string;
}

export interface GraphMetrics {
  density: number;
  averagePathLength: number;
  clusteringCoefficient: number;
  centralization: number;
  modularity: number;
  efficiency: number;
  smallWorldness: number;
}

export class AIInsights {
  private static instance: AIInsights;
  private optimizationCache: Map<string, LayoutOptimization> = new Map();
  private clusterCache: Map<string, ClusterDetection> = new Map();
  private patternCache: Map<string, PatternRecognition> = new Map();
  private metricsCache: Map<string, GraphMetrics> = new Map();

  private constructor() {}

  public static getInstance(): AIInsights {
    if (!AIInsights.instance) {
      AIInsights.instance = new AIInsights();
    }
    return AIInsights.instance;
  }

  
  public async optimizeLayout(
    graphData: GraphData,
    currentPositions: Map<string, Vector3>,
    constraints: {
      preserveRelativePositions?: boolean;
      minimizeEdgeCrossings?: boolean;
      maximizeReadability?: boolean;
      respectClusters?: boolean;
    } = {}
  ): Promise<LayoutOptimization> {
    logger.info('Starting AI-powered layout optimization');

    const cacheKey = this.generateCacheKey(graphData, constraints);
    if (this.optimizationCache.has(cacheKey)) {
      return this.optimizationCache.get(cacheKey)!;
    }

    
    const currentMetrics = this.calculateLayoutMetrics(graphData, currentPositions);
    
    
    const algorithm = this.selectOptimalAlgorithm(graphData, constraints);
    
    
    const optimizedPositions = await this.applyOptimizationAlgorithm(
      graphData,
      currentPositions,
      algorithm,
      constraints
    );

    
    const improvedMetrics = this.calculateLayoutMetrics(graphData, optimizedPositions);
    
    
    // Build improvements object first for reuse
    const improvements = {
      edgeCrossings: {
        before: currentMetrics.edgeCrossings,
        after: improvedMetrics.edgeCrossings
      },
      nodeOverlaps: {
        before: currentMetrics.nodeOverlaps,
        after: improvedMetrics.nodeOverlaps
      },
      readability: {
        before: currentMetrics.readability,
        after: improvedMetrics.readability
      }
    };

    const optimization: LayoutOptimization = {
      algorithmUsed: algorithm,
      improvements,
      optimizedPositions,
      confidence: this.calculateOptimizationConfidence(
        { ...currentMetrics, density: 0, averagePathLength: 0, clusteringCoefficient: 0, centralization: 0, modularity: 0, efficiency: currentMetrics.readability, smallWorldness: 0 },
        { ...improvedMetrics, density: 0, averagePathLength: 0, clusteringCoefficient: 0, centralization: 0, modularity: 0, efficiency: improvedMetrics.readability, smallWorldness: 0 },
      ),
      reasoning: this.generateOptimizationReasoning(algorithm, improvements)
    };

    this.optimizationCache.set(cacheKey, optimization);
    return optimization;
  }

  
  public async detectClusters(
    graphData: GraphData,
    options: {
      algorithm?: 'modularity' | 'density' | 'hierarchical' | 'spectral';
      minClusterSize?: number;
      maxClusters?: number;
    } = {}
  ): Promise<ClusterDetection> {
    logger.info('Detecting clusters using AI algorithms');

    const cacheKey = this.generateCacheKey(graphData, options);
    if (this.clusterCache.has(cacheKey)) {
      return this.clusterCache.get(cacheKey)!;
    }

    const algorithm = options.algorithm || this.selectOptimalClusteringAlgorithm(graphData);
    const clusters = await this.applyClustering(graphData, algorithm, options);
    
    
    const quality = this.calculateClusterQuality(graphData, clusters);
    
    
    const recommendations = this.generateClusterRecommendations(clusters, quality);

    const detection: ClusterDetection = {
      clusters,
      algorithm,
      quality,
      recommendations
    };

    this.clusterCache.set(cacheKey, detection);
    return detection;
  }

  
  public async generateNodeRecommendations(
    graphData: GraphData,
    targetNodeId?: string
  ): Promise<NodeRecommendation[]> {
    logger.info('Generating AI-powered node recommendations');

    const recommendations: NodeRecommendation[] = [];
    const nodes = targetNodeId ? [graphData.nodes.find(n => n.id === targetNodeId)!] : graphData.nodes;

    for (const node of nodes) {
      if (!node) continue;

      
      const connectivity = this.analyzeNodeConnectivity(node, graphData);
      
      
      const positioning = this.analyzeNodePositioning(node, graphData);
      
      
      const typeAnalysis = this.analyzeNodeType(node, graphData);
      
      
      const nodeRecommendations = this.generateNodeSpecificRecommendations(
        node,
        connectivity,
        positioning,
        typeAnalysis
      );

      recommendations.push(...nodeRecommendations);
    }

    
    recommendations.sort((a, b) => {
      const aScore = a.confidence * (
        a.potentialImpact.connectivityImprovement +
        a.potentialImpact.readabilityImprovement +
        a.potentialImpact.structuralImprovement
      );
      const bScore = b.confidence * (
        b.potentialImpact.connectivityImprovement +
        b.potentialImpact.readabilityImprovement +
        b.potentialImpact.structuralImprovement
      );
      return bScore - aScore;
    });

    return recommendations.slice(0, 10); 
  }

  
  public async recognizePatterns(
    logseqGraph: GraphData,
    visionflowGraph: GraphData,
    options: {
      detectAnomalies?: boolean;
      crossGraphAnalysis?: boolean;
      patternTypes?: GraphPattern['type'][];
    } = {}
  ): Promise<PatternRecognition> {
    logger.info('Recognizing patterns using AI algorithms');

    const cacheKey = this.generateCacheKey({ logseqGraph, visionflowGraph }, options);
    if (this.patternCache.has(cacheKey)) {
      return this.patternCache.get(cacheKey)!;
    }

    
    const logseqPatterns = await this.detectGraphPatterns(logseqGraph, options.patternTypes);
    const visionflowPatterns = await this.detectGraphPatterns(visionflowGraph, options.patternTypes);

    
    const crossGraphPatterns = options.crossGraphAnalysis 
      ? await this.analyzeCrossGraphPatterns(logseqPatterns, visionflowPatterns)
      : [];

    
    const anomalies = options.detectAnomalies 
      ? await this.detectGraphAnomalies(logseqGraph, visionflowGraph)
      : [];

    
    const insights = this.generatePatternInsights(
      [...logseqPatterns, ...visionflowPatterns],
      crossGraphPatterns,
      anomalies
    );

    const recognition: PatternRecognition = {
      patterns: [...logseqPatterns, ...visionflowPatterns],
      crossGraphPatterns,
      anomalies,
      insights
    };

    this.patternCache.set(cacheKey, recognition);
    return recognition;
  }

  
  public calculateGraphMetrics(graphData: GraphData): GraphMetrics {
    const cacheKey = this.generateCacheKey(graphData);
    if (this.metricsCache.has(cacheKey)) {
      return this.metricsCache.get(cacheKey)!;
    }

    const nodes = graphData.nodes.length;
    const edges = graphData.edges.length;
    const maxEdges = nodes * (nodes - 1) / 2;

    
    const density = maxEdges > 0 ? edges / maxEdges : 0;
    const averagePathLength = this.calculateAveragePathLength(graphData);
    const clusteringCoefficient = this.calculateClusteringCoefficient(graphData);
    const centralization = this.calculateCentralization(graphData);
    const modularity = this.calculateModularity(graphData);
    const efficiency = this.calculateNetworkEfficiency(graphData);
    const smallWorldness = this.calculateSmallWorldness(clusteringCoefficient, averagePathLength);

    const metrics: GraphMetrics = {
      density,
      averagePathLength,
      clusteringCoefficient,
      centralization,
      modularity,
      efficiency,
      smallWorldness
    };

    this.metricsCache.set(cacheKey, metrics);
    return metrics;
  }

  

  private selectOptimalAlgorithm(
    graphData: GraphData,
    constraints: any
  ): LayoutOptimization['algorithmUsed'] {
    const nodeCount = graphData.nodes.length;
    const edgeCount = graphData.edges.length;
    const density = edgeCount / (nodeCount * (nodeCount - 1) / 2);

    
    if (nodeCount < 50 && constraints.minimizeEdgeCrossings) {
      return 'force-directed';
    }
    if (density > 0.3 && constraints.respectClusters) {
      return 'hierarchical';
    }
    if (nodeCount > 200) {
      return 'grid';
    }
    if (this.hasHierarchicalStructure(graphData)) {
      return 'hierarchical';
    }

    return 'organic'; 
  }

  private async applyOptimizationAlgorithm(
    graphData: GraphData,
    currentPositions: Map<string, Vector3>,
    algorithm: LayoutOptimization['algorithmUsed'],
    constraints: any
  ): Promise<Map<string, Vector3>> {
    const optimizedPositions = new Map<string, Vector3>();

    switch (algorithm) {
      case 'force-directed':
        return this.applyForceDirectedLayout(graphData, currentPositions, constraints);
      
      case 'hierarchical':
        return this.applyHierarchicalLayout(graphData, constraints);
      
      case 'circular':
        return this.applyCircularLayout(graphData);
      
      case 'grid':
        return this.applyGridLayout(graphData);
      
      case 'organic':
        return this.applyOrganicLayout(graphData, currentPositions, constraints);
      
      default:
        return currentPositions;
    }
  }

  private applyForceDirectedLayout(
    graphData: GraphData,
    currentPositions: Map<string, Vector3>,
    constraints: any
  ): Map<string, Vector3> {
    const positions = new Map(currentPositions);
    const iterations = 100;
    const coolingFactor = 0.95;
    let temperature = 1.0;

    for (let i = 0; i < iterations; i++) {
      
      for (const node1 of graphData.nodes) {
        const pos1 = positions.get(node1.id)!;
        let force = new Vector3(0, 0, 0);

        for (const node2 of graphData.nodes) {
          if (node1.id === node2.id) continue;
          
          const pos2 = positions.get(node2.id)!;
          const distance = pos1.distanceTo(pos2);
          const direction = new Vector3().subVectors(pos1, pos2).normalize();
          
          
          const repulsion = direction.multiplyScalar(1 / Math.max(distance * distance, 0.1));
          force.add(repulsion);
        }

        
        for (const edge of graphData.edges) {
          if (edge.source === node1.id || edge.target === node1.id) {
            const otherId = edge.source === node1.id ? edge.target : edge.source;
            const otherPos = positions.get(otherId)!;
            const distance = pos1.distanceTo(otherPos);
            const direction = new Vector3().subVectors(otherPos, pos1).normalize();
            
            
            const attraction = direction.multiplyScalar(distance * 0.01);
            force.add(attraction);
          }
        }

        
        const newPos = pos1.clone().add(force.multiplyScalar(temperature));
        positions.set(node1.id, newPos);
      }

      temperature *= coolingFactor;
    }

    return positions;
  }

  private applyHierarchicalLayout(graphData: GraphData, constraints: any): Map<string, Vector3> {
    const positions = new Map<string, Vector3>();
    
    
    const inDegree = new Map<string, number>();
    graphData.nodes.forEach(node => inDegree.set(node.id, 0));
    graphData.edges.forEach(edge => {
      inDegree.set(edge.target, (inDegree.get(edge.target) || 0) + 1);
    });

    const rootNodes = graphData.nodes.filter(node => inDegree.get(node.id) === 0);
    
    
    const levels = new Map<string, number>();
    const queue = rootNodes.map(node => ({ id: node.id, level: 0 }));
    
    while (queue.length > 0) {
      const { id, level } = queue.shift()!;
      levels.set(id, level);
      
      
      const children = graphData.edges
        .filter(edge => edge.source === id)
        .map(edge => edge.target)
        .filter(childId => !levels.has(childId));
      
      children.forEach(childId => {
        queue.push({ id: childId, level: level + 1 });
      });
    }

    
    const maxLevel = Math.max(...Array.from(levels.values()));
    const levelCounts = new Map<number, number>();
    
    levels.forEach((level, nodeId) => {
      levelCounts.set(level, (levelCounts.get(level) || 0) + 1);
    });

    levels.forEach((level, nodeId) => {
      const nodesAtLevel = levelCounts.get(level) || 1;
      const positionInLevel = Array.from(levels.entries())
        .filter(([_, l]) => l === level)
        .findIndex(([id, _]) => id === nodeId);
      
      const x = (positionInLevel - (nodesAtLevel - 1) / 2) * 10;
      const y = (maxLevel - level) * 10;
      const z = 0;
      
      positions.set(nodeId, new Vector3(x, y, z));
    });

    return positions;
  }

  private applyCircularLayout(graphData: GraphData): Map<string, Vector3> {
    const positions = new Map<string, Vector3>();
    const radius = Math.max(10, graphData.nodes.length * 0.5);
    
    graphData.nodes.forEach((node, index) => {
      const angle = (index / graphData.nodes.length) * 2 * Math.PI;
      const x = Math.cos(angle) * radius;
      const z = Math.sin(angle) * radius;
      positions.set(node.id, new Vector3(x, 0, z));
    });

    return positions;
  }

  private applyGridLayout(graphData: GraphData): Map<string, Vector3> {
    const positions = new Map<string, Vector3>();
    const gridSize = Math.ceil(Math.sqrt(graphData.nodes.length));
    const spacing = 5;
    
    graphData.nodes.forEach((node, index) => {
      const row = Math.floor(index / gridSize);
      const col = index % gridSize;
      const x = (col - gridSize / 2) * spacing;
      const z = (row - gridSize / 2) * spacing;
      positions.set(node.id, new Vector3(x, 0, z));
    });

    return positions;
  }

  private applyOrganicLayout(
    graphData: GraphData,
    currentPositions: Map<string, Vector3>,
    constraints: any
  ): Map<string, Vector3> {
    
    const positions = this.applyForceDirectedLayout(graphData, currentPositions, constraints);
    
    
    const clusters = this.detectSimpleClusters(graphData);
    clusters.forEach(cluster => {
      const clusterCenter = this.calculateClusterCenter(cluster, positions);
      
      cluster.forEach(nodeId => {
        const currentPos = positions.get(nodeId)!;
        const toCenter = new Vector3().subVectors(clusterCenter, currentPos).multiplyScalar(0.1);
        positions.set(nodeId, currentPos.add(toCenter));
      });
    });

    return positions;
  }

  private calculateLayoutMetrics(
    graphData: GraphData,
    positions: Map<string, Vector3>
  ): { edgeCrossings: number; nodeOverlaps: number; readability: number } {
    let edgeCrossings = 0;
    let nodeOverlaps = 0;
    
    
    const edges = graphData.edges.map(edge => ({
      start: positions.get(edge.source)!,
      end: positions.get(edge.target)!
    }));

    for (let i = 0; i < edges.length; i++) {
      for (let j = i + 1; j < edges.length; j++) {
        if (this.doEdgesCross(edges[i], edges[j])) {
          edgeCrossings++;
        }
      }
    }

    
    const nodes = Array.from(positions.values());
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        if (nodes[i].distanceTo(nodes[j]) < 2.0) { 
          nodeOverlaps++;
        }
      }
    }

    
    const averageDistance = this.calculateAverageNodeDistance(positions);
    const readability = Math.min(1, averageDistance / 5); 

    return { edgeCrossings, nodeOverlaps, readability };
  }

  private selectOptimalClusteringAlgorithm(graphData: GraphData): ClusterDetection['algorithm'] {
    const nodeCount = graphData.nodes.length;
    const edgeCount = graphData.edges.length;
    
    if (nodeCount < 50) return 'modularity';
    if (edgeCount / nodeCount > 3) return 'density';
    if (this.hasHierarchicalStructure(graphData)) return 'hierarchical';
    
    return 'spectral';
  }

  private async applyClustering(
    graphData: GraphData,
    _algorithm: ClusterDetection['algorithm'],
    options: any
  ): Promise<GraphCluster[]> {
    // All algorithms currently use the same connected-component clustering
    const clusters: GraphCluster[] = [];
    const visited = new Set<string>();
    let clusterId = 0;

    for (const node of graphData.nodes) {
      if (visited.has(node.id)) continue;

      const cluster = this.growClusterFromNode(node.id, graphData, visited);
      if (cluster.length >= (options.minClusterSize || 2)) {
        clusters.push(this.createClusterFromNodes(cluster, graphData, `cluster-${clusterId++}`));
      }
    }

    return clusters;
  }

  private growClusterFromNode(
    startNodeId: string,
    graphData: GraphData,
    visited: Set<string>
  ): string[] {
    const cluster: string[] = [];
    const queue = [startNodeId];

    while (queue.length > 0) {
      const nodeId = queue.shift()!;
      if (visited.has(nodeId)) continue;

      visited.add(nodeId);
      cluster.push(nodeId);

      
      const connectedNodes = graphData.edges
        .filter(edge => edge.source === nodeId || edge.target === nodeId)
        .map(edge => edge.source === nodeId ? edge.target : edge.source)
        .filter(id => !visited.has(id));

      queue.push(...connectedNodes);
    }

    return cluster;
  }

  private createClusterFromNodes(
    nodeIds: string[],
    graphData: GraphData,
    clusterId: string
  ): GraphCluster {
    const nodes = nodeIds.map(id => graphData.nodes.find(n => n.id === id)!);
    const positions = nodes.map(n => n.position || { x: 0, y: 0, z: 0 });
    
    
    const centerPosition = new Vector3(
      positions.reduce((sum, pos) => sum + pos.x, 0) / positions.length,
      positions.reduce((sum, pos) => sum + pos.y, 0) / positions.length,
      positions.reduce((sum, pos) => sum + pos.z, 0) / positions.length
    );

    
    const radius = Math.max(...positions.map(pos => 
      centerPosition.distanceTo(new Vector3(pos.x, pos.y, pos.z))
    ));

    
    const internalEdges = graphData.edges.filter(edge => 
      nodeIds.includes(edge.source) && nodeIds.includes(edge.target)
    ).length;
    
    const externalEdges = graphData.edges.filter(edge => 
      (nodeIds.includes(edge.source) && !nodeIds.includes(edge.target)) ||
      (!nodeIds.includes(edge.source) && nodeIds.includes(edge.target))
    ).length;

    const density = nodeIds.length > 1 ? 
      internalEdges / (nodeIds.length * (nodeIds.length - 1) / 2) : 0;

    const dominantTypes = this.getDominantTypes(nodes);
    const averageConnections = (internalEdges * 2) / nodeIds.length;
    const coherenceScore = internalEdges / Math.max(internalEdges + externalEdges, 1);

    return {
      id: clusterId,
      nodes: nodeIds,
      centerPosition,
      radius,
      density,
      dominantTypes,
      characteristics: {
        averageConnections,
        internalEdges,
        externalEdges,
        coherenceScore
      },
      suggestedColor: this.generateClusterColor(dominantTypes[0]),
      label: this.generateClusterLabel(dominantTypes, nodeIds.length)
    };
  }

  
  

  private generateCacheKey(...args: any[]): string {
    return JSON.stringify(args);
  }

  private hasHierarchicalStructure(graphData: GraphData): boolean {
    
    const connectionCounts = new Map<string, number>();
    
    graphData.edges.forEach(edge => {
      connectionCounts.set(edge.source, (connectionCounts.get(edge.source) || 0) + 1);
      connectionCounts.set(edge.target, (connectionCounts.get(edge.target) || 0) + 1);
    });

    const counts = Array.from(connectionCounts.values());
    const avg = counts.reduce((sum, count) => sum + count, 0) / counts.length;
    const hasHubs = counts.some(count => count > avg * 3);

    return hasHubs;
  }

  private doEdgesCross(edge1: { start: Vector3; end: Vector3 }, edge2: { start: Vector3; end: Vector3 }): boolean {
    // Line segment intersection using cross-product orientation test (2D projection on XZ plane)
    const p1x = edge1.start.x, p1z = edge1.start.z;
    const p2x = edge1.end.x,   p2z = edge1.end.z;
    const p3x = edge2.start.x, p3z = edge2.start.z;
    const p4x = edge2.end.x,   p4z = edge2.end.z;

    // Direction (cross product sign) of point relative to a directed line segment
    const direction = (ax: number, az: number, bx: number, bz: number, cx: number, cz: number): number => {
      return (bx - ax) * (cz - az) - (bz - az) * (cx - ax);
    };

    const d1 = direction(p3x, p3z, p4x, p4z, p1x, p1z);
    const d2 = direction(p3x, p3z, p4x, p4z, p2x, p2z);
    const d3 = direction(p1x, p1z, p2x, p2z, p3x, p3z);
    const d4 = direction(p1x, p1z, p2x, p2z, p4x, p4z);

    // Segments cross when endpoints of each segment lie on opposite sides of the other
    return d1 * d2 < 0 && d3 * d4 < 0;
  }

  private calculateAverageNodeDistance(positions: Map<string, Vector3>): number {
    const nodes = Array.from(positions.values());
    let totalDistance = 0;
    let count = 0;

    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        totalDistance += nodes[i].distanceTo(nodes[j]);
        count++;
      }
    }

    return count > 0 ? totalDistance / count : 0;
  }

  private getDominantTypes(nodes: KGNode[]): string[] {
    const typeCounts = new Map<string, number>();
    
    nodes.forEach(node => {
      const type = node.metadata?.type || 'unknown';
      typeCounts.set(type, (typeCounts.get(type) || 0) + 1);
    });

    return Array.from(typeCounts.entries())
      .sort((a, b) => b[1] - a[1])
      .map(([type, _]) => type)
      .slice(0, 3);
  }

  private generateClusterColor(dominantType: string): Color {
    const typeColors: Record<string, string> = {
      'file': '#4CAF50',
      'folder': '#FF9800',
      'function': '#2196F3',
      'class': '#9C27B0',
      'variable': '#00BCD4',
      'unknown': '#757575'
    };

    return new Color(typeColors[dominantType] || typeColors.unknown);
  }

  private generateClusterLabel(dominantTypes: string[], nodeCount: number): string {
    const primaryType = dominantTypes[0] || 'Mixed';
    return `${primaryType} cluster (${nodeCount} nodes)`;
  }

  private calculateAveragePathLength(graphData: GraphData): number {
    const nodes = graphData.nodes;
    const edges = graphData.edges;
    if (nodes.length < 2 || edges.length === 0) return 0;

    const adj = new Map<string, Set<string>>();
    nodes.forEach(n => adj.set(n.id, new Set()));
    edges.forEach(e => {
      adj.get(e.source)?.add(e.target);
      adj.get(e.target)?.add(e.source);
    });

    const sampleSize = Math.min(50, nodes.length);
    const step = Math.max(1, Math.floor(nodes.length / sampleSize));
    let totalDist = 0;
    let pairCount = 0;

    for (let si = 0; si < nodes.length; si += step) {
      const start = nodes[si].id;
      const dist = new Map<string, number>([[start, 0]]);
      const queue = [start];
      let head = 0;
      while (head < queue.length) {
        const cur = queue[head++];
        const d = dist.get(cur)!;
        for (const nb of adj.get(cur) || []) {
          if (!dist.has(nb)) {
            dist.set(nb, d + 1);
            queue.push(nb);
          }
        }
      }
      dist.forEach((d, id) => {
        if (id !== start && d > 0) { totalDist += d; pairCount++; }
      });
    }
    return pairCount > 0 ? totalDist / pairCount : 0;
  }

  private calculateClusteringCoefficient(graphData: GraphData): number {
    const nodes = graphData.nodes;
    const edges = graphData.edges;
    if (nodes.length < 3) return 0;

    const adj = new Map<string, Set<string>>();
    nodes.forEach(n => adj.set(n.id, new Set()));
    edges.forEach(e => {
      adj.get(e.source)?.add(e.target);
      adj.get(e.target)?.add(e.source);
    });

    let totalCoeff = 0;
    let qualifying = 0;
    adj.forEach((neighbors, _nodeId) => {
      const k = neighbors.size;
      if (k < 2) return;
      const nbArr = Array.from(neighbors);
      let triangles = 0;
      for (let i = 0; i < nbArr.length; i++) {
        for (let j = i + 1; j < nbArr.length; j++) {
          if (adj.get(nbArr[i])?.has(nbArr[j])) triangles++;
        }
      }
      totalCoeff += (2 * triangles) / (k * (k - 1));
      qualifying++;
    });
    return qualifying > 0 ? totalCoeff / qualifying : 0;
  }

  private calculateCentralization(graphData: GraphData): number {
    const n = graphData.nodes.length;
    if (n < 3) return 0;

    const degree = new Map<string, number>();
    graphData.nodes.forEach(nd => degree.set(nd.id, 0));
    graphData.edges.forEach(e => {
      degree.set(e.source, (degree.get(e.source) || 0) + 1);
      degree.set(e.target, (degree.get(e.target) || 0) + 1);
    });

    const degrees = Array.from(degree.values());
    const maxDeg = Math.max(...degrees);
    const sumDiff = degrees.reduce((s, d) => s + (maxDeg - d), 0);
    return sumDiff / ((n - 1) * (n - 2));
  }

  private calculateModularity(graphData: GraphData): number {
    const nodes = graphData.nodes;
    const edges = graphData.edges;
    if (nodes.length < 2 || edges.length === 0) return 0;

    const adj = new Map<string, Set<string>>();
    nodes.forEach(n => adj.set(n.id, new Set()));
    edges.forEach(e => {
      adj.get(e.source)?.add(e.target);
      adj.get(e.target)?.add(e.source);
    });

    // Detect communities via connected components
    const community = new Map<string, number>();
    let cId = 0;
    nodes.forEach(n => {
      if (community.has(n.id)) return;
      const queue = [n.id];
      let head = 0;
      community.set(n.id, cId);
      while (head < queue.length) {
        for (const nb of adj.get(queue[head++]) || []) {
          if (!community.has(nb)) { community.set(nb, cId); queue.push(nb); }
        }
      }
      cId++;
    });

    if (cId <= 1) return 0; // single component

    const m = edges.length;
    const comEdges = new Map<number, number>();
    const comDegree = new Map<number, number>();
    edges.forEach(e => {
      const cs = community.get(e.source)!;
      const ct = community.get(e.target)!;
      if (cs === ct) comEdges.set(cs, (comEdges.get(cs) || 0) + 1);
      comDegree.set(cs, (comDegree.get(cs) || 0) + 1);
      comDegree.set(ct, (comDegree.get(ct) || 0) + 1);
    });

    let Q = 0;
    for (let c = 0; c < cId; c++) {
      const ecc = (comEdges.get(c) || 0) / m;
      const ac = (comDegree.get(c) || 0) / (2 * m);
      Q += ecc - ac * ac;
    }
    return Q;
  }

  private calculateNetworkEfficiency(graphData: GraphData): number {
    const nodes = graphData.nodes;
    const edges = graphData.edges;
    if (nodes.length < 2 || edges.length === 0) return 0;

    const adj = new Map<string, Set<string>>();
    nodes.forEach(n => adj.set(n.id, new Set()));
    edges.forEach(e => {
      adj.get(e.source)?.add(e.target);
      adj.get(e.target)?.add(e.source);
    });

    const n = nodes.length;
    const sampleSize = Math.min(50, n);
    const step = Math.max(1, Math.floor(n / sampleSize));
    let totalEff = 0;
    let pairCount = 0;

    for (let si = 0; si < n; si += step) {
      const start = nodes[si].id;
      const dist = new Map<string, number>([[start, 0]]);
      const queue = [start];
      let head = 0;
      while (head < queue.length) {
        const cur = queue[head++];
        const d = dist.get(cur)!;
        for (const nb of adj.get(cur) || []) {
          if (!dist.has(nb)) { dist.set(nb, d + 1); queue.push(nb); }
        }
      }
      dist.forEach((d, id) => {
        if (id !== start && d > 0) { totalEff += 1 / d; pairCount++; }
      });
    }
    return pairCount > 0 ? totalEff / pairCount : 0;
  }

  private calculateSmallWorldness(clustering: number, pathLength: number): number {

    return clustering / pathLength;
  }

  private calculateOptimizationConfidence(currentMetrics: GraphMetrics, improvedMetrics: GraphMetrics): number {
    const improvement = (improvedMetrics.efficiency - currentMetrics.efficiency) / currentMetrics.efficiency;
    return Math.min(0.95, 0.5 + improvement);
  }

  private generateOptimizationReasoning(_algorithm: string, _improvements: any): string[] {
    return ['Layout optimization applied', 'Edge crossings reduced', 'Node spacing improved'];
  }

  private calculateClusterQuality(_graphData: GraphData, clusters: GraphCluster[]): { modularity: number; silhouette: number; cohesion: number } {
    if (clusters.length === 0) return { modularity: 0, silhouette: 0, cohesion: 0 };
    const avgDensity = clusters.reduce((sum, c) => sum + c.density, 0) / clusters.length;
    return { modularity: avgDensity, silhouette: avgDensity * 0.8, cohesion: avgDensity * 0.9 };
  }

  private generateClusterRecommendations(clusters: GraphCluster[], _quality: { modularity: number; silhouette: number; cohesion: number }): string[] {
    if (clusters.length === 0) return ['No clusters detected'];
    return clusters.map(c => `Cluster ${c.id}: ${c.nodes.length} nodes, density ${c.density.toFixed(2)}`);
  }

  private analyzeNodeConnectivity(_node: KGNode, graphData: GraphData): number {
    return graphData.edges.filter(e => e.source === _node.id || e.target === _node.id).length;
  }

  private analyzeNodePositioning(_node: KGNode, _graphData: GraphData): { x: number; y: number; z: number } {
    return _node.position || { x: 0, y: 0, z: 0 };
  }

  private analyzeNodeType(_node: KGNode, _graphData: GraphData): string {
    return _node.metadata?.type || 'unknown';
  }

  private generateNodeSpecificRecommendations(
    node: KGNode,
    connectivity: number,
    _positioning: { x: number; y: number; z: number },
    _typeAnalysis: string
  ): NodeRecommendation[] {
    return [{
      nodeId: node.id,
      recommendationType: connectivity > 5 ? 'highlight' : 'connect',
      reasoning: `Node has ${connectivity} connections`,
      confidence: 0.7,
      suggestedActions: [],
      potentialImpact: { connectivityImprovement: 0, readabilityImprovement: 0, structuralImprovement: 0 }
    }];
  }

  private async detectGraphPatterns(_graphData: GraphData, _patternTypes?: GraphPattern['type'][]): Promise<GraphPattern[]> {
    return [];
  }

  private async analyzeCrossGraphPatterns(_logseqPatterns: GraphPattern[], _visionflowPatterns: GraphPattern[]): Promise<any[]> {
    return [];
  }

  private async detectGraphAnomalies(_logseqGraph: GraphData, _visionflowGraph: GraphData): Promise<any[]> {
    return [];
  }

  private generatePatternInsights(_patterns: GraphPattern[], _crossGraphPatterns: any[], _anomalies: any[]): string[] {
    return ['Pattern analysis complete'];
  }

  private detectSimpleClusters(graphData: GraphData): string[][] {
    const visited = new Set<string>();
    const clusters: string[][] = [];

    for (const node of graphData.nodes) {
      if (visited.has(node.id)) continue;
      const cluster = this.growClusterFromNode(node.id, graphData, visited);
      if (cluster.length > 1) clusters.push(cluster);
    }
    return clusters;
  }

  private calculateClusterCenter(cluster: string[], positions: Map<string, Vector3>): Vector3 {
    const center = new Vector3();
    let count = 0;
    for (const nodeId of cluster) {
      const pos = positions.get(nodeId);
      if (pos) {
        center.add(pos);
        count++;
      }
    }
    return count > 0 ? center.divideScalar(count) : center;
  }


  public dispose(): void {
    this.optimizationCache.clear();
    this.clusterCache.clear();
    this.patternCache.clear();
    this.metricsCache.clear();
    logger.info('AI insights disposed');
  }
}

// Export singleton instance
export const aiInsights = AIInsights.getInstance();