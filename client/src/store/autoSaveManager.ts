import { createLogger } from '../utils/loggerConfig';
import { settingsApi } from '../api/settingsApi';
import { settingsRetryManager } from './settingsRetryManager';
import { useSettingsSyncStatus, type UnsavedReason } from './settingsSyncStatus';
import { serverBucketForPath } from '../api/settings/schemaMappings';

interface BatchOperation {
  path: string;
  value: any;
}

const logger = createLogger('AutoSaveManager');


export class AutoSaveManager {
  private pendingChanges: Map<string, any> = new Map();
  private saveDebounceTimer: NodeJS.Timeout | null = null;
  private isInitialized: boolean = false;
  private _syncEnabled: boolean = true;
  private readonly DEBOUNCE_DELAY = 500;

  /** Whether settings changes are synced to the server (true) or local-only (false) */
  get syncEnabled(): boolean { return this._syncEnabled; }
  setSyncEnabled(enabled: boolean) {
    this._syncEnabled = enabled;
    logger.info(`Settings sync ${enabled ? 'ENABLED' : 'DISABLED (local-only mode)'}`);
    if (!enabled) {
      if (this.saveDebounceTimer) {
        clearTimeout(this.saveDebounceTimer);
        this.saveDebounceTimer = null;
      }
      // Pending changes stay in this browser only: say so rather than dropping them silently.
      this.markUnsaved([...this.pendingChanges.keys()], 'sync-off');
      this.pendingChanges.clear();
    }
  }

  private readonly CLIENT_ONLY_PATHS = [
    'auth.nostr.connected',
    'auth.nostr.publicKey',
  ];


  private isClientOnlyPath(path: string): boolean {
    return this.CLIENT_ONLY_PATHS.some(clientPath =>
      path === clientPath || path.startsWith(clientPath + '.')
    );
  }

  setInitialized(initialized: boolean) {
    logger.debug(`[SETTINGS-DIAG] autoSaveManager.setInitialized(${initialized})`);
    this.isInitialized = initialized;
    // Changes made before the store finished loading were held, not dropped: send them now.
    if (initialized && this._syncEnabled && this.pendingChanges.size > 0) this.scheduleFlush();
  }

  /** Record changes the server does not have, so the user sees them and a reload keeps them. */
  private markUnsaved(paths: string[], reason: UnsavedReason) {
    const serverPaths = paths.filter((p) => serverBucketForPath(p) !== null);
    if (serverPaths.length === 0) return;
    useSettingsSyncStatus.getState().markUnsaved(serverPaths, reason);
    logger.warn(`Settings not saved to the server (${reason}): ${serverPaths.join(', ')}`);
  }

  queueChange(path: string, value: any) {
    this.queueChanges(new Map([[path, value]]));
  }


  queueChanges(changes: Map<string, any>) {
    const paths = [...changes.keys()].filter((path) => !this.isClientOnlyPath(path));
    if (paths.length === 0) return;
    if (!this._syncEnabled) {
      this.markUnsaved(paths, 'sync-off');
      return;
    }
    for (const path of paths) this.pendingChanges.set(path, changes.get(path));
    if (!this.isInitialized) {
      // Held until setInitialized(true); flagged meanwhile.
      this.markUnsaved(paths, 'not-ready');
      return;
    }
    logger.debug(`[SETTINGS-DIAG] autoSaveManager.queueChanges: ${paths.length} changes`, paths);
    this.scheduleFlush();
  }


  private scheduleFlush() {
    if (this.saveDebounceTimer) {
      clearTimeout(this.saveDebounceTimer);
    }

    this.saveDebounceTimer = setTimeout(() => {
      this.flushPendingChanges();
    }, this.DEBOUNCE_DELAY);
  }


  async forceFlush(): Promise<void> {
    if (this.saveDebounceTimer) {
      clearTimeout(this.saveDebounceTimer);
      this.saveDebounceTimer = null;
    }
    await this.flushPendingChanges();
  }


  private async flushPendingChanges(): Promise<void> {
    if (this.pendingChanges.size === 0) return;

    const updates: BatchOperation[] = Array.from(this.pendingChanges.entries())
      .map(([path, value]) => ({ path, value }));

    // Clear pending immediately to avoid re-sending on next flush
    this.pendingChanges.clear();

    logger.debug(`[SETTINGS-DIAG] autoSaveManager.flush: ${updates.length} updates`, updates.map(u => `${u.path}=${JSON.stringify(u.value)}`));

    try {
      await settingsApi.updateSettingsByPaths(updates);
      logger.debug(`[SETTINGS-DIAG] autoSaveManager.flush SUCCESS: ${updates.length} updates sent to server`);
      useSettingsSyncStatus.getState().markSaved(updates.map((u) => u.path));
    } catch (error) {
      this.markUnsaved(updates.map((u) => u.path), 'failed');

      // Delegate all failed updates to the centralized retry manager
      for (const { path, value } of updates) {
        settingsRetryManager.addFailedUpdate(
          path,
          value,
          error instanceof Error ? error.message : 'Auto-save flush failed'
        );
      }
    }
  }


  hasPendingChanges(): boolean {
    return this.pendingChanges.size > 0;
  }


  getPendingCount(): number {
    return this.pendingChanges.size;
  }
}

// Export singleton instance
export const autoSaveManager = new AutoSaveManager();
