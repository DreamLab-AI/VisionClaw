# Control Center Panel

> Captured from research agent. Source files cited inline.

## Summary
- 10 tabs (6 basic + 3 advanced + 1 advanced-power-user-only)
- 180+ leaf controls
- Single source of truth: `client/src/features/visualisation/components/ControlPanel/unifiedSettingsConfig.ts` (496 lines)
- Two interaction modes: interactive (full panel) and collapsed (CommandInput "AI control" textbox)
- Sync via `autoSaveManager` (500 ms debounce) → NIP-98-signed PATCH `/api/settings`

## Component Tree (interactive mode)
| File:Line | Component | Children | Mode Visibility |
|---|---|---|---|
| `IntegratedControlPanel.tsx:410-418` | IntegratedControlPanel | Root wrapper | Always |
| `IntegratedControlPanel.tsx:34-407` | IntegratedControlPanelInner | inside ControlPanelProvider | Always |
| `ControlPanel/ControlPanelHeader.tsx` | ControlPanelHeader | chrome | Always |
| `ControlPanel/AdvancedModeToggle.tsx` | AdvancedModeToggle | header/sidebar | Always |
| `ControlPanel/TabNavigation.tsx` | TabNavigation | tab bar | filtered by `advancedMode + isPowerUser` |
| `ControlPanel/UnifiedSettingsTabContent.tsx` | UnifiedSettingsTabContent | TabsContent[6 tabs] | filtered by `section.isAdvanced + section.isPowerUserOnly` |
| `ControlPanel/BotsStatusPanel.tsx` | BotsStatusPanel | chrome | Always |
| `ControlPanel/SystemHealthIndicator.tsx` | SystemHealthIndicator | header | Always |
| `ControlPanel/SpacePilotStatus.tsx` | SpacePilotStatus | chrome | Always |
| `ControlPanel/SystemInfo.tsx` | SystemInfo | chrome | Always |
| `visualisation/CommandInput.tsx` | CommandInput | collapsed panel | when `isExpanded=false` |

## AI text-entry subsurface (`CommandInput.tsx`)
| File:Line | Element | Wire Path | Server Handler | Capabilities |
|---|---|---|---|---|
| `CommandInput.tsx:59-160` | parseCommandToActions() | mostly local Zustand; some PUT `/api/settings/physics` | physics_handler | view/graph configuration only |
| `CommandInput.tsx:17-49` | validateCommand() | none | n/a | blocks `exec/eval/fetch/sudo`; allows `cluster/hull/show/hide/zoom/repel/spring/damp/knowledge/ontology/agent` |
| `CommandInput.tsx:64-88` | hull cluster toggle | local Zustand → `visualisation.clusterHulls.enabled` | n/a | local |
| `CommandInput.tsx:92-101` | repulsion adjust | PUT `/api/settings/physics` `{repelK: 400|100}` | physics_handler | "more/less repel" |
| `CommandInput.tsx:104-113` | spring strength | PUT `/api/settings/physics` `{springK: 5.0|1.0}` | physics_handler | "tighter/looser" |
| `CommandInput.tsx:116-124` | damping | PUT `/api/settings/physics` `{damping: 0.8|0.3}` | physics_handler | |
| `CommandInput.tsx:127-148` | node-type visibility | local Zustand + `autoSaveManager.queueChange()` | settings batch | knowledge/ontology/agent show/hide |

**Disconnect:** keyword-only parser. `src/handlers/natural_language_query_handler.rs` (`/api/nl-query/translate`, `/api/nl-query/explain`, `/api/nl-query/validate`, `/api/nl-query/examples`) exists and is unused by the UI.

## Settings inventory (every leaf in `unifiedSettingsConfig.ts`)

### GRAPH tab (lines 118-180)
| Key | Label | Type | Path | Min/Max | Adv | PU |
|---|---|---|---|---|---|---|
| nodeColor | Node Color | color | `visualisation.graphs.logseq.nodes.baseColor` | – | – | – |
| nodeSize | Node Size | slider | `…nodes.nodeSize` | 0.2-2 | – | – |
| nodeOpacity | Node Opacity | slider | `…nodes.opacity` | 0-1 | – | – |
| enableInstancing | GPU Instancing | toggle | `…nodes.enableInstancing` | – | – | – |
| nodeMetalness | Metalness | slider | `…nodes.metalness` | 0-1 | A | – |
| nodeRoughness | Roughness | slider | `…nodes.roughness` | 0-1 | A | – |
| enableMetadataShape | Metadata Shape | toggle | `…nodes.enableMetadataShape` | – | A | – |
| enableMetadataVis | Metadata Visual | toggle | `…nodes.enableMetadataVisualisation` | – | A | – |
| nodeImportance | Show Importance | toggle | `…nodes.enableImportance` | – | A | – |
| showKnowledge | Knowledge Nodes | toggle | `…nodes.nodeTypeVisibility.knowledge` | – | – | – |
| showOntology | Ontology Nodes | toggle | `…nodes.nodeTypeVisibility.ontology` | – | – | – |
| showAgents | Agent Nodes | toggle | `…nodes.nodeTypeVisibility.agent` | – | – | – |
| edgeColor | Edge Color | color | `…edges.color` | – | – | – |
| edgeWidth | Edge Width | slider | `…edges.baseWidth` | 0.01-2 | – | – |
| edgeOpacity | Edge Opacity | slider | `…edges.opacity` | 0-1 | – | – |
| enableArrows | Show Arrows | toggle | `…edges.enableArrows` | – | – | – |
| arrowSize | Arrow Size | slider | `…edges.arrowSize` | 0.01-0.5 | A | – |
| useGradient | Edge Gradient | toggle | `…edges.useGradient` | – | A | – |
| distanceIntensity | Distance Intensity | slider | `…edges.distanceIntensity` | 0-10 | A | – |
| kgEdgeColor | KG Edge Color | color | `visualisation.graphTypeVisuals.knowledgeGraph.edgeColor` | – | – | – |
| ontologyEdgeColor | Ontology Edge Color | color | `visualisation.graphTypeVisuals.ontology.edgeColor` | – | A | – |
| enableLabels | Show Labels | toggle | `…labels.enableLabels` | – | – | – |
| labelSize | Label Size | slider | `…labels.desktopFontSize` | 0.05-3.0 | – | – |
| labelColor | Label Color | color | `…labels.textColor` | – | – | – |
| showMetadata | Show Metadata | toggle | `…labels.showMetadata` | – | – | – |
| labelStandoff | Label Standoff | slider | `…labels.textPadding` | -1.0-3.0 | – | – |
| labelOutlineColor | Outline Color | color | `…labels.textOutlineColor` | – | A | – |
| labelOutlineWidth | Outline Width | slider | `…labels.textOutlineWidth` | 0-0.01 | A | – |
| labelDistanceThreshold | Label Draw Distance | slider | `…labels.labelDistanceThreshold` | 50-2000 | A | – |
| maxLabelWidth | Max Label Width | slider | `…labels.maxLabelWidth` | 2-20 | A | – |
| ambientLight | Ambient Light | slider | `visualisation.rendering.ambientLightIntensity` | 0-2 | – | – |
| directionalLight | Direct Light | slider | `…rendering.directionalLightIntensity` | 0-2 | – | – |
| antialiasing | Antialiasing | toggle | `…rendering.enableAntialiasing` | – | A | – |
| shadows | Shadows | toggle | `…rendering.enableShadows` | – | A | – |
| ambientOcclusion | Ambient Occlusion | toggle | `…rendering.enableAmbientOcclusion` | – | A | – |
| selectionHighlightColor | Selection Color | color | `visualisation.interaction.selectionHighlightColor` | – | – | – |
| selectionEdgeFlow | Selection Flow | toggle | `…interaction.selectionEdgeFlow` | – | – | – |
| selectionEdgeFlowSpeed | Selection Flow Speed | slider | `…interaction.selectionEdgeFlowSpeed` | 0.5-5 | A | – |
| selectionEdgeWidth | Selection Edge Width | slider | `…interaction.selectionEdgeWidth` | 0.1-2 | A | – |
| selectionEdgeOpacity | Selection Opacity | slider | `…interaction.selectionEdgeOpacity` | 0.3-1 | A | – |

### PHYSICS tab (lines 185-250)
| Key | Label | Type | Path | Min/Max | Adv | PU | Notes |
|---|---|---|---|---|---|---|---|
| enabled | Physics Enabled | toggle | `…physics.enabled` | – | – | – | |
| autoBalance | Auto Balance | toggle | `…physics.autoBalance` | – | – | – | |
| damping | Damping | slider | `…physics.damping` | 0-1 | – | – | |
| springK | Spring Strength | slider | `…physics.springK` | 0.1-100 | – | – | rec 8-20 for 2K+ nodes |
| repelK | Repulsion | slider | `…physics.repelK` | 0-3000 | – | – | rec 800-1500 |
| attractionK | Attraction | slider | `…physics.attractionK` | 0-10 | – | – | |
| layoutMode | Layout Mode | select | `qualityGates.layoutMode` | force-directed/dag-topdown/dag-radial/dag-leftright/type-clustering | – | – | calls `layoutApi.setMode()` |
| layoutAlgorithm | Layout Algorithm | select | `…physics.layoutAlgorithm` | forceDirected/hierarchical/radial/spectral/temporal/clustered | – | – | **DUPLICATE intent** |
| graphSeparationX | Dual Graph Separation | slider | `…physics.graphSeparationX` | 0-500 | – | – | KG/onto plane separation |
| zDamping | Flatten to Planes | slider | `…physics.zDamping` | 0-0.1 | – | – | 3D→2D flatten |
| ontologyPhysics | Ontology Forces | toggle | `qualityGates.ontologyPhysics` | – | – | – | OWL constraint forces |
| ontologyStrength | Ontology Strength | slider | `qualityGates.ontologyStrength` | 0-1 | A | – | |
| semanticForces | Semantic Layout Forces | toggle | `qualityGates.semanticForces` | – | – | – | DAG + type clustering |
| dagLevelAttraction | DAG Level Attraction | slider | `qualityGates.dagLevelAttraction` | 0-20 | A | – | |
| dagSiblingRepulsion | DAG Sibling Repulsion | slider | `qualityGates.dagSiblingRepulsion` | 0-20 | A | – | |
| typeClusterAttraction | Type Cluster Attraction | slider | `qualityGates.typeClusterAttraction` | 0-20 | A | – | |
| typeClusterRadius | Type Cluster Radius | slider | `qualityGates.typeClusterRadius` | 10-5000 | A | – | |
| maxVelocity | Max Velocity | slider | `…physics.maxVelocity` | 0.1-500 | – | – | |
| enableBounds | Enable Bounds | toggle | `…physics.enableBounds` | – | – | – | |
| boundsSize | Bounds Size | slider | `…physics.boundsSize` | 100-100000 | – | – | |
| dt | Time Step | slider | `…physics.dt` | 0.001-0.1 | A | – | |
| separationRadius | Separation Radius | slider | `…physics.separationRadius` | 0.01-200 | A | – | |
| iterations | Iterations | slider | `…physics.iterations` | 1-5000 | A | – | solver iters/frame |
| warmupIterations | Warmup Iterations | slider | `…physics.warmupIterations` | 0-500 | A | – | |
| coolingRate | Cooling Rate | slider | `…physics.coolingRate` | 0.00001-0.01 | A | – | |
| minDistance | Min Distance | slider | `…physics.minDistance` | 0.05-20 | A | – | |
| maxRepulsionDist | Max Repulsion Dist | slider | `…physics.maxRepulsionDist` | 10-2000 | A | – | |
| restLength | Node Spacing | slider | `…physics.restLength` | 1-200 | – | – | spring rest length |
| repulsionCutoff | Repulsion Cutoff | slider | `…physics.repulsionCutoff` | 1-2000 | A | – | |
| centerGravityK | Cluster Tightness | slider | `…physics.centerGravityK` | 0-10 | – | – | |
| gridCellSize | Grid Cell Size | slider | `…physics.gridCellSize` | 1-2000 | A | – | spatial grid broad-phase |
| repulsionSofteningEpsilon | Repulsion Epsilon | slider | `…physics.repulsionSofteningEpsilon` | 0.00001-0.01 | A | – | |
| boundaryExtremeMultiplier | Boundary Extreme Mult | slider | `…physics.boundaryExtremeMultiplier` | 1-5 | A | – | |
| boundaryExtremeForceMultiplier | Boundary Force Mult | slider | `…physics.boundaryExtremeForceMultiplier` | 1-20 | A | – | |
| boundaryVelocityDamping | Boundary Vel Damping | slider | `…physics.boundaryVelocityDamping` | 0-1 | A | – | |
| boundaryDamping | Boundary Damping | slider | `…physics.boundaryDamping` | 0-1 | A | – | |
| updateThreshold | Update Threshold | slider | `…physics.updateThreshold` | 0-0.5 | A | – | |
| maxForce | Max Force | slider | `…physics.maxForce` | 1-1000 | A | – | |
| temperature | Temperature | slider | `…physics.temperature` | 0.001-100 | A | – | |
| massScale | Mass Scale | slider | `…physics.massScale` | 0.001-100 | A | – | |
| tweeningEnabled | Smooth Node Movement | toggle | `…tweening.enabled` | – | – | – | client-side lerp |
| tweeningLerpBase | Node Animation Speed | slider | `…tweening.lerpBase` | 0.0001-0.15 | – | – | |
| tweeningMaxDivergence | Maximum Node Jump | slider | `…tweening.maxDivergence` | 1-100 | – | – | snap threshold |
| tweeningSnapThreshold | Snap Distance | slider | `…tweening.snapThreshold` | 0.01-1.0 | A | – | |

### EFFECTS tab (lines 256-320)
| Key | Label | Type | Path | Min/Max | Adv | PU | Notes |
|---|---|---|---|---|---|---|---|
| webgpuRenderer | WebGPU Renderer | action-button | – | – | – | – | `action='toggle-webgpu'`; reloads page |
| rendererInfo | Renderer Info | readonly | `rendererCapabilities` | – | – | – | runtime-only, no schema |
| sceneEffectsEnabled | Scene Effects | toggle | `visualisation.sceneEffects.enabled` | – | – | – | WASM ambient |
| particleCount | Particle Count | slider | `…sceneEffects.particleCount` | 64-512 | – | – | |
| particleOpacity | Particle Opacity | slider | `…sceneEffects.particleOpacity` | 0-1 | – | – | |
| particleDrift | Particle Drift | slider | `…sceneEffects.particleDrift` | 0-2 | A | – | |
| wispsEnabled | Energy Wisps | toggle | `…sceneEffects.wispsEnabled` | – | – | – | |
| wispCount | Wisp Count | slider | `…sceneEffects.wispCount` | 8-128 | – | – | |
| wispOpacity | Wisp Opacity | slider | `…sceneEffects.wispOpacity` | 0-1 | – | – | |
| wispDriftSpeed | Wisp Speed | slider | `…sceneEffects.wispDriftSpeed` | 0-3 | A | – | |
| fogEnabled | Atmosphere | toggle | `…sceneEffects.fogEnabled` | – | – | – | nebula bg |
| fogOpacity | Atmosphere Opacity | slider | `…sceneEffects.fogOpacity` | 0-0.15 | – | – | |
| atmosphereResolution | Atmosphere Detail | slider | `…sceneEffects.atmosphereResolution` | 64-256 | A | – | |
| glow | Bloom Glow | toggle | `visualisation.glow.enabled` | – | – | – | post-process |
| glowIntensity | Glow Intensity | slider | `visualisation.glow.intensity` | 0-1.5 | – | – | |
| glowRadius | Glow Radius | slider | `visualisation.glow.radius` | 0-1.0 | – | – | |
| glowThreshold | Glow Threshold | slider | `visualisation.glow.threshold` | 0-1 | A | – | |
| ringCount | Ring Count | slider | `visualisation.hologram.ringCount` | 0-10 | – | – | |
| ringColor | Ring Color | color | `visualisation.hologram.ringColor` | – | – | – | |
| ringOpacity | Ring Opacity | slider | `visualisation.hologram.ringOpacity` | 0-1 | – | – | |
| ringRotationSpeed | Ring Speed | slider | `visualisation.hologram.ringRotationSpeed` | 0-5 | A | – | |
| gemIor | Gem IOR | slider | `visualisation.gemMaterial.ior` | 1.0-3.0 | A | – | |
| gemTransmission | Gem Transmission | slider | `visualisation.gemMaterial.transmission` | 0-1 | A | – | |
| gemClearcoat | Gem Clearcoat | slider | `visualisation.gemMaterial.clearcoat` | 0-1 | A | – | |
| gemClearcoatRoughness | Clearcoat Rough | slider | `visualisation.gemMaterial.clearcoatRoughness` | 0-0.5 | A | – | |
| gemEmissiveIntensity | Gem Emissive | slider | `visualisation.gemMaterial.emissiveIntensity` | 0-2 | A | – | |
| gemIridescence | Gem Iridescence | slider | `visualisation.gemMaterial.iridescence` | 0-1 | A | – | rainbow sheen |
| embeddingCloudEnabled | Embedding Cloud | toggle | `visualisation.embeddingCloud.enabled` | – | – | – | RuVector point cloud |
| embeddingCloudScale | Cloud Scale | slider | `…embeddingCloud.cloudScale` | 0.5-20 | – | – | |
| embeddingPointSize | Point Size | slider | `…embeddingCloud.pointSize` | 0.5-25 | – | – | |
| embeddingOpacity | Cloud Opacity | slider | `…embeddingCloud.opacity` | 0-1 | – | – | |
| embeddingRotation | Rotation Speed | slider | `…embeddingCloud.rotationSpeed` | 0-0.005 | A | – | |
| flowEffect | Edge Flow | toggle | `…edges.enableFlowEffect` | – | – | – | animated edge flow |
| flowSpeed | Flow Speed | slider | `…edges.flowSpeed` | 0.1-5 | A | – | |
| flowIntensity | Flow Intensity | slider | `…edges.flowIntensity` | 0-10 | A | – | |
| glowStrength | Edge Glow | slider | `…edges.glowStrength` | 0-5 | – | – | |
| nodeAnimations | Node Animations | toggle | `visualisation.animations.enableNodeAnimations` | – | – | – | |
| pulseEnabled | Pulse Effect | toggle | `…animations.pulseEnabled` | – | – | – | |
| pulseSpeed | Pulse Speed | slider | `…animations.pulseSpeed` | 0.1-2 | A | – | |
| pulseStrength | Pulse Strength | slider | `…animations.pulseStrength` | 0.1-2 | A | – | |
| selectionWave | Selection Wave | toggle | `…animations.selectionWaveEnabled` | – | A | – | |
| waveSpeed | Wave Speed | slider | `…animations.waveSpeed` | 0.1-2 | A | – | |

### ANALYTICS tab (lines 326-349)
| Key | Label | Type | Path | Min/Max | Adv | PU | Notes |
|---|---|---|---|---|---|---|---|
| enableMetrics | Enable Metrics | toggle | `analytics.enableMetrics` | – | – | – | |
| updateInterval | Update Interval | slider | `analytics.updateInterval` | 1-60 s | – | – | |
| showDegreeDistribution | Degree Distribution | toggle | `analytics.showDegreeDistribution` | – | A | – | |
| showClustering | Clustering Coefficient | toggle | `analytics.showClusteringCoefficient` | – | A | – | |
| showCentrality | Centrality Metrics | toggle | `analytics.showCentrality` | – | A | – | |
| clusteringAlgorithm | Clustering Algorithm | select | `analytics.clustering.algorithm` | none/kmeans/spectral/louvain/dbscan | A | – | |
| clusterCount | Cluster Count | slider | `analytics.clustering.clusterCount` | 2-20 | A | – | |
| clusterResolution | Resolution | slider | `analytics.clustering.resolution` | 0.1-2 | A | – | |
| clusterIterations | Cluster Iterations | slider | `analytics.clustering.iterations` | 10-100 | A | – | |
| exportClusters | Export Clusters | toggle | `analytics.clustering.exportEnabled` | – | A | PU | |
| importDistances | Import Distances | toggle | `analytics.clustering.importEnabled` | – | A | PU | |
| clusterHullsEnabled | Cluster Hulls | toggle | `visualisation.clusterHulls.enabled` | – | – | – | |
| clusterHullsOpacity | Hull Opacity | slider | `visualisation.clusterHulls.opacity` | 0.01-0.3 | – | – | |
| clusterHullsPadding | Hull Padding | slider | `visualisation.clusterHulls.padding` | 0-0.5 | A | – | |

### QUALITY tab (lines 354-389)
| Key | Label | Type | Path | Min/Max | Adv | PU | Notes |
|---|---|---|---|---|---|---|---|
| filterEnabled | Enable Filtering | toggle | `nodeFilter.enabled` | – | – | – | |
| filterByQuality | Filter by Quality | toggle | `nodeFilter.filterByQuality` | – | – | – | |
| qualityThreshold | Quality Threshold | slider | `nodeFilter.qualityThreshold` | 0-1 | – | – | |
| filterByAuthority | Filter by Authority | toggle | `nodeFilter.filterByAuthority` | – | – | – | |
| authorityThreshold | Authority Threshold | slider | `nodeFilter.authorityThreshold` | 0-1 | – | – | |
| filterMode | Filter Mode | select | `nodeFilter.filterMode` | or/and | A | – | |
| refreshGraph | Refresh Graph | action-button | – | – | – | – | `webSocketService.forceRefreshFilter()` |
| gpuAcceleration | GPU Acceleration | toggle | `qualityGates.gpuAcceleration` | – | – | – | |
| autoAdjust | Auto-Adjust Quality | toggle | `qualityGates.autoAdjust` | – | – | – | |
| minFpsThreshold | Min FPS Threshold | slider | `qualityGates.minFpsThreshold` | 15-60 | – | – | |
| maxNodeCount | Max Node Count | slider | `qualityGates.maxNodeCount` | 1000-500000 | – | – | |
| showClusters | Show Clusters | toggle | `qualityGates.showClusters` | – | – | – | colour-coded |
| showAnomalies | Show Anomalies | toggle | `qualityGates.showAnomalies` | – | – | – | highlight outliers |
| showCommunities | Show Communities | toggle | `qualityGates.showCommunities` | – | A | – | Louvain |
| gnnPhysics | GNN-Enhanced Physics | toggle | `qualityGates.gnnPhysics` | – | A | PU | |
| ruvectorEnabled | RuVector Integration | toggle | `qualityGates.ruvectorEnabled` | – | A | PU | HNSW |
| lodEnabled | LOD Enabled | toggle | `constraints.lodEnabled` | – | A | – | level-of-detail |
| farThreshold | Far Threshold | slider | `constraints.farThreshold` | 100-2000 | A | – | |
| mediumThreshold | Medium Threshold | slider | `constraints.mediumThreshold` | 50-500 | A | – | |
| nearThreshold | Near Threshold | slider | `constraints.nearThreshold` | 5-100 | A | – | |
| progressiveActivation | Progressive Activation | toggle | `constraints.progressiveActivation` | – | A | – | |
| activationFrames | Activation Frames | slider | `constraints.activationFrames` | 1-600 | A | – | |

### SYSTEM tab (lines 395-410)
| Key | Label | Type | Path | Adv | PU | Notes |
|---|---|---|---|---|---|---|
| nostr | Nostr Login | nostr-button | `auth.nostr` | – | – | calls `nostrAuth.login/logout()` |
| authEnabled | Auth Enabled | toggle | `auth.enabled` | A | – | |
| authRequired | Auth Required | toggle | `auth.required` | A | PU | |
| persistSettings | Persist Settings | toggle | `system.persistSettings` | – | – | save to server |
| customBackendURL | Custom Backend URL | text | `system.customBackendUrl` | A | PU | |
| enableDebug | Debug Mode | toggle | `system.debug.enabled` | A | – | **DUPLICATE of Developer.enableDebug** |

### XR tab (lines 415-432) — ADVANCED ONLY
| Key | Label | Type | Path | Min/Max | Notes |
|---|---|---|---|---|---|
| xrEnabled | XR Mode | toggle | `xr.enabled` | – | VR/AR |
| xrQuality | XR Quality | select | `xr.quality` | Low/Medium/High | |
| xrRenderScale | XR Render Scale | slider | `xr.renderScale` | 0.5-2 | |
| handTracking | Hand Tracking | toggle | `xr.enableHandTracking` | – | |
| enableHaptics | Haptics | toggle | `xr.enableHaptics` | – | |
| xrComputeMode | XR Compute Mode | toggle | `xr.gpu.enableOptimizedCompute` | – | (within-tab advanced) |
| xrPerformancePreset | XR Performance | select | `xr.performance.preset` | Battery/Balanced/Performance | (within-tab advanced) |
| xrAdaptiveQuality | Adaptive Quality | toggle | `xr.enableAdaptiveQuality` | – | (within-tab advanced) |

### AI tab (lines 438-466) — ADVANCED + POWER USER ONLY
| Key | Label | Type | Path | Min/Max | Notes |
|---|---|---|---|---|---|
| ragflowApiUrl | RAGFlow API URL | text | `ragflow.apiBaseUrl` | – | |
| ragflowAgentId | Agent ID | text | `ragflow.agentId` | – | |
| ragflowTimeout | Timeout (ms) | slider | `ragflow.timeout` | 5000-120000 | |
| perplexityModel | Perplexity Model | text | `perplexity.model` | – | |
| perplexityMaxTokens | Max Tokens | slider | `perplexity.maxTokens` | 100-4096 | |
| perplexityTemperature | Temperature | slider | `perplexity.temperature` | 0-2 | |
| openaiBaseUrl | OpenAI Base URL | text | `openai.baseUrl` | – | |
| openaiTimeout | Timeout (ms) | slider | `openai.timeout` | 5000-120000 | |
| kokoroApiUrl | Kokoro API URL | text | `kokoro.apiUrl` | – | TTS |
| kokoroVoice | Default Voice | text | `kokoro.defaultVoice` | – | |
| kokoroSpeed | Speech Speed | slider | `kokoro.defaultSpeed` | 0.5-2 | |
| whisperApiUrl | Whisper API URL | text | `whisper.apiUrl` | – | STT |
| whisperModel | Whisper Model | text | `whisper.defaultModel` | – | |
| whisperLanguage | Language | text | `whisper.defaultLanguage` | – | |

### DEVELOPER tab (lines 472-495) — ADVANCED + POWER USER ONLY
| Key | Label | Type | Path | Notes |
|---|---|---|---|---|
| enableDebug | Debug Mode | toggle | `system.debug.enabled` | DUPLICATE of System.enableDebug |
| enableDataDebug | Data Debug | toggle | `system.debug.enableDataDebug` | |
| enableWebsocketDebug | WebSocket Debug | toggle | `system.debug.enableWebsocketDebug` | |
| logBinaryHeaders | Log Binary Headers | toggle | `system.debug.logBinaryHeaders` | |
| logFullJson | Log Full JSON | toggle | `system.debug.logFullJson` | |
| enablePhysicsDebug | Physics Debug | toggle | `system.debug.enablePhysicsDebug` | |
| enableNodeDebug | Node Debug | toggle | `system.debug.enableNodeDebug` | |
| enableShaderDebug | Shader Debug | toggle | `system.debug.enableShaderDebug` | |
| enableMatrixDebug | Matrix Debug | toggle | `system.debug.enableMatrixDebug` | |
| enablePerformanceDebug | Performance Debug | toggle | `system.debug.enablePerformanceDebug` | |
| showForceVectors | Show Force Vectors | toggle | `developer.gpu.showForceVectors` | |
| showConstraints | Show Constraints | toggle | `developer.gpu.showConstraints` | |
| showBoundaryForces | Show Boundary Forces | toggle | `developer.gpu.showBoundaryForces` | |
| showConvergenceGraph | Convergence Graph | toggle | `developer.gpu.showConvergenceGraph` | |

## Wire bindings
| Class | Reader | Writer | Transport | Endpoint |
|---|---|---|---|---|
| Most settings | `UnifiedSettingsTabContent.tsx:59` `getValueFromPath()` | `UnifiedSettingsTabContent.tsx:78` `updateSettingByPath()` → `autoSaveManager.queueChange()` | 500 ms debounced batch | NIP-98 PATCH `/api/settings` |
| Layout (`qualityGates.layoutMode`, `…physics.layoutAlgorithm`) | same | `UnifiedSettingsTabContent.tsx:109-116` `layoutApi.setMode(value, 800ms)` | immediate REST | `/api/layout/setMode` |
| Refresh action | n/a | action-button | WS push | `webSocketService.forceRefreshFilter()` |
| `auth.nostr` | `UnifiedSettingsTabContent.tsx:127-146` | same | local | nostrAuthService |
| WebGPU toggle | n/a | action-button | local | sets `forceWebGLOverride` global, full reload |

## Disconnects (control center)
1. **Layout duplication**: `qualityGates.layoutMode` (force-directed/dag-*/type-clustering) vs `…physics.layoutAlgorithm` (forceDirected/hierarchical/radial/spectral/temporal/clustered) — two selects with overlapping intent and different option vocabularies.
2. **Debug duplication**: `system.debug.enabled` appears in both System and Developer tabs.
3. **`rendererInfo` orphan**: read-only field bound to `rendererCapabilities`, no schema entry.
4. **Nostr power-user cache**: server-denied power-user demotion mid-session does not clear the UI cache; UI continues to render PU-only fields enabled.
5. **Generated-types drift**: many UI paths (e.g. `qualityGates.ontologyStrength`) are not present in `client/src/types/generated/settings.ts`.
6. **Action-button registry**: `refresh_graph` and `toggle-webgpu` are hardcoded — no plugin registration.
7. **filterMode disconnect**: `nodeFilter.filterMode` (or/and) is set client-side; server-side honour during refresh is unverified.
8. **Collapsed AI mode is shallow**: keyword regex parser; ignores the existing `/api/nl-query/*` handler which can do real Cypher translation/explanation/validation.
9. **Power-user gating UX inconsistent**: some PU-only fields hidden, some shown but disabled (opacity 0.5).
10. **Many "Adv" flags inconsistent**: some controls flagged Advanced (e.g. `arrowSize`), the equivalent settings on a sibling object are not (e.g. `edgeWidth`).
