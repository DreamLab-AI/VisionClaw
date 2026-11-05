import { describe, it, expect, vi } from 'vitest';
const { processFrame } = vi.hoisted(() => ({ processFrame: vi.fn().mockResolvedValue(undefined) }));
vi.mock('../graphWorkerProxy', () => ({ graphWorkerProxy: { processBinaryFrame: processFrame } }));
vi.mock('../../../../utils/loggerConfig', () => ({ createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }), createErrorMetadata: (e: unknown) => e }));
vi.mock('../../../../utils/clientDebugState', () => ({ debugState: { isEnabled: () => false, isDataDebugEnabled: () => false } }));
vi.mock('../../../../store/settingsStore', () => ({ useSettingsStore: { getState: () => ({ settings: {} }) } }));
vi.mock('../../../../store/workerErrorStore', () => ({ useWorkerErrorStore: { getState: () => ({ resetTransientErrors: vi.fn(), recordTransientError: vi.fn() }) } }));
import { handleBinaryFrame } from './wsClient';
describe('ack-paced frame application', () => {
  it('applies consecutive chunks inside 16ms and propagates failure to the ack caller', async () => {
    vi.spyOn(Date, 'now').mockReturnValue(1000);
    const frame = new ArrayBuffer(61);
    await handleBinaryFrame(frame, 1000, vi.fn());
    await handleBinaryFrame(frame, 1000, vi.fn());
    expect(processFrame).toHaveBeenCalledTimes(2);
    processFrame.mockRejectedValueOnce(new Error('worker failed'));
    await expect(handleBinaryFrame(frame, 1000, vi.fn())).rejects.toThrow('worker failed');
    vi.restoreAllMocks();
  });
});
