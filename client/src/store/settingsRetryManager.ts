import { createLogger } from '../utils/loggerConfig';
import { settingsApi } from '../api/settingsApi';
import { SettingsPath } from '../features/settings/config/settings';
import { useSettingsSyncStatus } from './settingsSyncStatus';

interface BatchOperation {
  path: SettingsPath;
  value: any;
}

const logger = createLogger('SettingsRetryManager');

interface RetryableUpdate {
  path: SettingsPath;
  value: any;
  attempts: number;
  lastAttempt: number;
  error?: string;
}

export class SettingsRetryManager {
  private retryQueue: Map<SettingsPath, RetryableUpdate> = new Map();
  private retryInterval: number | null = null;
  private maxRetries = 3;
  private baseRetryDelay = 1000;
  private maxRetryDelay = 30000;

  constructor() {
    // Don't start the processor eagerly -- it auto-starts when items are added
  }


  addFailedUpdate(path: SettingsPath, value: any, error?: string) {
    const existing = this.retryQueue.get(path);

    if (existing && existing.attempts >= this.maxRetries) {
      logger.warn(`Max retries reached for path: ${path}`);
      return;
    }

    this.retryQueue.set(path, {
      path,
      value,
      attempts: existing ? existing.attempts + 1 : 1,
      lastAttempt: Date.now(),
      error
    });

    logger.info(`Added failed update to retry queue: ${path} (attempt ${this.retryQueue.get(path)!.attempts})`);

    // Auto-start the processor if it was stopped (e.g. queue was previously empty)
    this.startRetryProcessor();
  }


  private async processRetryQueue() {
    if (this.retryQueue.size === 0) {
      // Auto-stop the interval when there is nothing to retry
      this.stopRetryProcessor();
      return;
    }

    const now = Date.now();
    const updates: RetryableUpdate[] = [];


    this.retryQueue.forEach((update) => {
      const delay = this.calculateRetryDelay(update.attempts);
      if (now - update.lastAttempt >= delay) {
        updates.push(update);
      }
    });

    if (updates.length === 0) return;

    logger.info(`Processing ${updates.length} retryable updates`);


    if (updates.length > 1) {
      const batchOps: BatchOperation[] = updates.map(u => ({
        path: u.path,
        value: u.value
      }));

      try {
        // Use updateSettingsByPaths instead of non-existent batchUpdateSettings
        await settingsApi.updateSettingsByPaths(batchOps.map(op => ({ path: op.path, value: op.value })));

        // On success, remove all paths from retry queue
        useSettingsSyncStatus.getState().markSaved(batchOps.map((op) => op.path));
        batchOps.forEach(({ path }: { path: SettingsPath }) => {
          this.retryQueue.delete(path);
          logger.info(`Successfully retried update for path: ${path}`);
        });

      } catch (error: any) {
        logger.error('Batch retry failed:', error);

        updates.forEach(update => {
          update.lastAttempt = Date.now();
          update.error = error instanceof Error ? error.message : 'Unknown error';
        });
      }
    } else {

      const update = updates[0];
      try {
        await settingsApi.updateSettingByPath(update.path, update.value);
        this.retryQueue.delete(update.path);
        useSettingsSyncStatus.getState().markSaved([update.path]);
        logger.info(`Successfully retried update for path: ${update.path}`);
      } catch (error) {
        update.lastAttempt = Date.now();
        update.error = error instanceof Error ? error.message : 'Unknown error';
        logger.error(`Retry failed for path ${update.path}:`, error);
      }
    }


    this.retryQueue.forEach((update, path) => {
      if (update.attempts >= this.maxRetries) {
        logger.error(`Giving up on path ${path} after ${update.attempts} attempts`);
        this.retryQueue.delete(path);


        window.dispatchEvent(new CustomEvent('settings-retry-failed', {
          detail: { path, value: update.value, error: update.error }
        }));
      }
    });
  }


  private calculateRetryDelay(attempts: number): number {
    const delay = Math.min(
      this.baseRetryDelay * Math.pow(2, attempts - 1),
      this.maxRetryDelay
    );

    return delay + Math.random() * 1000;
  }


  private startRetryProcessor() {
    if (this.retryInterval) return;

    this.retryInterval = window.setInterval(() => {
      this.processRetryQueue().catch(error => {
        logger.error('Error in retry processor:', error);
      });
    }, 5000);
  }


  stopRetryProcessor() {
    if (this.retryInterval) {
      clearInterval(this.retryInterval);
      this.retryInterval = null;
    }
  }


  getRetryStatus(): {
    queueSize: number;
    items: Array<{ path: string; attempts: number; error?: string }>
  } {
    const items = Array.from(this.retryQueue.entries()).map(([path, update]) => ({
      path,
      attempts: update.attempts,
      error: update.error
    }));

    return {
      queueSize: this.retryQueue.size,
      items
    };
  }


  clearRetryQueue() {
    this.retryQueue.clear();
    logger.info('Retry queue cleared');
  }


  async retryPath(path: SettingsPath): Promise<boolean> {
    const update = this.retryQueue.get(path);
    if (!update) return false;

    try {
      await settingsApi.updateSettingByPath(update.path, update.value);
      this.retryQueue.delete(path);
      useSettingsSyncStatus.getState().markSaved([path]);
      logger.info(`Successfully retried update for path: ${path}`);
      return true;
    } catch (error) {
      update.lastAttempt = Date.now();
      update.attempts++;
      update.error = error instanceof Error ? error.message : 'Unknown error';
      logger.error(`Immediate retry failed for path ${path}:`, error);
      return false;
    }
  }
}

// Export singleton instance
export const settingsRetryManager = new SettingsRetryManager();

// HMR cleanup: stop interval on hot-module dispose to prevent leaked timers
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    settingsRetryManager.stopRetryProcessor();
  });
}
