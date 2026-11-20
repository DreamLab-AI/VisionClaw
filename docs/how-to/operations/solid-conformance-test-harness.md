# Running Solid Conformance Test Harness (CTH)

> Note 2026-11-20: the Node JSS server this guide starts (`bin/jss.js`, `src/server.js`) and its `test/` suite were removed (CD-F6). The commands below no longer run as written; point the harness at the embedded solid-pod server instead.

Step-by-step instructions for running the Solid Conformance Test Harness against this server.

## Prerequisites

- Node.js 18+
- Docker
- Port 4000 available

## Quick Start

```bash
# 1. Kill any existing server on port 4000
fuser -k 4000/tcp 2>/dev/null || true

# 2. Clean data directory
rm -rf data && mkdir data

# 3. Start server with IdP and content negotiation
JSS_PORT=4000 JSS_CONNEG=true JSS_IDP=true node bin/jss.js start &

# 4. Wait for server to be ready
sleep 3
curl -s http://localhost:4000/ > /dev/null && echo "Server ready"

# 5. Create test users
curl -s -X POST http://localhost:4000/.pods \
  -H "Content-Type: application/json" \
  -d '{"name": "alice", "email": "alice@example.com", "password": "alicepassword123"}'

curl -s -X POST http://localhost:4000/.pods \
  -H "Content-Type: application/json" \
  -d '{"name": "bob", "email": "bob@example.com", "password": "bobpassword123"}'

# 6. Create test container (required by CTH)
mkdir -p data/alice/cth-test

# 7. Run authentication tests (assumes test-subjects.ttl and cth.env exist - see Configuration Files below)
docker run --rm --network=host \
  -v $(pwd)/test-subjects.ttl:/app/test-subjects.ttl \
  --env-file cth.env \
  -e SUBJECTS=/app/test-subjects.ttl \
  solidproject/conformance-test-harness:latest \
  --target="https://github.com/solid/conformance-test-harness/jss" \
  --filter="authentication"
```

## Configuration Files

### Test Subjects File (test-subjects.ttl)

Create `test-subjects.ttl`:

```turtle
@base <https://github.com/solid/conformance-test-harness/> .
@prefix solid-test: <https://github.com/solid/conformance-test-harness/vocab#> .
@prefix doap: <http://usefulinc.com/ns/doap#> .
@prefix earl: <http://www.w3.org/ns/earl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

<jss>
    a earl:Software, earl:TestSubject ;
    doap:name "JavaScript Solid Server"@en ;
    doap:release <jss#test-subject-release> ;
    doap:developer <https://github.com/JavaScriptSolidServer> ;
    doap:homepage <https://github.com/JavaScriptSolidServer/JavaScriptSolidServer> ;
    doap:description "A minimal, fast, JSON-LD native Solid server."@en ;
    doap:programming-language "JavaScript"@en ;
    solid-test:skip "acp", "wac", "wac-allow-public" .

<jss#test-subject-release>
    doap:revision "0.0.14"@en ;
    doap:created "2025-12-27"^^xsd:date .
```

### Environment File (cth.env)

Create `cth.env`:

```bash
SERVER_ROOT=http://localhost:4000
TEST_CONTAINER=/alice/cth-test/
RESOURCE_SERVER_ROOT=http://localhost:4000
LOGIN_ENDPOINT=http://localhost:4000/idp/credentials
SOLID_IDENTITY_PROVIDER=http://localhost:4000/
USERS_ALICE_IDP=http://localhost:4000/
USERS_BOB_IDP=http://localhost:4000/
USERS_ALICE_WEBID=http://localhost:4000/alice/#me
USERS_BOB_WEBID=http://localhost:4000/bob/#me
USERS_ALICE_USERNAME=alice@example.com
USERS_ALICE_PASSWORD=alicepassword123
USERS_BOB_USERNAME=bob@example.com
USERS_BOB_PASSWORD=bobpassword123
```

### Environment Variables Reference

| Variable | Description | Example |
|----------|-------------|---------|
| `SERVER_ROOT` | Server base URL | `http://localhost:4000` |
| `TEST_CONTAINER` | Path to test container | `/alice/cth-test/` |
| `SOLID_IDENTITY_PROVIDER` | IdP issuer URL (with trailing slash) | `http://localhost:4000/` |
| `USERS_ALICE_IDP` | Alice's IdP | `http://localhost:4000/` |
| `USERS_ALICE_WEBID` | Alice's WebID | `http://localhost:4000/alice/#me` |
| `USERS_ALICE_USERNAME` | Alice's email | `alice@example.com` |
| `USERS_ALICE_PASSWORD` | Alice's password | `alicepassword123` |
| `USERS_BOB_IDP` | Bob's IdP | `http://localhost:4000/` |
| `USERS_BOB_WEBID` | Bob's WebID | `http://localhost:4000/bob/#me` |
| `USERS_BOB_USERNAME` | Bob's email | `bob@example.com` |
| `USERS_BOB_PASSWORD` | Bob's password | `bobpassword123` |
| `SUBJECTS` | Path to test-subjects.ttl inside container | `/app/test-subjects.ttl` |

## Running Specific Test Suites

### Authentication Tests (6 scenarios)

```bash
docker run --rm --network=host \
  --env-file cth.env \
  -v $(pwd)/test-subjects.ttl:/app/test-subjects.ttl \
  -e SUBJECTS=/app/test-subjects.ttl \
  solidproject/conformance-test-harness:latest \
  --target="https://github.com/solid/conformance-test-harness/jss" \
  --filter="authentication"
```

**Expected result:** 6/6 scenarios passing

### All Protocol Tests

```bash
docker run --rm --network=host \
  --env-file cth.env \
  -v $(pwd)/test-subjects.ttl:/app/test-subjects.ttl \
  -e SUBJECTS=/app/test-subjects.ttl \
  solidproject/conformance-test-harness:latest \
  --target="https://github.com/solid/conformance-test-harness/jss"
```

## Interpreting Results

### Success Output

```
scenarios:  6 | passed:  6 | failed:  0 | time: 0.6349
  MustFeatures  passed: 1, failed: 0
  MustScenarios passed: 6, failed: 0
```

### Failure Output

```
scenarios:  6 | passed:  4 | failed:  2 | time: 0.7952
Then status 401
status code was: 200, expected: 401, response time in milliseconds: 15
```

## Troubleshooting

### "Cannot get ACL url for root test container"

The test container doesn't exist. Create it:

```bash
mkdir -p data/alice/cth-test
```

### "Failed to read WebID Document" (401)

The WebID profile is not publicly readable. Check that the pod's ACL allows public read on the container itself (but not necessarily on children).

### "NullPointerException" during authentication

Usually means the IdP isn't returning proper responses. Check:
1. Server is running with `--idp` flag
2. Issuer URL has trailing slash
3. Users were created with email and password

### "DPoP htu mismatch"

URL mismatch in DPoP proof validation. Check that issuer URL doesn't have double slashes.

### Token format errors

Ensure the server returns JWT access tokens with:
- `aud: "solid"` claim
- 3-part JWT format (header.payload.signature)
- `webid` claim

## Server Requirements for CTH

The server must support:

1. **Solid-OIDC Identity Provider**
   - OIDC discovery at `/.well-known/openid-configuration`
   - JWKS at `/.well-known/jwks.json`
   - Dynamic client registration at `/idp/reg`
   - Credentials endpoint at `/idp/credentials` (for programmatic login)

2. **DPoP Token Binding**
   - RS256 and ES256 algorithms
   - Proper `cnf.jkt` claim in tokens

3. **WWW-Authenticate Header**
   - 401 responses must include `WWW-Authenticate: DPoP realm="..."`

4. **Container Creation via PUT**
   - PUT to path ending with `/` creates container

5. **ACL Inheritance**
   - Children should NOT inherit public read by default
   - Only owner permissions should have `acl:default`

## Current CTH Status (v0.0.14)

| Test Suite | Status |
|------------|--------|
| Authentication | 6/6 passing |
| Protocol (other) | Not yet tested |
| WAC | Skipped |
| ACP | Skipped |
