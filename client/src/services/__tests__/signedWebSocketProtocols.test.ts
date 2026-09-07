import { describe, expect, it, vi } from 'vitest';
import { signedWebSocketProtocols } from '../signedWebSocketProtocols';

describe('signed WebSocket upgrades', () => {
  it.each([
    ['wss://Public.Example:443/ws?room=a%2Fb', 'https://public.example/ws?room=a%2Fb'],
    ['ws://localhost:8080/ws', 'http://localhost:8080/ws'],
  ])('signs the browser-visible HTTP URL for %s', async (url, expected) => {
    const signer = { isAuthenticated: () => true, signRequest: vi.fn().mockResolvedValue('a+b/cA==') };
    expect(await signedWebSocketProtocols(url, signer)).toEqual(['visionclaw', 'nostr.a-b_cA']);
    expect(signer.signRequest).toHaveBeenCalledWith(expected, 'GET');
  });
  it('fails before socket construction for absent, declined or malformed signatures', async () => {
    const signRequest = vi.fn();
    await expect(signedWebSocketProtocols('wss://example/ws', { isAuthenticated: () => false, signRequest })).rejects.toThrow('unavailable');
    expect(signRequest).not.toHaveBeenCalled();
    signRequest.mockRejectedValueOnce(new Error('User rejected'));
    await expect(signedWebSocketProtocols('wss://example/ws', { isAuthenticated: () => true, signRequest })).rejects.toThrow('User rejected');
    signRequest.mockResolvedValue('');
    await expect(signedWebSocketProtocols('wss://example/ws', { isAuthenticated: () => true, signRequest })).rejects.toThrow('Invalid signed request');
  });
});
