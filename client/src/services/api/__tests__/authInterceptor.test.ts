import { describe, it, expect, beforeEach, vi } from 'vitest';

// Mock dependencies - vi.mock factories cannot reference outer variables because
// they are hoisted. Use vi.hoisted() to create shared mocks.
const { mockNostrAuth } = vi.hoisted(() => ({
  mockNostrAuth: {
    isAuthenticated: vi.fn(() => false),
    getCurrentUser: vi.fn(() => null),
    isDevMode: vi.fn(() => false),
    getDevSessionToken: vi.fn(() => 'dev-session-token'),
    signRequest: vi.fn(),
    onAuthStateChanged: vi.fn(() => () => {}),
  },
}));

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));

vi.mock('../../nostrAuthService', () => ({
  nostrAuth: mockNostrAuth,
}));

vi.mock('uuid', () => ({
  v4: vi.fn(() => 'test-uuid-1234'),
}));

import { authRequestInterceptor, authResponseInterceptor, generateRequestId, initializeAuthInterceptor, setupAuthStateListener } from '../authInterceptor';
import type { RequestConfig } from '../UnifiedApiClient';

describe('authInterceptor', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockNostrAuth.isAuthenticated.mockReturnValue(false);
    mockNostrAuth.getCurrentUser.mockReturnValue(null);
    mockNostrAuth.isDevMode.mockReturnValue(false);
  });

  describe('generateRequestId', () => {
    it('should return a UUID string', () => {
      const id = generateRequestId();
      expect(id).toBe('test-uuid-1234');
    });
  });

  describe('authRequestInterceptor', () => {
    it('should add X-Request-ID header to all requests', async () => {
      const config: RequestConfig = {};
      const result = await authRequestInterceptor(config, '/api/test');
      expect(result.headers).toBeDefined();
      expect((result.headers as Record<string, string>)['X-Request-ID']).toBe('test-uuid-1234');
    });

    it('should not add auth headers when not authenticated', async () => {
      mockNostrAuth.isAuthenticated.mockReturnValue(false);
      const config: RequestConfig = {};
      const result = await authRequestInterceptor(config, '/api/test');
      const headers = result.headers as Record<string, string>;
      expect(headers['Authorization']).toBeUndefined();
    });

    it('should add dev-mode Bearer token in dev mode', async () => {
      mockNostrAuth.isAuthenticated.mockReturnValue(true);
      mockNostrAuth.isDevMode.mockReturnValue(true);
      mockNostrAuth.getCurrentUser.mockReturnValue({ pubkey: 'devpubkey123', isPowerUser: true });

      const config: RequestConfig = {};
      const result = await authRequestInterceptor(config, '/api/test');
      const headers = result.headers as Record<string, string>;

      expect(headers['Authorization']).toBe('Bearer dev-session-token');
      expect(headers['X-Nostr-Pubkey']).toBe('devpubkey123');
    });

    it('should add NIP-98 Nostr auth header in production mode', async () => {
      mockNostrAuth.isAuthenticated.mockReturnValue(true);
      mockNostrAuth.isDevMode.mockReturnValue(false);
      mockNostrAuth.getCurrentUser.mockReturnValue({ pubkey: 'prodpubkey456', isPowerUser: false });
      mockNostrAuth.signRequest.mockResolvedValue('base64signedtoken');

      const config: RequestConfig = { method: 'POST', body: '{"hello":"world"}' };
      const result = await authRequestInterceptor(config, '/api/data');
      const headers = result.headers as Record<string, string>;

      expect(headers['Authorization']).toBe('Nostr base64signedtoken');
      expect(mockNostrAuth.signRequest).toHaveBeenCalledWith(
        expect.stringContaining('/api/data'),
        'POST',
        '{"hello":"world"}'
      );
    });

    it('should handle NIP-98 signing failure gracefully', async () => {
      mockNostrAuth.isAuthenticated.mockReturnValue(true);
      mockNostrAuth.isDevMode.mockReturnValue(false);
      mockNostrAuth.getCurrentUser.mockReturnValue({ pubkey: 'pk', isPowerUser: false });
      mockNostrAuth.signRequest.mockRejectedValue(new Error('signing failed'));

      const config: RequestConfig = {};
      const result = await authRequestInterceptor(config, '/api/test');
      const headers = result.headers as Record<string, string>;

      // Should not crash; Authorization header should not be set
      expect(headers['Authorization']).toBeUndefined();
    });

    it('should default to GET method when not specified', async () => {
      mockNostrAuth.isAuthenticated.mockReturnValue(true);
      mockNostrAuth.isDevMode.mockReturnValue(false);
      mockNostrAuth.getCurrentUser.mockReturnValue({ pubkey: 'pk2', isPowerUser: false });
      mockNostrAuth.signRequest.mockResolvedValue('token');

      const config: RequestConfig = {}; // no method
      await authRequestInterceptor(config, '/api/resource');

      expect(mockNostrAuth.signRequest).toHaveBeenCalledWith(
        expect.any(String),
        'GET',
        undefined
      );
    });

    it('should not mutate the original config object', async () => {
      const config: RequestConfig = { headers: { 'Existing': 'header' } };
      const result = await authRequestInterceptor(config, '/api/test');
      expect(result).not.toBe(config); // shallow copy
    });

    it('should not add X-Nostr-Pubkey when user has no pubkey in dev mode', async () => {
      mockNostrAuth.isAuthenticated.mockReturnValue(true);
      mockNostrAuth.isDevMode.mockReturnValue(true);
      mockNostrAuth.getCurrentUser.mockReturnValue({ pubkey: '', isPowerUser: true });

      const config: RequestConfig = {};
      const result = await authRequestInterceptor(config, '/api/test');
      const headers = result.headers as Record<string, string>;
      expect(headers['X-Nostr-Pubkey']).toBeUndefined();
    });
  });

  describe('initializeAuthInterceptor', () => {
    it('should call setInterceptors on the api client', () => {
      const mockClient = { setInterceptors: vi.fn() };
      initializeAuthInterceptor(mockClient);
      // ADR-06 §D1: both request signing and the release-mode 401 response
      // detector are registered.
      expect(mockClient.setInterceptors).toHaveBeenCalledWith({
        onRequest: authRequestInterceptor,
        onResponse: authResponseInterceptor,
        // ADR-2205: re-bind hook for the distinct session_key_expired 401.
        onSessionKeyExpired: expect.any(Function),
      });
    });
  });

  describe('setupAuthStateListener', () => {
    it('should subscribe to auth state changes', () => {
      setupAuthStateListener();
      expect(mockNostrAuth.onAuthStateChanged).toHaveBeenCalledWith(expect.any(Function));
    });
  });
});
