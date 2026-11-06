

export const VIEWPORT_SETTINGS_PATTERNS = [
  
  'visualisation.nodes',
  'visualisation.edges',
  'visualisation.physics',
  'visualisation.rendering',
  'visualisation.animations',
  'visualisation.labels',
  'visualisation.bloom',
  'visualisation.glow', 
  'visualisation.hologram',
  'visualisation.camera',
  
  
  'visualisation.graphs.*.nodes',
  'visualisation.graphs.*.edges',
  'visualisation.graphs.*.physics',
  'visualisation.graphs.*.rendering',
  'visualisation.graphs.*.animations',
  'visualisation.graphs.*.labels',
  'visualisation.graphs.*.bloom',
  'visualisation.graphs.*.glow', 
  'visualisation.graphs.*.hologram',
  'visualisation.graphs.*.camera',
  
  
  'xr.mode',
  'xr.handMeshEnabled',
  'xr.handRayEnabled',
  'xr.enableLightEstimation',
  'xr.enablePlaneDetection',
  'xr.enablePassthroughPortal',
  
  // Graph-type visual settings
  'visualisation.graphTypeVisuals.*',

  'system.debug.enabled',
  'system.debug.showAxesHelper',
  'system.debug.showStats',
];


export function isViewportSetting(path: string): boolean {
  return VIEWPORT_SETTINGS_PATTERNS.some(pattern => {
    
    const regexPattern = pattern
      .replace(/\./g, '\\.')  
      .replace(/\*/g, '[^.]+'); 
    
    const regex = new RegExp(`^${regexPattern}($|\\.)`);
    return regex.test(path);
  });
}


export function getViewportPaths(paths: string[]): string[] {
  return paths.filter(isViewportSetting);
}