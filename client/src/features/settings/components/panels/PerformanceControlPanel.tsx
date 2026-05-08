import React, { useState, useEffect } from 'react';
import { Zap, Cpu, MemoryStick, Gauge, TrendingUp, Activity, Settings as SettingsIcon } from 'lucide-react';
import { useSettingsStore } from '@/store/settingsStore';
import { createLogger } from '../../../../utils/loggerConfig';

const logger = createLogger('PerformanceControlPanel');
import { Button } from '@/features/design-system/components/Button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/features/design-system/components/Card';
import { Switch } from '@/features/design-system/components/Switch';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/features/design-system/components/Select';
import { Slider } from '@/features/design-system/components/Slider';
import { Badge } from '@/features/design-system/components/Badge';
import { Separator } from '@/features/design-system/components/Separator';
import { Label } from '@/features/design-system/components/Label';
import { Alert, AlertDescription } from '@/features/design-system/components/Alert';



interface PerformanceMetrics {
  currentFPS: number;
  gpuUsage: number;
  gpuMemoryUsed: number;
  gpuMemoryTotal: number;
  iterationsPerSecond: number;
  convergenceRate: number;
}

const LOD_LEVELS = [
  { value: 'low', label: 'Low', description: 'Battery saver, 30-45 FPS', targetFPS: 30 },
  { value: 'medium', label: 'Medium', description: 'Balanced, 45-60 FPS', targetFPS: 60 },
  { value: 'high', label: 'High', description: 'Recommended, 55-60 FPS', targetFPS: 60 },
  { value: 'ultra', label: 'Ultra', description: 'Maximum quality, 90-120 FPS', targetFPS: 120 }
];

const GPU_BLOCK_SIZES = ['64', '128', '256', '512'];

export const PerformanceControlPanel: React.FC = () => {
  const settings = useSettingsStore(state => state.settings);
  const updateSettings = useSettingsStore(state => state.updateSettings);

  // Extended performance settings that may not be in base PerformanceSettings type
  interface ExtendedPerfSettings {
    showFPS?: boolean;
    targetFPS?: number;
    levelOfDetail?: string;
    gpuMemoryLimit?: number;
    gpuBlockSize?: string;
    enableMemoryCoalescing?: boolean;
    iterationLimit?: number;
    warmupDuration?: number;
    convergenceThreshold?: number;
    enableAdaptiveQuality?: boolean;
    enableAdaptiveCooling?: boolean;
  }
  const perfSettings = (settings?.performance ?? {}) as unknown as ExtendedPerfSettings;

  const updateSetting = (path: string, value: string | number | boolean) => {
    updateSettings((draft) => {
      const parts = path.split('.');
      let current = draft as unknown as Record<string, unknown>;
      for (let i = 0; i < parts.length - 1; i++) {
        if (!current[parts[i]]) current[parts[i]] = {};
        current = current[parts[i]] as Record<string, unknown>;
      }
      current[parts[parts.length - 1]] = value;
    });
  };
  const [metrics, setMetrics] = useState<PerformanceMetrics>({
    currentFPS: 0,
    gpuUsage: 0,
    gpuMemoryUsed: 0,
    gpuMemoryTotal: 0,
    iterationsPerSecond: 0,
    convergenceRate: 0
  });

  
  useEffect(() => {
    const pollMetrics = async () => {
      try {
        const response = await fetch('/api/performance/metrics');
        if (response.ok) {
          const data = await response.json();
          setMetrics({
            currentFPS: data.fps || 0,
            gpuUsage: data.gpu_usage || 0,
            gpuMemoryUsed: data.gpu_memory_used || 0,
            gpuMemoryTotal: data.gpu_memory_total || 0,
            iterationsPerSecond: data.iterations_per_second || 0,
            convergenceRate: data.convergence_rate || 0
          });
        }
      } catch (error) {
        logger.warn('Failed to fetch performance metrics:', error);
      }
    };

    const delay = setTimeout(() => {
      pollMetrics();
      timer = setInterval(pollMetrics, 10000);
    }, 3000);
    let timer: ReturnType<typeof setInterval>;
    return () => { clearTimeout(delay); clearInterval(timer); };
  }, []);

  const getFPSColor = (fps: number, target: number) => {
    if (fps >= target * 0.9) return 'text-green-500';
    if (fps >= target * 0.7) return 'text-yellow-500';
    return 'text-red-500';
  };

  const getMemoryColor = (used: number, total: number) => {
    const percentage = (used / total) * 100;
    if (percentage < 70) return 'bg-green-500';
    if (percentage < 90) return 'bg-yellow-500';
    return 'bg-red-500';
  };

  const applyQualityPreset = (preset: string) => {
    const level = LOD_LEVELS.find(l => l.value === preset);
    if (level) {
      updateSetting('performance.levelOfDetail', preset);
      updateSetting('performance.targetFPS', level.targetFPS);

      
      switch (preset) {
        case 'low':
          updateSetting('performance.gpuMemoryLimit', 1024);
          updateSetting('performance.iterationLimit', 100);
          updateSetting('performance.gpuBlockSize', '64');
          break;
        case 'medium':
          updateSetting('performance.gpuMemoryLimit', 2048);
          updateSetting('performance.iterationLimit', 300);
          updateSetting('performance.gpuBlockSize', '128');
          break;
        case 'high':
          updateSetting('performance.gpuMemoryLimit', 4096);
          updateSetting('performance.iterationLimit', 500);
          updateSetting('performance.gpuBlockSize', '256');
          break;
        case 'ultra':
          updateSetting('performance.gpuMemoryLimit', 8192);
          updateSetting('performance.iterationLimit', 1000);
          updateSetting('performance.gpuBlockSize', '512');
          break;
      }
    }
  };

  return (
    <div className="space-y-6">
      {}
      <div>
        <h2 className="text-2xl font-bold flex items-center gap-2">
          <Zap className="w-6 h-6" />
          Performance Settings
        </h2>
        <p className="text-sm text-muted-foreground mt-1">
          GPU optimization, FPS controls, and quality management
        </p>
      </div>

      {/* Live metrics display when FPS counter is enabled */}
      {perfSettings?.showFPS && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Activity className="w-5 h-5" />
              Live Performance Metrics
            </CardTitle>
            <CardDescription>
              Real-time GPU and rendering statistics
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {/* Current FPS display */}
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium">Current FPS:</span>
              <span className={`text-3xl font-mono font-bold ${getFPSColor(metrics.currentFPS, perfSettings?.targetFPS || 60)}`}>
                {metrics.currentFPS.toFixed(1)}
              </span>
            </div>

            {/* Target FPS display */}
            <div className="flex items-center justify-between text-sm">
              <span className="text-muted-foreground">Target:</span>
              <span className="font-mono">{perfSettings?.targetFPS || 60} FPS</span>
            </div>

            <Separator />

            {}
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">GPU Usage:</span>
                <span className="text-lg font-mono">{metrics.gpuUsage.toFixed(1)}%</span>
              </div>
              <div className="w-full bg-gray-200 rounded-full h-2">
                <div
                  className="bg-blue-500 h-2 rounded-full transition-all duration-300"
                  style={{ width: `${metrics.gpuUsage}%` }}
                />
              </div>
            </div>

            {}
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">GPU Memory:</span>
                <span className="text-lg font-mono">
                  {metrics.gpuMemoryUsed} MB / {metrics.gpuMemoryTotal} MB
                </span>
              </div>
              <div className="w-full bg-gray-200 rounded-full h-2">
                <div
                  className={`h-2 rounded-full transition-all duration-300 ${getMemoryColor(metrics.gpuMemoryUsed, metrics.gpuMemoryTotal)}`}
                  style={{ width: `${(metrics.gpuMemoryUsed / metrics.gpuMemoryTotal) * 100}%` }}
                />
              </div>
            </div>

            {}
            <Separator />
            <div className="grid grid-cols-2 gap-4 text-sm">
              <div>
                <span className="text-muted-foreground">Iterations/sec:</span>
                <p className="text-lg font-mono">{metrics.iterationsPerSecond.toFixed(0)}</p>
              </div>
              <div>
                <span className="text-muted-foreground">Convergence:</span>
                <p className="text-lg font-mono">{(metrics.convergenceRate * 100).toFixed(1)}%</p>
              </div>
            </div>
          </CardContent>
        </Card>
      )}

      <Separator />

      {}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Gauge className="w-5 h-5" />
            Quality Presets
          </CardTitle>
          <CardDescription>
            One-click optimization for different hardware
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-2 gap-3">
            {LOD_LEVELS.map(level => (
              <Button
                key={level.value}
                variant={perfSettings?.levelOfDetail === level.value ? 'default' : 'outline'}
                onClick={() => applyQualityPreset(level.value)}
                className="h-auto py-3 flex-col items-start"
              >
                <span className="font-semibold">{level.label}</span>
                <span className="text-xs opacity-80">{level.description}</span>
              </Button>
            ))}
          </div>

          <Alert>
            <AlertDescription className="text-xs">
              Quality presets automatically adjust GPU memory, iteration limits, and block sizes
            </AlertDescription>
          </Alert>
        </CardContent>
      </Card>

      <Separator />

      {}
      <Card>
        <CardHeader>
          <CardTitle>FPS Controls</CardTitle>
          <CardDescription>
            Frame rate limiting and monitoring
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {}
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label htmlFor="show-fps">Show FPS Counter</Label>
              <p className="text-xs text-muted-foreground">
                Display real-time frame rate metrics
              </p>
            </div>
            <Switch
              id="show-fps"
              checked={perfSettings?.showFPS ?? true}
              onCheckedChange={(checked) => updateSetting('performance.showFPS', checked)}
            />
          </div>

          {}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label htmlFor="target-fps">Target FPS</Label>
              <span className="text-sm font-mono">{perfSettings?.targetFPS || 60}</span>
            </div>
            <Slider
              id="target-fps"
              min={30}
              max={144}
              step={15}
              value={[perfSettings?.targetFPS || 60]}
              onValueChange={([value]) => updateSetting('performance.targetFPS', value)}
              className="w-full"
            />
            <div className="flex justify-between text-xs text-muted-foreground">
              <span>30 (Battery)</span>
              <span>60 (Standard)</span>
              <span>144 (High-end)</span>
            </div>
          </div>
        </CardContent>
      </Card>

      <Separator />

      {}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <MemoryStick className="w-5 h-5" />
            GPU Memory Management
          </CardTitle>
          <CardDescription>
            Control GPU memory allocation and usage
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label htmlFor="gpu-memory">Memory Limit</Label>
              <span className="text-sm font-mono">{perfSettings?.gpuMemoryLimit || 4096} MB</span>
            </div>
            <Slider
              id="gpu-memory"
              min={512}
              max={16384}
              step={512}
              value={[perfSettings?.gpuMemoryLimit || 4096]}
              onValueChange={([value]) => updateSetting('performance.gpuMemoryLimit', value)}
              className="w-full"
            />
            <p className="text-xs text-muted-foreground">
              Maximum GPU memory allocation for physics and rendering
            </p>
          </div>

          {}
          <div className="space-y-2">
            <Label htmlFor="block-size">CUDA Block Size</Label>
            <Select
              value={perfSettings?.gpuBlockSize || '256'}
              onValueChange={(value) => updateSetting('performance.gpuBlockSize', value)}
            >
              <SelectTrigger id="block-size">
                <SelectValue placeholder="Select block size" />
              </SelectTrigger>
              <SelectContent>
                {GPU_BLOCK_SIZES.map(size => (
                  <SelectItem key={size} value={size}>
                    {size} threads
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">
              Kernel block size - Higher = better for large graphs
            </p>
          </div>

          {}
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label htmlFor="memory-coalescing">Enable Memory Coalescing</Label>
              <p className="text-xs text-muted-foreground">
                Optimize memory access patterns (5-15% speedup)
              </p>
            </div>
            <Switch
              id="memory-coalescing"
              checked={perfSettings?.enableMemoryCoalescing ?? true}
              onCheckedChange={(checked) => updateSetting('performance.enableMemoryCoalescing', checked)}
            />
          </div>
        </CardContent>
      </Card>

      <Separator />

      {}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Cpu className="w-5 h-5" />
            Physics Optimization
          </CardTitle>
          <CardDescription>
            Fine-tune physics simulation performance
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label htmlFor="iteration-limit">Iteration Limit</Label>
              <span className="text-sm font-mono">{perfSettings?.iterationLimit || 500}</span>
            </div>
            <Slider
              id="iteration-limit"
              min={50}
              max={2000}
              step={50}
              value={[perfSettings?.iterationLimit || 500]}
              onValueChange={([value]) => updateSetting('performance.iterationLimit', value)}
              className="w-full"
            />
            <p className="text-xs text-muted-foreground">
              Maximum physics iterations per frame
            </p>
          </div>

          {}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label htmlFor="warmup">Warmup Duration</Label>
              <span className="text-sm font-mono">{perfSettings?.warmupDuration || 1000} ms</span>
            </div>
            <Slider
              id="warmup"
              min={500}
              max={5000}
              step={500}
              value={[perfSettings?.warmupDuration || 1000]}
              onValueChange={([value]) => updateSetting('performance.warmupDuration', value)}
              className="w-full"
            />
            <p className="text-xs text-muted-foreground">
              Initial simulation stabilization period
            </p>
          </div>

          {}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label htmlFor="convergence">Convergence Threshold</Label>
              <span className="text-sm font-mono">{perfSettings?.convergenceThreshold || 0.001}</span>
            </div>
            <Slider
              id="convergence"
              min={0.0001}
              max={0.01}
              step={0.0001}
              value={[perfSettings?.convergenceThreshold || 0.001]}
              onValueChange={([value]) => updateSetting('performance.convergenceThreshold', value)}
              className="w-full"
            />
            <p className="text-xs text-muted-foreground">
              Lower = more accurate but slower convergence
            </p>
          </div>
        </CardContent>
      </Card>

      <Separator />

      {}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <TrendingUp className="w-5 h-5" />
            Adaptive Features
          </CardTitle>
          <CardDescription>
            Automatic quality and performance adjustments
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {}
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label htmlFor="adaptive-quality">Adaptive Quality</Label>
              <p className="text-xs text-muted-foreground">
                Automatically adjust quality to maintain target FPS
              </p>
            </div>
            <Switch
              id="adaptive-quality"
              checked={perfSettings?.enableAdaptiveQuality ?? true}
              onCheckedChange={(checked) => updateSetting('performance.enableAdaptiveQuality', checked)}
            />
          </div>

          {}
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label htmlFor="adaptive-cooling">Adaptive Cooling</Label>
              <p className="text-xs text-muted-foreground">
                Dynamic cooling rate based on convergence progress
              </p>
            </div>
            <Switch
              id="adaptive-cooling"
              checked={perfSettings?.enableAdaptiveCooling ?? true}
              onCheckedChange={(checked) => updateSetting('performance.enableAdaptiveCooling', checked)}
            />
          </div>

          <Alert>
            <AlertDescription className="text-xs">
              Adaptive features may temporarily reduce quality during heavy computation
            </AlertDescription>
          </Alert>
        </CardContent>
      </Card>
    </div>
  );
};
