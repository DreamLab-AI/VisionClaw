// frontend/src/api/settingsApi.ts
// Facade — re-exports from split modules under api/settings/
// Public API is unchanged; all callers continue to import from this file.

export type {
  PhysicsSettings,
  ConstraintSettings,
  RenderingSettings,
  NodeFilterSettings,
  QualityGateSettings,
  AllSettings,
  SettingsProfile,
  SaveProfileRequest,
  ProfileIdResponse,
  ErrorResponse,
  PriorityWeighting,
} from './settings/types';

export { DEFAULT_PHYSICS_SETTINGS } from './settings/defaults';

export { clamp, validatePhysicsSettings, validateConstraintSettings } from './settings/validators';

export {
  getPhysics,
  updatePhysics,
  getConstraints,
  updateConstraints,
  getRendering,
  updateRendering,
  getNodeFilter,
  updateNodeFilter,
  getQualityGates,
  updateQualityGates,
  toggleOntologyPhysics,
  configureOntologyWeights,
  getVisualSettings,
  updateVisualSettings,
  getAll,
  getSettingsByPaths,
  getSettingByPath,
  updateSettingByPath,
  updateSettingsByPaths,
  saveProfile,
  listProfiles,
  loadProfile,
  deleteProfile,
  resetSettings,
  exportSettings,
  importSettings,
  flushPendingUpdates,
} from './settings/endpoints';

// ============================================================================
// settingsApi object — preserves the named-object import used by most callers
// ============================================================================

import {
  getPhysics,
  updatePhysics,
  getConstraints,
  updateConstraints,
  getRendering,
  updateRendering,
  getNodeFilter,
  updateNodeFilter,
  getQualityGates,
  updateQualityGates,
  toggleOntologyPhysics,
  configureOntologyWeights,
  getVisualSettings,
  updateVisualSettings,
  getAll,
  getSettingsByPaths,
  getSettingByPath,
  updateSettingByPath,
  updateSettingsByPaths,
  saveProfile,
  listProfiles,
  loadProfile,
  deleteProfile,
  resetSettings,
  exportSettings,
  importSettings,
  flushPendingUpdates,
} from './settings/endpoints';

export const settingsApi = {
  getPhysics,
  updatePhysics,
  getConstraints,
  updateConstraints,
  getRendering,
  updateRendering,
  getNodeFilter,
  updateNodeFilter,
  getQualityGates,
  updateQualityGates,
  toggleOntologyPhysics,
  configureOntologyWeights,
  getVisualSettings,
  updateVisualSettings,
  getAll,
  getSettingsByPaths,
  getSettingByPath,
  updateSettingByPath,
  updateSettingsByPaths,
  saveProfile,
  listProfiles,
  loadProfile,
  deleteProfile,
  resetSettings,
  exportSettings,
  importSettings,
  flushPendingUpdates,
};
