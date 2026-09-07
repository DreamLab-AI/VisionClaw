interface RequestSigner {
  isAuthenticated(): boolean;
  signRequest(url: string, method: string): Promise<string>;
}

/** Sign the HTTP upgrade URL; the server negotiates only the public protocol. */
export async function signedWebSocketProtocols(url: string, signer: RequestSigner): Promise<string[]> {
  if (!signer.isAuthenticated()) throw new Error('WebSocket request signer unavailable');
  const target = new URL(url);
  if (!['ws:', 'wss:'].includes(target.protocol) || target.username || target.password || target.hash) {
    throw new Error('Invalid WebSocket upgrade URL');
  }
  target.protocol = target.protocol === 'wss:' ? 'https:' : 'http:';
  const token = await signer.signRequest(target.href, 'GET');
  if (!token || !/^[A-Za-z0-9+/]+={0,2}$/.test(token)) throw new Error('Invalid signed request');
  return ['visionclaw', `nostr.${token.replace(/=+$/, '').replace(/\+/g, '-').replace(/\//g, '_')}`];
}
