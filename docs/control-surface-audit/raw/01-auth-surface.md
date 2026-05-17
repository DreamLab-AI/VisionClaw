# Auth & Sign-in Surface

Comprehensive audit of authentication and access control components across the SIGN-IN and ACCESS-CONTROL surface. Maps client and server implementations, state contracts, API pathways, and field configurations.

## 1. Components (Client)

| File:Line | Name | Role | Mounted Where | State Source |
|-----------|------|------|---------------|--------------|
| client/src/components/NostrLoginScreen.tsx:5 | NostrLoginScreen | Primary login UI; passkey, PodKey (NIP-07), dev bypass | App root when !authenticated | useNostrAuth hook + nostrAuth service |
| client/src/components/AuthGatedVoiceButton.tsx:11 | AuthGatedVoiceButton | Locks voice features behind auth; shows lock icon when !authenticated | VoiceButton wrapper (voice UI) | useSettingsStore.authenticated + nostrAuth.isAuthenticated() |
| client/src/components/AuthGatedVoiceIndicator.tsx:10 | AuthGatedVoiceIndicator | Locks voice status display behind auth | VoiceIndicator wrapper | useSettingsStore.authenticated + nostrAuth.isAuthenticated() |
| client/src/components/OnboardingWizard.tsx:116 | OnboardingWizard | Multi-step registration/login: welcome → username → passkey → identity-ready or sign-in-method → sign-in/extension | Shown when first-time or re-auth needed | State machine; calls passkeyService + nostrAuth |
| client/src/components/ConnectionWarning.tsx:10 | ConnectionWarning | Network status banner; detects server disconnect, falls back to localStorage settings | Fixed top banner (z-40) | webSocketService.isReady() + settings fallback |
| client/src/contexts/ApplicationModeContext.tsx:43 | ApplicationModeProvider | Application mode (desktop/mobile/xr) and layout visibility | App root provider | Window resize + mode state |

---

## 2. Auth State Contracts

| Context/Store/Hook | File:Line | Shape | Consumers |
|--------------------|-----------|-------|-----------|
| useNostrAuth hook | client/src/hooks/useNostrAuth.ts:7 | `{ authenticated: bool, user?: SimpleNostrUser, error?: string, isLoading, login(), logout(), devLogin(), loginWithPasskey(), hasNip07, hasPasskeySession, isDevLoginAvailable }` | NostrLoginScreen, App, OnboardingWizard, settingsStore, AuthGatedVoice* |
| nostrAuth singleton | client/src/services/nostrAuthService.ts:197 | NostrAuthService class; holds currentUser, localPrivateKey; listeners for auth state changes | All client modules via import |
| SimpleNostrUser | client/src/services/nostrAuthService.ts:149 | `{ pubkey: string, npub?: string, isPowerUser: bool }` | Passed to app, stored in localStorage["nostr_user"] |
| settingsStore auth props | client/src/store/settingsStore.ts:247 | `{ authenticated: bool, user: { isPowerUser, pubkey } \| null, isPowerUser: bool }` | IntegratedControlPanel, UnifiedSettingsTabContent |
| ApplicationModeContext | client/src/contexts/ApplicationModeContext.tsx:14 | `{ mode: 'desktop'\|'mobile'\|'xr', isXRMode, isMobileView, layoutSettings }` | Layout rendering, control panel visibility |

**Key: Nostr-native (NIP-98). No session tokens.** User authenticated = has valid passkey OR NIP-07 extension OR dev mode enabled. User persisted in localStorage["nostr_user"]; private key held in memory only.

---

## 3. API Pathways

### Client → Server Auth Calls

| Client Caller (file:line) | Method | Path | Server Handler (file:line) | Status |
|---------------------------|--------|------|---------------------------|--------|
| nostrAuthService.ts:388 | POST | `/api/auth/nostr` (legacy verify) | src/handlers/nostr_handler.rs:129 login() | Live; NIP-98 signed |
| nostrAuthService.ts:427 | DELETE | `/api/auth/nostr` | src/handlers/nostr_handler.rs:logout() | Live; logout clears session |
| nostrAuthService.ts:243 | (NIP-98 signing) | Any API route | settingsApi.ts:16 axios interceptor | All requests signed with `Authorization: Nostr <base64-event>` |
| passkeyService.ts:82 | POST | `/idp/passkey/register-new/options` | src/idp/passkey.js + interactions.js | WebAuthn registration options; 409=taken, 200=available |
| passkeyService.ts:132 | POST | `/idp/passkey/register-new` (create) | src/idp/passkey.js | Credential attestation; derives PRF if available |
| passkeyService.ts:508 | POST | `/idp/passkey/authenticate/options` | src/idp/passkey.js | WebAuthn authentication options |
| passkeyService.ts:512 | POST | `/idp/passkey/authenticate` (verify) | src/idp/passkey.js | Verifies credential; derives Nostr key from PRF |
| nostrAuthService.ts:70 (NIP-07) | signEvent() | (Extension method) | window.nostr (PodKey/Alby) | NIP-07 provider; client signs auth events |
| settingsApi.ts:520–875 | GET/PUT/POST | `/api/settings/physics`, `/api/settings/rendering`, `/api/settings/visual`, `/api/settings/node-filter`, `/api/settings/quality-gates`, `/api/settings/profiles`, `/api/settings/all`, `/api/clustering/*`, `/api/semantic-forces/*`, `/api/ontology-physics/*` | src/handlers/settings_handler.rs (inferred) | Settings sync gated by auth; some paths local-only (no server endpoint) |
| settingsApi.ts:538 | POST | `/idp/register` (passkey auth entry) | src/idp/interactions.js | Redirects to registration flow |

### Server Auth Routes (inferred from handlers)

| Server Route | Handler | Auth Middleware | Returns |
|--------------|---------|-----------------|---------|
| POST /api/auth/nostr | nostr_handler.rs:129 login() | NIP-98 verify | `{ user: { pubkey, npub, isPowerUser }, token, expiresAt, features }` |
| DELETE /api/auth/nostr | nostr_handler.rs logout() | Authenticated | Session cleared |
| POST /api/auth/nostr/verify | nostr_handler.rs:verify() | NIP-98 | `{ valid: bool, user?, features[] }` |
| POST /api/auth/nostr/refresh | nostr_handler.rs:refresh() | Authenticated | New token + features |
| GET /api/auth/nostr/power-user-status | nostr_handler.rs:70 | RequireAuth::authenticated() | `{ is_power_user: bool }` |
| GET /api/auth/nostr/features | nostr_handler.rs:89 | RequireAuth::authenticated() | `{ features: string[] }` |
| GET /api/auth/nostr/features/{feature} | nostr_handler.rs:109 | RequireAuth::authenticated() | `{ has_access: bool }` |
| POST /api/auth/nostr/api-keys | nostr_handler.rs:update_api_keys() | RequireAuth::authenticated() | Updates perplexity/openai/ragflow API keys |
| GET /api/auth/nostr/api-keys | nostr_handler.rs:get_api_keys() | RequireAuth::authenticated() | Returns stored API keys (masked) |

---

## 4. Settings / Fields Exposed by Auth Surface

### Authentication Gating Fields (Control Panel UI)

| UI Element (file:line) | Setting Path/Key | Type | Default | Persisted To | Server Source-of-Truth | Notes |
|------------------------|------------------|------|---------|--------------|------------------------|-------|
| IntegratedControlPanel.tsx | auth.enabled | bool | false | localStorage (settingsStore) | No server endpoint | Feature flag; controls whether voice/editor gates are active |
| UnifiedSettingsTabContent.tsx:isPowerUserOnly | isPowerUserOnly | attr (UI) | false | N/A (config-driven) | FeatureAccess.is_power_user() | Hides advanced tabs/fields from non-power-users |
| TabNavigation.tsx (tab filtering) | isPowerUserOnly | attr (UI) | false | N/A (config-driven) | FeatureAccess.is_power_user() | Filters visible tabs based on power-user status |
| UnifiedSettingsTabContent.tsx (field filtering) | isPowerUserOnly (field attr) | attr (UI) | false | N/A (config-driven) | FeatureAccess.is_power_user() | Hides power-user-only settings fields from UI |

### Session & Identity Fields (Persisted)

| Field | Storage | Get Path | Set Path | Type | Notes |
|-------|---------|----------|----------|------|-------|
| nostr_user | localStorage | JSON parse | JSON stringify | SimpleNostrUser | Cached at authenticate time; cleared on logout |
| ephemeral_session_pubkey | sessionStorage | getItem() | setItem() | string (hex pubkey) | Dev mode only; per-tab unique pubkey in dev |
| nostr_passkey_pubkey | sessionStorage | getItem() | setItem() | string (hex pubkey) | Cached pubkey from passkey login; used to verify stored key matches |
| nostr_prf | sessionStorage | getItem() | setItem() | "0" or "1" | Flag: 1 = PRF was used to derive Nostr key from passkey |
| nostr_privkey (legacy) | sessionStorage | getItem() | removeItem() | hex string | **DEPRECATED**: removed on init; replaced by in-memory _localKeyHex |
| _localKeyHex (module scope) | Memory only | (internal) | setLocalKey() | hex string | Private key never persisted; cleared on page unload or logout |

### Developer/Testing Fields

| Field | Source | Type | Trigger | Notes |
|-------|--------|------|---------|-------|
| VITE_DEV_MODE_AUTH | .env | bool string ("true") | import.meta.env.DEV && VITE_DEV_MODE_AUTH==="true" | When true, nostrAuth.isDevMode()=true; auto-login as power user |
| VITE_DEV_POWER_USER_PUBKEY | .env | hex string (pubkey) | Dev mode init | Used as auto-login pubkey; falls back to random UUID if not set |
| devLogin() method | NostrLoginScreen | button action | isDevLoginAvailable + click | Only on localhost/192.168.*/10.*/172.16-31.*; logs in as power user |

---

## 5. Feature Access & Role Control

### Power User Gating (Server)

| Endpoint/Field | Power-User Check | Source | Bypass |
|----------------|------------------|--------|--------|
| Advanced control panel tabs | isPowerUserOnly config attr | FeatureAccess.is_power_user() | None; checked on render |
| Advanced settings fields | isPowerUserOnly field attr | FeatureAccess.is_power_user() | Local checkbox (client-side only, no server validation) |
| Settings sync to server | can_sync_settings() | FeatureAccess.is_power_user() \|\| in SETTINGS_SYNC_ENABLED_PUBKEYS | Power user status OR explicit allowlist |
| Clustering endpoints | (inferred) | Feature access check | (inferred) |
| Semantic forces endpoints | (inferred) | Feature access check | (inferred) |

### Feature Access Lists (Environment Variables)

| Var | Type | Meaning | Example |
|-----|------|---------|---------|
| APPROVED_PUBKEYS | CSV pubkeys | Users allowed to access the system at all | abc123...,def456... |
| POWER_USER_PUBKEYS | CSV pubkeys | Users with advanced UI + settings sync | power1...,power2... |
| SETTINGS_SYNC_ENABLED_PUBKEYS | CSV pubkeys | Non-power-users allowed to sync settings to server | sync1...,sync2... |
| PERPLEXITY_ENABLED_PUBKEYS | CSV pubkeys | Perplexity API access | perc1... |
| OPENAI_ENABLED_PUBKEYS | CSV pubkeys | OpenAI API access | openai1... |
| RAGFLOW_ENABLED_PUBKEYS | CSV pubkeys | RagFlow API access | rag1... |
| VITE_DEV_MODE_AUTH | bool ("true") | Enable dev login bypass on localhost | true / false / unset |

---

## 6. Auth Middleware & Access Levels (Server)

| Level | Middleware | Used For | Check |
|-------|-----------|----------|-------|
| Optional | RequireAuth::optional() | GET graph data, public endpoints | NIP-98 verified OR anonymous (empty pubkey) |
| Authenticated | RequireAuth::authenticated() | /api/auth/nostr/* | NIP-98 header required; rejects malformed |
| ReadOnly | RequireAuth::read_only() | GET operations | Authenticated + no write permission needed |
| WriteGraph | RequireAuth::write_graph() | Graph modification endpoints | Authenticated + permission check |
| WriteSettings | RequireAuth::write_settings() | PUT /api/settings/* | Authenticated + power user OR sync allowlist |
| PowerUser | RequireAuth::power_user() | Advanced features | isPowerUser from FeatureAccess |
| Admin | RequireAuth::admin() | Admin endpoints | Admin pubkey in allowlist |

---

## 7. Disconnects & Notes

### Potential Issues / Asymmetries

1. **Client-side power-user gating not validated server-side**: Control panel tabs/fields have `isPowerUserOnly` attrs that filter the UI, but the server does not enforce these on settings endpoints. A client could manually POST to a power-user-only endpoint. Settings endpoints should validate `can_sync_settings()` or `is_power_user()`.

2. **localStorage persistence of user without server session validation**: `nostr_user` is restored from localStorage on init, but if the NIP-07 extension is not available and passkey session is stale, the client reports "stale session" after 5s. However, intermediate state (user cached, no signing key) could allow observer to see partial auth state.

3. **Dev mode always bypasses**: `VITE_DEV_MODE_AUTH=true` auto-logs in on init without any user interaction. No "remember me" check; always authenticated as power user. Risk if dev .env exposed.

4. **Passkey PRF derivation optional**: If WebAuthn PRF extension not supported, passkey login falls back to randomly-generated Nostr key and forces manual backup download. User may not understand key is not recoverable. No clear UI warning in all flows.

5. **Socket flow filter_auth may have inverted logic**: src/handlers/socket_flow_handler/filter_auth.rs referenced but not audited. May need separate review.

6. **NIP-98 signing on every request**: Every API call is signed (not just auth). If client signing fails silently, request may go unsigned. Interceptor logs warning but does not fail request. Server may reject or process unsigned.

7. **No refresh token mechanism**: `getSessionToken()` returns null (deprecated comment). Token expiry hardcoded to `AUTH_TOKEN_EXPIRY` env var (default 3600s). No automatic refresh; expired tokens force re-login.

8. **Voice/editor gates use both store.authenticated AND nostrAuth.isAuthenticated()**:  Redundant checks; settingsStore is the source of truth for UI, but service layer also checked. Could diverge if store not synced.

9. **Connection warning falls back to localStorage**: If server unreachable, settings loaded from localStorage. No cache invalidation strategy. Stale settings may persist.

10. **OIDC config exists but not wired to client**: src/config/oidc.rs defines OpenID Connect, but client does not call any OIDC endpoints. OIDC appears disabled by default. No client UI for OIDC login.

11. **Legacy sessionStorage keys not fully cleaned**: removeItem calls for 'nostr_privkey', 'nostr_passkey_key', 'nostr_prf' happen, but these keys may linger in older browsers or if cleanup fails. Could expose stale key material.

12. **Passkey login stores pubkey in sessionStorage**: 'nostr_passkey_pubkey' stored in sessionStorage for validation on restore, but private key is in-memory only. On page reload, user loses session even if private key could be re-derived (PRF mode).

13. **No explicit logout from all connected resources**: Logout clears local state but does not notify server. No revocation of any NIP-98 signing capability. Server has no way to know user is logged out (stateless NIP-98).

---

## 8. API Key Management

| Field | Location | Type | Persisted | Validation | Notes |
|-------|----------|------|-----------|------------|-------|
| apiKeys.perplexity | settingsApi:update_api_keys() | string | Server (encrypted?) | Not checked client-side | POST /api/auth/nostr/api-keys |
| apiKeys.openai | settingsApi:update_api_keys() | string | Server (encrypted?) | Not checked client-side | POST /api/auth/nostr/api-keys |
| apiKeys.ragflow | settingsApi:update_api_keys() | string | Server (encrypted?) | Not checked client-side | POST /api/auth/nostr/api-keys |
| VITE_VIRCADIA_AUTH_TOKEN | VircadiaAdapter.ts:contextValue | string | .env | N/A | Vircadia system token; not Nostr-related |
| VITE_VIRCADIA_AUTH_PROVIDER | VircadiaAdapter.ts:contextValue | string | .env | N/A | Vircadia auth provider; defaults to "system" |

---

## 9. Key Security Properties & Assumptions

1. **Private keys never written to storage**: Passkey-derived Nostr keys held in memory only via `_localKeyHex`. On page unload, key is cleared.

2. **NIP-07 extension manages its own key**: Client never sees extension private key. Only calls signEvent().

3. **NIP-98 auth is per-request**: Each API call is freshly signed with timestamp. No token reuse. Prevents token replay.

4. **Dev mode uses ephemeral per-tab identity**: Each browser tab gets a unique pubkey in dev mode so multiple tabs don't interfere.

5. **No CORS bypass for auth**: Auth headers (Authorization, X-Nostr-Pubkey) require proper origin matching.

---

## 10. Routes Not Yet Audited (Out of Scope)

- src/idp/interactions.js (login, consent, registration flows)
- src/idp/adapter.js (credential/account storage)
- src/auth/solid-oidc.js (OIDC token validation)
- src/handlers/socket_flow_handler/filter_auth.rs (WebSocket auth filtering)
- Enterprise auth (src/middleware/enterprise_auth.rs)

---

## 11. Summary Table: Client → Server Auth Flows

| Flow | Steps | Key Files | Status |
|------|-------|-----------|--------|
| **Passkey Registration** | 1. Start (/idp/passkey/register-new/options) 2. Create credential (WebAuthn) 3. Derive Nostr key (PRF) 4. Verify (/idp/passkey/register-new) 5. Store in memory | OnboardingWizard.tsx, passkeyService.ts, nostrAuthService.ts:531 | Live |
| **Passkey Login** | 1. Start (/idp/passkey/authenticate/options) 2. Authenticate (WebAuthn) 3. Derive Nostr key (PRF) 4. Verify (/idp/passkey/authenticate) | OnboardingWizard.tsx, passkeyService.ts, nostrAuthService.ts:531 | Live |
| **NIP-07 (PodKey/Alby)** | 1. Detect extension (Object.defineProperty + poll) 2. getPublicKey() 3. Store pubkey in localStorage 4. Sign requests with signEvent() | useNostrAuth.ts:14, nostrAuthService.ts:388, settingsApi.ts:37 | Live |
| **Dev Login** | 1. Check localhost 2. Use VITE_DEV_POWER_USER_PUBKEY or random UUID 3. Store in sessionStorage 4. Auto-login on init if VITE_DEV_MODE_AUTH=true | nostrAuthService.ts:235, NostrLoginScreen.tsx:105, .env | Live |
| **Logout** | 1. Clear currentUser 2. Wipe localPrivateKey & _localKeyHex 3. Clear localStorage["nostr_user"] 4. Clear sessionStorage entries | nostrAuthService.ts:446 | Live |

