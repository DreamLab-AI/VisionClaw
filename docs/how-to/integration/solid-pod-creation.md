# Solid Pod Creation Flow

## Overview

VisionClaw integrates with JavaScript Solid Server (JSS) to provide user-owned data pods for ontology contributions, personal preferences, and agent memory. This document describes the pod creation and management flow.

## Architecture

```
React Frontend ────► VisionClaw (Rust) ────► JSS (Fastify)
     │                     │                     │
     │ Nostr Auth          │ NIP-98 Forward      │ WebID Creation
     │                     │ /solid/* proxy      │ Pod Provisioning
     └─────────────────────┴─────────────────────┘
```

## Pod URL Structure

User pods follow this URL pattern:
```
/pods/{npub}/
```

Where `{npub}` is the Nostr bech32-encoded public key (e.g., `npub1abc123...`).

## Auto-Provisioning Flow

### 1. User Authentication
User authenticates via existing Nostr flow (NIP-07 browser extension or NIP-46 remote signer).

### 2. Pod Existence Check
VisionClaw checks if the user's pod exists:

```bash
# Server-side check
HEAD /pods/{npub}/
```

- **200 OK**: Pod exists, proceed to access
- **404 Not Found**: Pod needs creation

### 3. Pod Creation Request
If pod doesn't exist, VisionClaw creates it:

```bash
# Create pod with Nostr identity
POST /.pods
Content-Type: application/json
Authorization: Bearer {NIP-98-token}

{
  "name": "{npub}",
  "webId": "did:nostr:{hex-pubkey}",
  "template": "visionclaw-user"
}
```

### 4. Default Pod Structure

JSS creates the following structure:

```
/pods/{npub}/
├── profile/
│   └── card                    # WebID document with Nostr key
├── ontology/
│   ├── contributions/          # User's ontology additions
│   ├── proposals/              # Pending proposals for review
│   └── annotations/            # Comments on public ontology
├── preferences/
│   └── visionclaw.ttl          # App settings (theme, layout, etc.)
├── inbox/                      # Notifications and messages
└── .acl                        # Access control list
```

### 5. WebID Document

The profile card contains the user's WebID:

```turtle
@prefix foaf: <http://xmlns.com/foaf/0.1/>.
@prefix solid: <http://www.w3.org/ns/solid/terms#>.
@prefix nostr: <https://nostr.com/ns#>.

<#me>
    a foaf:Person;
    foaf:name "Anonymous Nostr User";
    nostr:pubkey "npub1abc123...";
    nostr:hexPubkey "abc123...";
    solid:account </pods/{npub}/>;
    solid:privateTypeIndex </pods/{npub}/settings/privateTypeIndex.ttl>;
    solid:publicTypeIndex </pods/{npub}/settings/publicTypeIndex.ttl>.
```

## Access Control

### Default ACL Policy

```turtle
@prefix acl: <http://www.w3.org/ns/auth/acl#>.

# Owner has full control
<#owner>
    a acl:Authorization;
    acl:agent <profile/card#me>;
    acl:accessTo <./>;
    acl:default <./>;
    acl:mode acl:Read, acl:Write, acl:Control.

# Public read access to contributions
<#public>
    a acl:Authorization;
    acl:agentClass foaf:Agent;
    acl:accessTo <ontology/contributions/>;
    acl:mode acl:Read.
```

### Proposal Workflow ACLs

When a user submits an ontology proposal:

1. User writes to `/pods/{npub}/ontology/proposals/{proposal-id}.ttl`
2. Proposal visible to maintainers via shared ACL
3. Maintainer reviews and either:
   - Merges to `/public/ontology/`
   - Returns with comments to user's inbox

## API Endpoints

### Check Pod Status
```
GET /api/solid/pod/status
Authorization: Bearer {session-token}

Response:
{
  "exists": true,
  "podUrl": "/pods/npub1abc.../",
  "webId": "did:nostr:abc123...",
  "storage": {
    "used": 1024000,
    "quota": 104857600
  }
}
```

### Create Pod
```
POST /api/solid/pod/create
Authorization: Bearer {session-token}

Response:
{
  "success": true,
  "podUrl": "/pods/npub1abc.../",
  "webId": "/pods/npub1abc.../profile/card#me"
}
```

### List Pod Contents
```
GET /api/solid/pod/list?path=/ontology/contributions/
Authorization: Bearer {session-token}

Response:
{
  "contents": [
    {"name": "my-class.ttl", "type": "file", "modified": "2025-12-31T12:00:00Z"},
    {"name": "properties/", "type": "container", "modified": "2025-12-30T10:00:00Z"}
  ]
}
```

## Frontend Integration

### SolidPodService.ts

```typescript
import { useNostrAuth } from '@/hooks/useNostrAuth';

export class SolidPodService {
  private baseUrl = '/solid';

  async ensurePodExists(): Promise<PodInfo> {
    const { pubkey } = useNostrAuth();
    const npub = nip19.npubEncode(pubkey);

    // Check if pod exists
    const response = await fetch(`${this.baseUrl}/pods/${npub}/`, {
      method: 'HEAD',
      headers: await this.getAuthHeaders()
    });

    if (response.status === 404) {
      return this.createPod(npub);
    }

    return this.getPodInfo(npub);
  }

  async createPod(npub: string): Promise<PodInfo> {
    const response = await fetch(`${this.baseUrl}/.pods`, {
      method: 'POST',
      headers: {
        ...await this.getAuthHeaders(),
        'Content-Type': 'application/json'
      },
      body: JSON.stringify({
        name: npub,
        template: 'visionclaw-user'
      })
    });

    return response.json();
  }

  private async getAuthHeaders(): Promise<Headers> {
    const nip98Token = await generateNip98Token(
      `${window.location.origin}${this.baseUrl}`,
      'POST'
    );
    return {
      'Authorization': `Nostr ${nip98Token}`
    };
  }
}
```

## Nginx Reverse Proxy

The `/solid/*` routes are proxied to JSS via nginx:

```nginx
# /solid/* -> JSS (already configured in nginx.conf)
location /solid/ {
    proxy_pass http://jss/;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;

    # Pass through Nostr auth
    proxy_set_header Authorization $http_authorization;

    # WebSocket support for notifications
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
}

# /pods/* -> JSS pods endpoint
location /pods/ {
    proxy_pass http://jss/pods/;
    # ... same headers as above
}
```

## Docker Integration

JSS is included in `docker-compose.unified.yml`:

```yaml
jss:
  container_name: visionclaw-jss
  build:
    context: ./JavaScriptSolidServer
    dockerfile: Dockerfile.jss
  environment:
    - JSS_PORT=3030
    - JSS_NOTIFICATIONS=true
    - JSS_CONNEG=true
    - JSS_MULTIUSER=true
    - JSS_POD_TEMPLATE=visionclaw
  volumes:
    - jss-data:/data
    - ./ontology:/data/public/ontology:ro  # Public ontology
  healthcheck:
    test: ["CMD", "curl", "-f", "http://localhost:3030/.well-known/solid"]
    interval: 30s
    timeout: 10s
    retries: 3
  networks:
    - visionclaw_network
```

## Security Considerations

1. **NIP-98 Token Validation**: All requests to JSS include NIP-98 tokens with 60-second expiry
2. **Pod Isolation**: Each user's pod is isolated via WAC (Web Access Control)
3. **Rate Limiting**: Pod creation is rate-limited to prevent abuse
4. **Quota Management**: Storage quotas prevent resource exhaustion
5. **HTTPS**: All production traffic uses TLS via Cloudflare

## Related Documentation

- [Solid Integration](solid-integration.md)
- [Nostr Authentication](../features/nostr-auth.md)
- [Security Model](../../explanation/security-model.md)
