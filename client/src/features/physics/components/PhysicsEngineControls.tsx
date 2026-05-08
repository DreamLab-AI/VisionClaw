import React, { useState, useCallback, useEffect } from 'react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/features/design-system/components/Card';
import { useSettingsStore } from '@/store/settingsStore';
import { PhysicsSettings } from '@/features/settings/config/settings';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/features/design-system/components/Select';
import { Slider } from '@/features/design-system/components/Slider';
import { Switch } from '@/features/design-system/components/Switch';
import { Label } from '@/features/design-system/components/Label';
import { Button } from '@/features/design-system/components/Button';
import { Badge } from '@/features/design-system/components/Badge';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/features/design-system/components/Tabs';
import { Info, Cpu, Zap, Layers, GitBranch, Activity, AlertCircle, Plus } from 'lucide-react';
import { useToast } from '@/features/design-system/components/Toast';
import { TooltipRoot, TooltipContent, TooltipProvider, TooltipTrigger } from '@/features/design-system/components/Tooltip';
import { SemanticClusteringControls } from '@/features/analytics/components/SemanticClusteringControls';
import { ConstraintBuilderDialog } from './ConstraintBuilderDialog';
import { PhysicsPresets } from './PhysicsPresets';
import { unifiedApiClient } from '../../../services/api';

type KernelMode = 'legacy' | 'advanced' | 'visual_analytics';

interface ForceParameters {
  repulsionStrength: number;
  springStrength: number;
  damping: number;
  gravity: number;
  timeStep: number;
  maxVelocity: number;
  temperature: number;

  boundaryExtremeMultiplier: number;
  boundaryExtremeForceMultiplier: number;
  boundaryVelocityDamping: number;
}

interface ConstraintType {
  id: string;
  name: string;
  enabled: boolean;
  description: string;
  icon: string;
}

interface IsolationLayer {
  id: string;
  name: string;
  color: string;
  opacity: number;
  nodeCount: number;
  active: boolean;
}

export function PhysicsEngineControls() {
  const { toast } = useToast();
  
  const { settings, initialized, updateSettings, loadSection, ensureLoaded } = useSettingsStore();
  const [currentGraph] = useState<'logseq' | 'visionflow'>('logseq');
  
  
  const updatePhysics = async (physicsUpdate: Partial<PhysicsSettings>) => {
    updateSettings((draft) => {
      if (!draft.visualisation.graphs[currentGraph]) {
        draft.visualisation.graphs[currentGraph] = {
          nodes: draft.visualisation.graphs.logseq.nodes,
          edges: draft.visualisation.graphs.logseq.edges,
          labels: draft.visualisation.graphs.logseq.labels,
          physics: draft.visualisation.graphs.logseq.physics,
        };
      }
      Object.assign(draft.visualisation.graphs[currentGraph].physics, physicsUpdate);
    });
  };
  
  const [kernelMode, setKernelMode] = useState<KernelMode>('visual_analytics');
  const [showConstraintBuilder, setShowConstraintBuilder] = useState(false);
  
  
  const physicsSettings = settings?.visualisation?.graphs?.[currentGraph]?.physics;
  
  const [constraints, setConstraints] = useState<ConstraintType[]>([
    { id: 'fixed', name: 'Fixed Position', enabled: false, description: 'Lock nodes in place', icon: '📌' },
    { id: 'separation', name: 'Separation', enabled: false, description: 'Minimum distance between nodes', icon: '↔️' },
    { id: 'alignment_h', name: 'Horizontal Alignment', enabled: false, description: 'Align nodes horizontally', icon: '═' },
    { id: 'alignment_v', name: 'Vertical Alignment', enabled: false, description: 'Align nodes vertically', icon: '║' },
    { id: 'boundary', name: 'Boundary', enabled: false, description: 'Keep nodes within bounds', icon: '⬚' },
    { id: 'cluster', name: 'Cluster', enabled: false, description: 'Group related nodes', icon: '🔶' },
    { id: 'tree', name: 'Tree Layout', enabled: false, description: 'Hierarchical tree structure', icon: '🌳' },
    { id: 'radial', name: 'Radial', enabled: false, description: 'Radial distance constraints', icon: '⭕' },
    { id: 'layer', name: 'Layer', enabled: false, description: 'Layer-based positioning', icon: '📚' },
    { id: 'collision', name: 'Collision', enabled: false, description: 'Prevent node overlap', icon: '💥' },
  ]);
  
  const [isolationLayers, setIsolationLayers] = useState<IsolationLayer[]>([
    { id: 'focus', name: 'Focus Layer', color: '#3B82F6', opacity: 1.0, nodeCount: 0, active: false },
    { id: 'context', name: 'Context Layer', color: '#8B5CF6', opacity: 0.7, nodeCount: 0, active: false },
    { id: 'background', name: 'Background Layer', color: '#6B7280', opacity: 0.3, nodeCount: 0, active: false },
  ]);
  
  const [trajectorySettings, setTrajectorySettings] = useState({
    enabled: false,
    length: 8,
    fadeRate: 0.9,
    colorByVelocity: true,
  });
  
  const [gpuMetrics, setGpuMetrics] = useState({
    utilization: 0,
    memory: 0,
    temperature: 0,
    power: 0,
  });

  
  useEffect(() => {
    const loadPhysicsSettings = async () => {
      if (initialized) {
        
        await ensureLoaded([
          `visualisation.graphs.${currentGraph}.physics`,
          'visualisation.graphs.logseq.physics',
          'visualisation.graphs.visionflow.physics'
        ]);
      }
    };
    
    loadPhysicsSettings();
  }, [initialized, currentGraph, ensureLoaded]);
  
  
  
  
  useEffect(() => {
    const fetchMetrics = async () => {
      try {
        const response = await unifiedApiClient.get('/api/analytics/gpu-metrics');
        setGpuMetrics(response.data);
      } catch (error) {

      }
    };

    const delay = setTimeout(() => {
      fetchMetrics();
      interval = setInterval(fetchMetrics, 10000);
    }, 3000);
    let interval: ReturnType<typeof setInterval>;
    return () => { clearTimeout(delay); clearInterval(interval); };
  }, []);

  
  const handleKernelModeChange = useCallback(async (mode: KernelMode) => {
    try {
      const response = await unifiedApiClient.post('/api/analytics/kernel-mode', { mode });

      setKernelMode(mode);
      toast({
        title: 'Kernel Mode Changed',
        description: `Switched to ${mode} kernel`,
      });
    } catch (error) {
      toast({
        title: 'Error',
        description: 'Failed to change kernel mode',
        variant: 'destructive',
      });
    }
  }, [toast]);

  const handleForceParamChange = useCallback(async (param: keyof ForceParameters, value: number) => {
    
    const paramMapping: Record<string, string> = {
      repulsionStrength: 'repelK',
      springStrength: 'springK',
      damping: 'damping',
      gravity: 'gravity',
      timeStep: 'dt',
      maxVelocity: 'maxVelocity',
      temperature: 'temperature',

      boundaryExtremeMultiplier: 'boundaryExtremeMultiplier',
      boundaryExtremeForceMultiplier: 'boundaryExtremeForceMultiplier',
      boundaryVelocityDamping: 'boundaryVelocityDamping',
    };
    
    const settingsPath = `visualisation.graphs.${currentGraph}.physics.${paramMapping[param] || param}`;
    
    try {
      
      updateSettings((draft) => {
        const pathParts = settingsPath.split('.');
        let current: any = draft;
        for (let i = 0; i < pathParts.length - 1; i++) {
          current = current[pathParts[i]];
        }
        current[pathParts[pathParts.length - 1]] = value;
      });
      
      toast({
        title: 'Physics Updated',
        description: `${param} set to ${value.toFixed(3)}`,
      });
    } catch (error) {
      toast({
        title: 'Error',
        description: 'Failed to update physics parameters',
        variant: 'destructive',
      });
    }
  }, [currentGraph, updateSettings, toast]);

  const handleConstraintToggle = useCallback(async (constraintId: string) => {
    const newConstraints = constraints.map(c =>
      c.id === constraintId ? { ...c, enabled: !c.enabled } : c
    );
    setConstraints(newConstraints);

    try {
      await unifiedApiClient.post('/api/analytics/constraints', {
        constraints: newConstraints.filter(c => c.enabled).map(c => c.id),
      });
    } catch (error) {
      
    }
  }, [constraints]);

  const handleLayerToggle = useCallback(async (layerId: string) => {
    const newLayers = isolationLayers.map(l =>
      l.id === layerId ? { ...l, active: !l.active } : l
    );
    setIsolationLayers(newLayers);

    try {
      await unifiedApiClient.post('/api/analytics/layers', {
        layers: newLayers.filter(l => l.active),
      });
    } catch (error) {
      
    }
  }, [isolationLayers]);

  // No-op: constraint saving not yet implemented
  const handleSaveConstraint = useCallback((_constraint: any) => {}, []);

  return (
    <div className="h-full overflow-auto">
      <Tabs defaultValue="engine" className="w-full">
        <TabsList className="grid w-full grid-cols-4">
          <TabsTrigger value="engine">Engine</TabsTrigger>
          <TabsTrigger value="forces">Forces</TabsTrigger>
          <TabsTrigger value="constraints">Constraints</TabsTrigger>
          <TabsTrigger value="analytics">Analytics</TabsTrigger>
        </TabsList>
        
        {}
        <TabsContent value="engine" className="space-y-4">
          {}
          <PhysicsPresets />
          
          {}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Cpu className="h-5 w-5" />
                GPU Engine Status
              </CardTitle>
              <CardDescription>
                Real-time GPU performance metrics
              </CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 gap-4">
                <div className="space-y-2">
                  <div className="flex justify-between items-center">
                    <span className="text-sm text-muted-foreground">Utilization</span>
                    <Badge variant={gpuMetrics.utilization > 80 ? 'destructive' : 'default'}>
                      {gpuMetrics.utilization}%
                    </Badge>
                  </div>
                  <div className="flex justify-between items-center">
                    <span className="text-sm text-muted-foreground">Memory</span>
                    <Badge variant={gpuMetrics.memory > 80 ? 'destructive' : 'default'}>
                      {gpuMetrics.memory}%
                    </Badge>
                  </div>
                </div>
                <div className="space-y-2">
                  <div className="flex justify-between items-center">
                    <span className="text-sm text-muted-foreground">Temperature</span>
                    <Badge variant={gpuMetrics.temperature > 75 ? 'destructive' : 'default'}>
                      {gpuMetrics.temperature}°C
                    </Badge>
                  </div>
                  <div className="flex justify-between items-center">
                    <span className="text-sm text-muted-foreground">Power</span>
                    <Badge>{gpuMetrics.power}W</Badge>
                  </div>
                </div>
              </div>
            </CardContent>
          </Card>

          {}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Zap className="h-5 w-5" />
                GPU Kernel Mode
              </CardTitle>
              <CardDescription>
                Select the GPU computation kernel
              </CardDescription>
            </CardHeader>
            <CardContent>
              <Select value={kernelMode} onValueChange={handleKernelModeChange}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="legacy">
                    <div className="flex flex-col">
                      <span>Legacy</span>
                      <span className="text-xs text-muted-foreground">Basic force-directed layout</span>
                    </div>
                  </SelectItem>
                  <SelectItem value="advanced">
                    <div className="flex flex-col">
                      <span>Advanced</span>
                      <span className="text-xs text-muted-foreground">UMAP, spectral clustering, GNN</span>
                    </div>
                  </SelectItem>
                  <SelectItem value="visual_analytics">
                    <div className="flex flex-col">
                      <span>Visual Analytics</span>
                      <span className="text-xs text-muted-foreground">Temporal-spatial 4D visualization</span>
                    </div>
                  </SelectItem>
                </SelectContent>
              </Select>
            </CardContent>
          </Card>

          {}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Layers className="h-5 w-5" />
                Isolation Layers
              </CardTitle>
              <CardDescription>
                Visual focus and context layers
              </CardDescription>
            </CardHeader>
            <CardContent>
              <div className="space-y-3">
                {isolationLayers.map(layer => (
                  <div key={layer.id} className="flex items-center justify-between p-3 rounded-lg border">
                    <div className="flex items-center gap-3">
                      <div 
                        className="w-4 h-4 rounded"
                        style={{ 
                          backgroundColor: layer.color,
                          opacity: layer.opacity,
                        }}
                      />
                      <div>
                        <Label htmlFor={layer.id}>{layer.name}</Label>
                        <p className="text-xs text-muted-foreground">
                          {layer.nodeCount} nodes • {(layer.opacity * 100).toFixed(0)}% opacity
                        </p>
                      </div>
                    </div>
                    <Switch
                      id={layer.id}
                      checked={layer.active}
                      onCheckedChange={() => handleLayerToggle(layer.id)}
                    />
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        {}
        <TabsContent value="forces" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Activity className="h-5 w-5" />
                Force Parameters
              </CardTitle>
              <CardDescription>
                Fine-tune physics simulation parameters
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-2">
                <div className="flex justify-between items-center">
                  <Label htmlFor="repulsionStrength">Repulsion Strength</Label>
                  <span className="text-sm text-muted-foreground">{(physicsSettings?.repelK || 50.0).toFixed(1)}</span>
                </div>
                <Slider
                  id="repulsionStrength"
                  min={10}
                  max={200}  
                  step={1}
                  value={[physicsSettings?.repelK || 50.0]}
                  onValueChange={([v]) => handleForceParamChange('repulsionStrength', v)}
                />
              </div>
              
              {/* "Attraction Strength" slider removed 2026-04-29: it was a duplicate
                  of "Spring Strength" with no separate physics field on the server. */}

              <div className="space-y-2">
                <div className="flex justify-between items-center">
                  <Label htmlFor="damping">Damping</Label>
                  <span className="text-sm text-muted-foreground">{(physicsSettings?.damping || 0.95).toFixed(2)}</span>
                </div>
                <Slider
                  id="damping"
                  min={0.5}  
                  max={0.99}
                  step={0.01}
                  value={[physicsSettings?.damping || 0.95]}
                  onValueChange={([v]) => handleForceParamChange('damping', v)}
                />
              </div>
              
              <div className="space-y-2">
                <div className="flex justify-between items-center">
                  <Label htmlFor="temperature">Temperature</Label>
                  <span className="text-sm text-muted-foreground">{(physicsSettings?.temperature || 0.01).toFixed(2)}</span>
                </div>
                <Slider
                  id="temperature"
                  min={0}
                  max={2.0}  
                  step={0.01}
                  value={[physicsSettings?.temperature || 0.01]}
                  onValueChange={([v]) => handleForceParamChange('temperature', v)}
                />
              </div>

              <div className="space-y-2">
                <div className="flex justify-between items-center">
                  <Label htmlFor="gravity">Gravity</Label>
                  <span className="text-sm text-muted-foreground">{(physicsSettings?.gravity || 0.0001).toFixed(4)}</span>
                </div>
                <Slider
                  id="gravity"
                  min={-5.0}
                  max={5.0}  
                  step={0.01}
                  value={[physicsSettings?.gravity || 0.0001]}
                  onValueChange={([v]) => handleForceParamChange('gravity', v)}
                />
              </div>

              <div className="space-y-2">
                <div className="flex justify-between items-center">
                  <Label htmlFor="maxVelocity">Max Velocity</Label>
                  <span className="text-sm text-muted-foreground">{(physicsSettings?.maxVelocity || 2.0).toFixed(1)}</span>
                </div>
                <Slider
                  id="maxVelocity"
                  min={0.1}
                  max={10}   
                  step={0.1}
                  value={[physicsSettings?.maxVelocity || 2.0]}
                  onValueChange={([v]) => handleForceParamChange('maxVelocity', v)}
                />
              </div>

              <div className="space-y-2">
                <div className="flex justify-between items-center">
                  <Label htmlFor="timeStep">Time Step</Label>
                  <span className="text-sm text-muted-foreground">{(physicsSettings?.dt || 0.016).toFixed(3)}</span>
                </div>
                <Slider
                  id="timeStep"
                  min={0.001}
                  max={0.02}   
                  step={0.001}
                  value={[physicsSettings?.dt || 0.016]}
                  onValueChange={([v]) => handleForceParamChange('timeStep', v)}
                />
              </div>
            </CardContent>
          </Card>

          {}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <AlertCircle className="h-5 w-5" />
                Boundary Behavior
              </CardTitle>
              <CardDescription>
                Advanced boundary force control for CUDA physics
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-2">
                <div className="flex justify-between items-center">
                  <TooltipProvider>
                    <TooltipRoot>
                      <TooltipTrigger asChild>
                        <Label htmlFor="boundaryExtremeMultiplier" className="flex items-center gap-1">
                          Extreme Multiplier
                          <Info className="h-3 w-3 text-muted-foreground" />
                        </Label>
                      </TooltipTrigger>
                      <TooltipContent>
                        <p>Controls how aggressively boundary forces are applied (1.0-5.0)</p>
                      </TooltipContent>
                    </TooltipRoot>
                  </TooltipProvider>
                  <span className="text-sm text-muted-foreground">{(physicsSettings?.boundaryExtremeMultiplier || 2.0).toFixed(1)}</span>
                </div>
                <Slider
                  id="boundaryExtremeMultiplier"
                  min={1.0}
                  max={5.0}
                  step={0.1}
                  value={[physicsSettings?.boundaryExtremeMultiplier || 2.0]}
                  onValueChange={([v]) => handleForceParamChange('boundaryExtremeMultiplier', v)}
                />
              </div>
              
              <div className="space-y-2">
                <div className="flex justify-between items-center">
                  <TooltipProvider>
                    <TooltipRoot>
                      <TooltipTrigger asChild>
                        <Label htmlFor="boundaryExtremeForceMultiplier" className="flex items-center gap-1">
                          Force Strength
                          <Info className="h-3 w-3 text-muted-foreground" />
                        </Label>
                      </TooltipTrigger>
                      <TooltipContent>
                        <p>Controls the intensity of extreme boundary forces (1.0-20.0)</p>
                      </TooltipContent>
                    </TooltipRoot>
                  </TooltipProvider>
                  <span className="text-sm text-muted-foreground">{(physicsSettings?.boundaryExtremeForceMultiplier || 5.0).toFixed(1)}</span>
                </div>
                <Slider
                  id="boundaryExtremeForceMultiplier"
                  min={1.0}
                  max={20.0}
                  step={0.5}
                  value={[physicsSettings?.boundaryExtremeForceMultiplier || 5.0]}
                  onValueChange={([v]) => handleForceParamChange('boundaryExtremeForceMultiplier', v)}
                />
              </div>
              
              <div className="space-y-2">
                <div className="flex justify-between items-center">
                  <TooltipProvider>
                    <TooltipRoot>
                      <TooltipTrigger asChild>
                        <Label htmlFor="boundaryVelocityDamping" className="flex items-center gap-1">
                          Velocity Damping
                          <Info className="h-3 w-3 text-muted-foreground" />
                        </Label>
                      </TooltipTrigger>
                      <TooltipContent>
                        <p>Reduces velocity near boundaries to prevent edge oscillation (0.0-1.0)</p>
                      </TooltipContent>
                    </TooltipRoot>
                  </TooltipProvider>
                  <span className="text-sm text-muted-foreground">{(physicsSettings?.boundaryVelocityDamping || 0.8).toFixed(2)}</span>
                </div>
                <Slider
                  id="boundaryVelocityDamping"
                  min={0.0}
                  max={1.0}
                  step={0.01}
                  value={[physicsSettings?.boundaryVelocityDamping || 0.8]}
                  onValueChange={([v]) => handleForceParamChange('boundaryVelocityDamping', v)}
                />
              </div>
            </CardContent>
          </Card>

          {}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Activity className="h-5 w-5" />
                Trajectory Visualization
              </CardTitle>
              <CardDescription>
                Show node movement trails
              </CardDescription>
            </CardHeader>
            <CardContent>
              <div className="space-y-4">
                <div className="flex items-center justify-between">
                  <Label htmlFor="trajectory-enabled">Enable Trajectories</Label>
                  <Switch
                    id="trajectory-enabled"
                    checked={trajectorySettings.enabled}
                    onCheckedChange={(enabled) => 
                      setTrajectorySettings({ ...trajectorySettings, enabled })
                    }
                  />
                </div>
                
                {trajectorySettings.enabled && (
                  <>
                    <div className="space-y-2">
                      <div className="flex justify-between items-center">
                        <Label htmlFor="trajectory-length">Trail Length</Label>
                        <span className="text-sm text-muted-foreground">{trajectorySettings.length}</span>
                      </div>
                      <Slider
                        id="trajectory-length"
                        min={2}
                        max={16}
                        step={1}
                        value={[trajectorySettings.length]}
                        onValueChange={([v]) => 
                          setTrajectorySettings({ ...trajectorySettings, length: v })
                        }
                      />
                    </div>
                    
                    <div className="flex items-center justify-between">
                      <Label htmlFor="color-velocity">Color by Velocity</Label>
                      <Switch
                        id="color-velocity"
                        checked={trajectorySettings.colorByVelocity}
                        onCheckedChange={(colorByVelocity) => 
                          setTrajectorySettings({ ...trajectorySettings, colorByVelocity })
                        }
                      />
                    </div>
                  </>
                )}
              </div>
            </CardContent>
          </Card>

          {}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <GitBranch className="h-5 w-5" />
                SSSP Path Integration
              </CardTitle>
              <CardDescription>
                Single-Source Shortest Path influence on spring forces
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex items-center justify-between">
                <div className="space-y-1">
                  <Label htmlFor="useSsspDistances">Enable SSSP Integration</Label>
                  <p className="text-xs text-muted-foreground">
                    Modulate spring forces based on shortest path distances
                  </p>
                </div>
                <Switch
                  id="useSsspDistances"
                  checked={physicsSettings?.useSsspDistances || false}
                  onCheckedChange={(checked) => updatePhysics({ useSsspDistances: checked })}
                />
              </div>

              {physicsSettings?.useSsspDistances && (
                <div className="space-y-2">
                  <div className="flex justify-between items-center">
                    <TooltipProvider>
                      <TooltipRoot>
                        <TooltipTrigger asChild>
                          <Label htmlFor="ssspAlpha" className="flex items-center gap-1">
                            SSSP Influence
                            <Info className="h-3 w-3 text-muted-foreground" />
                          </Label>
                        </TooltipTrigger>
                        <TooltipContent>
                          <p>Weight factor for SSSP distances (0.0 = no effect, 1.0 = full effect)</p>
                        </TooltipContent>
                      </TooltipRoot>
                    </TooltipProvider>
                    <span className="text-sm text-muted-foreground">
                      {(physicsSettings?.ssspAlpha || 0.5).toFixed(2)}
                    </span>
                  </div>
                  <Slider
                    id="ssspAlpha"
                    min={0}
                    max={1}
                    step={0.01}
                    value={[physicsSettings?.ssspAlpha || 0.5]}
                    onValueChange={([v]) => updatePhysics({ ssspAlpha: v })}
                  />

                  <div className="mt-3 p-3 bg-muted/50 rounded-md">
                    <p className="text-xs text-muted-foreground">
                      SSSP integration adjusts rest length of springs based on shortest path distance between nodes:
                    </p>
                    <code className="text-xs mt-1 block">
                      ideal_length = rest_length + alpha * |d[u] - d[v]|
                    </code>
                  </div>
                </div>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        {}
        <TabsContent value="constraints" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <GitBranch className="h-5 w-5" />
                Layout Constraints
              </CardTitle>
              <CardDescription>
                Enable and configure layout constraints
              </CardDescription>
            </CardHeader>
            <CardContent>
              <div className="space-y-3">
                {constraints.map(constraint => (
                  <div key={constraint.id} className="flex items-center justify-between">
                    <div className="flex items-center gap-3">
                      <span className="text-lg">{constraint.icon}</span>
                      <div>
                        <Label htmlFor={constraint.id}>{constraint.name}</Label>
                        <p className="text-xs text-muted-foreground">{constraint.description}</p>
                      </div>
                    </div>
                    <Switch
                      id={constraint.id}
                      checked={constraint.enabled}
                      onCheckedChange={() => handleConstraintToggle(constraint.id)}
                    />
                  </div>
                ))}
              </div>
              
              <div className="mt-4 pt-4 border-t">
                <Button 
                  onClick={() => setShowConstraintBuilder(true)}
                  className="w-full"
                  variant="outline"
                >
                  <Plus className="mr-2 h-4 w-4" />
                  Create Custom Constraint
                </Button>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        {}
        <TabsContent value="analytics" className="space-y-4">
          <SemanticClusteringControls />
        </TabsContent>
      </Tabs>

      {}
      <ConstraintBuilderDialog
        isOpen={showConstraintBuilder}
        onClose={() => setShowConstraintBuilder(false)}
        onSave={handleSaveConstraint}
      />
    </div>
  );
}