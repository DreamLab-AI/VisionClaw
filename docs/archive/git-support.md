# Adding Git Support to a Solid Server

This guide explains how to add Git HTTP backend support to a Solid server, enabling `git clone` and `git push` operations on pod containers.

## Overview

The Git HTTP protocol allows clients to clone and push to repositories over HTTP. This is implemented using Git's built-in `git http-backend` CGI program - the same one used by Apache and Nginx.

### How It Works

```
┌─────────────┐     HTTP      ┌──────────────┐     CGI      ┌─────────────────┐
│  Git Client │ ─────────────▶│ Solid Server │ ────────────▶│ git http-backend│
│             │◀───────────── │              │◀──────────── │                 │
└─────────────┘               └──────────────┘              └─────────────────┘
```

**Clone flow:**
1. `GET /repo/info/refs?service=git-upload-pack` - Discovery
2. `POST /repo/git-upload-pack` - Fetch objects

**Push flow:**
1. `GET /repo/info/refs?service=git-receive-pack` - Discovery
2. `POST /repo/git-receive-pack` - Send objects

## Implementation

### 1. Detect Git Requests

Git protocol requests are identified by URL patterns:

```javascript
function isGitRequest(urlPath) {
  return urlPath.includes('/info/refs') ||
    urlPath.includes('/git-upload-pack') ||
    urlPath.includes('/git-receive-pack');
}

function isGitWriteOperation(urlPath) {
  return urlPath.includes('/git-receive-pack');
}
```

### 2. Security: Block Direct .git Access

**Important:** Git protocol requests should be allowed, but direct file access to `.git/` contents must be blocked:

```javascript
// BLOCK: Direct access to .git contents (security risk)
GET /.git/config         → 403 Forbidden
GET /.git/objects/abc123 → 403 Forbidden

// ALLOW: Git protocol (handled by git http-backend)
GET /repo/info/refs?service=git-upload-pack → 200 OK
POST /repo/git-upload-pack                   → 200 OK
```

### 3. Authorization with WAC

Check permissions before allowing git operations:

```javascript
// Clone/fetch requires Read access
// Push requires Write access

const needsWrite = isGitWriteOperation(request.url);
const requiredMode = needsWrite ? 'write' : 'read';

const { allowed } = await checkAccess({
  resourceUrl,
  resourcePath,
  agentWebId: request.webId,
  requiredMode
});

if (!allowed) {
  return reply.code(needsWrite ? 403 : 401).send({
    error: needsWrite ? 'Write access required' : 'Read access required'
  });
}
```

### 4. Git HTTP Backend Handler

The core handler spawns `git http-backend` with CGI environment variables:

```javascript
import { spawn } from 'child_process';

async function handleGit(request, reply) {
  const urlPath = decodeURIComponent(request.url.split('?')[0]);
  const queryString = request.url.split('?')[1] || '';

  // Build CGI environment
  const env = {
    ...process.env,
    GIT_PROJECT_ROOT: dataRoot,           // Where repos are stored
    GIT_HTTP_EXPORT_ALL: '',              // Allow read access
    GIT_HTTP_RECEIVE_PACK: 'true',        // Enable push
    PATH_INFO: urlPath,
    REQUEST_METHOD: request.method,
    CONTENT_TYPE: request.headers['content-type'] || '',
    QUERY_STRING: queryString,
    CONTENT_LENGTH: request.headers['content-length'] || '0',
  };

  // For non-bare repos, set GIT_DIR to .git subdirectory
  if (isRegularRepo) {
    env.GIT_DIR = path.join(repoPath, '.git');
  }

  // Spawn git http-backend
  const child = spawn('git', ['http-backend'], { env });

  // Send request body (for POST requests)
  if (request.body && request.body.length > 0) {
    child.stdin.write(request.body);
  }
  child.stdin.end();

  // Parse CGI response and send to client
  // ... (see full implementation below)
}
```

### 5. CGI Response Parsing

Git http-backend outputs CGI format (headers + body). Parse and forward:

```javascript
let buffer = Buffer.alloc(0);
let headersSent = false;

child.stdout.on('data', (data) => {
  buffer = Buffer.concat([buffer, data]);

  if (!headersSent) {
    // Find header/body separator (try both \r\n\r\n and \n\n)
    let headerEnd = buffer.indexOf('\r\n\r\n');
    let sep = '\r\n';
    let sepLen = 4;

    if (headerEnd === -1) {
      headerEnd = buffer.indexOf('\n\n');
      sep = '\n';
      sepLen = 2;
    }

    if (headerEnd !== -1) {
      const headerSection = buffer.subarray(0, headerEnd).toString();
      const bodySection = buffer.subarray(headerEnd + sepLen);

      // Parse CGI headers
      for (const line of headerSection.split(sep)) {
        const colonIdx = line.indexOf(':');
        if (colonIdx > 0) {
          const key = line.substring(0, colonIdx).trim();
          const value = line.substring(colonIdx + 1).trim();

          if (key.toLowerCase() === 'status') {
            statusCode = parseInt(value.split(' ')[0], 10);
          } else {
            reply.raw.setHeader(key, value);
          }
        }
      }

      reply.raw.writeHead(statusCode);
      reply.raw.write(bodySection);
      headersSent = true;
    }
  } else {
    reply.raw.write(buffer);
  }
  buffer = Buffer.alloc(0);
});

child.stdout.on('end', () => {
  reply.raw.end();
});
```

## Repository Setup

### Regular Repository (with working directory)

```bash
cd /path/to/pod/myrepo
git init
echo "# My Project" > README.md
git add .
git commit -m "Initial commit"
```

### Bare Repository (server-only, more efficient)

```bash
cd /path/to/pod
git init --bare myrepo.git
```

### ACL for Public Clone

Create `/path/to/pod/myrepo/.acl`:

```turtle
@prefix acl: <http://www.w3.org/ns/auth/acl#>.
@prefix foaf: <http://xmlns.com/foaf/0.1/>.

<#public>
    a acl:Authorization;
    acl:agentClass foaf:Agent;
    acl:accessTo <./>;
    acl:default <./>;
    acl:mode acl:Read.
```

### ACL for Authenticated Push

```turtle
@prefix acl: <http://www.w3.org/ns/auth/acl#>.
@prefix foaf: <http://xmlns.com/foaf/0.1/>.

<#owner>
    a acl:Authorization;
    acl:agent <https://alice.example.com/#me>;
    acl:accessTo <./>;
    acl:default <./>;
    acl:mode acl:Read, acl:Write, acl:Control.

<#public>
    a acl:Authorization;
    acl:agentClass foaf:Agent;
    acl:accessTo <./>;
    acl:default <./>;
    acl:mode acl:Read.
```

## Usage

### Server

```bash
# Start server with git support enabled
jss start --git

# Or via environment variable
JSS_GIT=true jss start
```

### Client

```bash
# Clone
git clone http://localhost:3000/myrepo

# Clone with authentication (if required)
git clone http://localhost:3000/myrepo
# Git will prompt for credentials

# Push (requires write access)
cd myrepo
echo "New content" >> README.md
git add .
git commit -m "Update readme"
git push
```

## Complete Handler Code

See `src/handlers/git.js` in the JSS repository for the full implementation.

## References

- [Git HTTP Protocol](https://git-scm.com/book/en/v2/Git-on-the-Server-Smart-HTTP)
- [git-http-backend documentation](https://git-scm.com/docs/git-http-backend)
- [CGI Specification](https://www.rfc-editor.org/rfc/rfc3875)
- [Web Access Control (WAC)](https://solidproject.org/TR/wac)

## Prior Art

- [nosdav/server](https://github.com/nosdav/server) - Git support implementation
- [QuitStore](https://github.com/AKSW/QuitStore) - Git + RDF versioning
