-- =================================================================
-- VisionClaw Settings Database (settings.db)
-- =================================================================
-- Purpose: Application configuration, user management, API keys, and audit logs
-- Version: 2.0.0
-- Created: 2025-10-22
-- =================================================================

-- Enable WAL mode for better concurrency
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON;

-- =================================================================
-- SCHEMA VERSION TRACKING
-- =================================================================

CREATE TABLE IF NOT EXISTS schema_version (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    version INTEGER NOT NULL,
    applied_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    description TEXT
);

INSERT OR IGNORE INTO schema_version (id, version, description)
VALUES (1, 2, 'Three-database system - Settings database');

-- =================================================================
-- GENERAL SETTINGS TABLE
-- =================================================================

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value_type TEXT NOT NULL CHECK (value_type IN ('string', 'integer', 'float', 'boolean', 'json')),
    value_text TEXT,
    value_integer INTEGER,
    value_float REAL,
    value_boolean INTEGER CHECK (value_boolean IN (0, 1)),
    value_json TEXT, -- JSON blob for complex settings
    description TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_settings_key ON settings(key);
CREATE INDEX IF NOT EXISTS idx_settings_updated_at ON settings(updated_at);
CREATE INDEX IF NOT EXISTS idx_settings_type ON settings(value_type);

-- =================================================================
-- PHYSICS SETTINGS TABLE
-- =================================================================

CREATE TABLE IF NOT EXISTS physics_settings (
    profile_name TEXT PRIMARY KEY,

    -- Core physics parameters
    damping REAL NOT NULL DEFAULT 0.85,
    dt REAL NOT NULL DEFAULT 0.016,
    iterations INTEGER NOT NULL DEFAULT 10,
    max_velocity REAL NOT NULL DEFAULT 100.0,
    max_force REAL NOT NULL DEFAULT 50.0,
    repel_k REAL NOT NULL DEFAULT 1000.0,
    spring_k REAL NOT NULL DEFAULT 0.5,
    mass_scale REAL NOT NULL DEFAULT 1.0,
    boundary_damping REAL NOT NULL DEFAULT 0.8,
    temperature REAL NOT NULL DEFAULT 1.0,
    gravity REAL NOT NULL DEFAULT 0.0,
    bounds_size REAL NOT NULL DEFAULT 500.0,
    enable_bounds INTEGER NOT NULL DEFAULT 1 CHECK (enable_bounds IN (0, 1)),

    -- Advanced parameters
    rest_length REAL NOT NULL DEFAULT 50.0,
    repulsion_cutoff REAL NOT NULL DEFAULT 200.0,
    repulsion_softening_epsilon REAL NOT NULL DEFAULT 0.1,
    center_gravity_k REAL NOT NULL DEFAULT 0.01,
    grid_cell_size REAL NOT NULL DEFAULT 100.0,
    warmup_iterations INTEGER NOT NULL DEFAULT 50,
    cooling_rate REAL NOT NULL DEFAULT 0.98,
    constraint_ramp_frames INTEGER NOT NULL DEFAULT 100,
    constraint_max_force_per_node REAL NOT NULL DEFAULT 1000.0,

    -- Metadata
    description TEXT,
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_physics_profile ON physics_settings(profile_name);
CREATE INDEX IF NOT EXISTS idx_physics_default ON physics_settings(is_default);

-- =================================================================
-- USER MANAGEMENT TABLE
-- =================================================================

CREATE TABLE IF NOT EXISTS users (
    user_id TEXT PRIMARY KEY,
    pubkey TEXT UNIQUE NOT NULL, -- Nostr public key for authentication
    tier TEXT NOT NULL DEFAULT 'public' CHECK (tier IN ('public', 'user', 'developer')),
    email TEXT UNIQUE,
    username TEXT UNIQUE,
    display_name TEXT,

    -- User preferences as JSON
    settings_json TEXT DEFAULT '{}',

    -- Account status
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    is_verified INTEGER NOT NULL DEFAULT 0 CHECK (is_verified IN (0, 1)),

    -- Rate limiting
    rate_limit_tier TEXT DEFAULT 'free' CHECK (rate_limit_tier IN ('free', 'power', 'unlimited')),
    max_requests_per_hour INTEGER DEFAULT 100,

    -- Timestamps
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    last_login DATETIME,
    last_activity DATETIME
);

CREATE INDEX IF NOT EXISTS idx_users_tier ON users(tier);
CREATE INDEX IF NOT EXISTS idx_users_pubkey ON users(pubkey);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_active ON users(is_active);
CREATE INDEX IF NOT EXISTS idx_users_last_activity ON users(last_activity);

-- =================================================================
-- API KEYS TABLE
-- =================================================================

CREATE TABLE IF NOT EXISTS api_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    service_name TEXT NOT NULL,

    -- Encrypted API key storage
    api_key_encrypted TEXT NOT NULL, -- Encrypted with AES-256
    key_hash TEXT NOT NULL, -- SHA-256 hash for verification

    -- Key metadata
    key_name TEXT,
    key_description TEXT,
    scopes TEXT DEFAULT '[]', -- JSON array of allowed scopes

    -- Status and expiration
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    expires_at DATETIME,

    -- Usage tracking
    usage_count INTEGER NOT NULL DEFAULT 0,
    last_used DATETIME,

    -- Timestamps
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,

    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
    UNIQUE (user_id, service_name, key_name)
);

CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_service ON api_keys(service_name);
CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_active ON api_keys(is_active);
CREATE INDEX IF NOT EXISTS idx_api_keys_expires ON api_keys(expires_at);

-- =================================================================
-- SETTINGS AUDIT LOG TABLE
-- =================================================================

CREATE TABLE IF NOT EXISTS settings_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    setting_key TEXT NOT NULL,
    old_value TEXT,
    new_value TEXT,
    changed_by TEXT, -- user_id or 'system'
    change_reason TEXT,
    change_type TEXT CHECK (change_type IN ('create', 'update', 'delete')),

    -- Request context
    ip_address TEXT,
    user_agent TEXT,

    -- Timestamp
    changed_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_audit_key ON settings_audit_log(setting_key);
CREATE INDEX IF NOT EXISTS idx_audit_date ON settings_audit_log(changed_at);
CREATE INDEX IF NOT EXISTS idx_audit_changed_by ON settings_audit_log(changed_by);
CREATE INDEX IF NOT EXISTS idx_audit_type ON settings_audit_log(change_type);

-- =================================================================
-- RATE LIMITING TABLE
-- =================================================================

CREATE TABLE IF NOT EXISTS rate_limits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    request_count INTEGER NOT NULL DEFAULT 1,
    window_start DATETIME NOT NULL,
    window_end DATETIME NOT NULL,

    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_rate_limits_user ON rate_limits(user_id);
CREATE INDEX IF NOT EXISTS idx_rate_limits_endpoint ON rate_limits(endpoint);
CREATE INDEX IF NOT EXISTS idx_rate_limits_window ON rate_limits(window_start, window_end);

-- =================================================================
-- SESSION MANAGEMENT TABLE
-- =================================================================

CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,

    -- Session data
    session_data TEXT DEFAULT '{}', -- JSON blob

    -- Security
    ip_address TEXT,
    user_agent TEXT,

    -- Status
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),

    -- Timestamps
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    expires_at DATETIME NOT NULL,
    last_activity DATETIME DEFAULT CURRENT_TIMESTAMP,

    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_active ON sessions(is_active);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);

-- =================================================================
-- FEATURE FLAGS TABLE
-- =================================================================

CREATE TABLE IF NOT EXISTS feature_flags (
    flag_name TEXT PRIMARY KEY,
    is_enabled INTEGER NOT NULL DEFAULT 0 CHECK (is_enabled IN (0, 1)),
    description TEXT,

    -- Targeting
    enabled_for_tiers TEXT DEFAULT '[]', -- JSON array of tier names
    enabled_for_users TEXT DEFAULT '[]', -- JSON array of user_ids

    -- Rollout percentage (0-100)
    rollout_percentage INTEGER DEFAULT 0 CHECK (rollout_percentage >= 0 AND rollout_percentage <= 100),

    -- Metadata
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    created_by TEXT
);

CREATE INDEX IF NOT EXISTS idx_feature_flags_enabled ON feature_flags(is_enabled);

-- =================================================================
-- INITIALIZATION DATA
-- =================================================================

BEGIN TRANSACTION;

-- Insert default application settings
INSERT OR IGNORE INTO settings (key, value_type, value_text, description)
VALUES
    ('app_name', 'string', 'VisionClaw', 'Application name'),
    ('app_version', 'string', '2.0.0', 'Current application version');

INSERT OR IGNORE INTO settings (key, value_type, value_integer, description)
VALUES
    ('max_connections', 'integer', 100, 'Maximum WebSocket connections'),
    ('session_timeout_minutes', 'integer', 30, 'Session timeout in minutes');

INSERT OR IGNORE INTO settings (key, value_type, value_boolean, description)
VALUES
    ('debug_mode', 'boolean', 0, 'Debug mode enabled'),
    ('maintenance_mode', 'boolean', 0, 'Maintenance mode enabled');

-- Create default physics profiles
INSERT OR IGNORE INTO physics_settings (profile_name, description, is_default)
VALUES
    ('default', 'Default balanced physics profile', 1),
    ('knowledge', 'Optimised for the vault knowledge graph', 0),
    ('ontology', 'Optimized for dense ontology graphs with hierarchies', 0),
    ('performance', 'High performance with lower quality', 0),
    ('quality', 'High quality with lower performance', 0);

-- Update non-default profiles with optimized settings
UPDATE physics_settings
SET
    damping = 0.85,
    repel_k = 1000.0,
    spring_k = 0.5,
    iterations = 10
WHERE profile_name = 'knowledge';

UPDATE physics_settings
SET
    damping = 0.90,
    repel_k = 1500.0,
    spring_k = 0.3,
    iterations = 15,
    cooling_rate = 0.96
WHERE profile_name = 'ontology';

UPDATE physics_settings
SET
    damping = 0.95,
    iterations = 5,
    max_velocity = 150.0,
    repel_k = 800.0
WHERE profile_name = 'performance';

UPDATE physics_settings
SET
    damping = 0.75,
    iterations = 20,
    max_velocity = 50.0,
    repel_k = 2000.0,
    spring_k = 0.2
WHERE profile_name = 'quality';

-- Create default feature flags
INSERT OR IGNORE INTO feature_flags (flag_name, is_enabled, description, enabled_for_tiers)
VALUES
    ('ontology_sync', 1, 'Enable GitHub ontology synchronization', '["developer"]'),
    ('advanced_physics', 1, 'Enable advanced physics controls', '["power", "developer"]'),
    ('api_access', 1, 'Enable API access', '["developer"]'),
    ('export_graph', 1, 'Enable graph export functionality', '["user", "power", "developer"]');

COMMIT;

-- =================================================================
-- TRIGGERS FOR AUTOMATIC TIMESTAMP UPDATES
-- =================================================================

CREATE TRIGGER IF NOT EXISTS update_settings_timestamp
AFTER UPDATE ON settings
FOR EACH ROW
BEGIN
    UPDATE settings SET updated_at = CURRENT_TIMESTAMP WHERE key = NEW.key;
END;

CREATE TRIGGER IF NOT EXISTS update_physics_settings_timestamp
AFTER UPDATE ON physics_settings
FOR EACH ROW
BEGIN
    UPDATE physics_settings SET updated_at = CURRENT_TIMESTAMP WHERE profile_name = NEW.profile_name;
END;

CREATE TRIGGER IF NOT EXISTS update_users_timestamp
AFTER UPDATE ON users
FOR EACH ROW
BEGIN
    UPDATE users SET updated_at = CURRENT_TIMESTAMP WHERE user_id = NEW.user_id;
END;

CREATE TRIGGER IF NOT EXISTS update_api_keys_timestamp
AFTER UPDATE ON api_keys
FOR EACH ROW
BEGIN
    UPDATE api_keys SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;

-- =================================================================
-- VIEWS FOR CONVENIENT QUERYING
-- =================================================================

-- Active users view
CREATE VIEW IF NOT EXISTS v_active_users AS
SELECT
    user_id,
    pubkey,
    tier,
    email,
    username,
    display_name,
    last_login,
    last_activity,
    created_at
FROM users
WHERE is_active = 1;

-- Active API keys view
CREATE VIEW IF NOT EXISTS v_active_api_keys AS
SELECT
    ak.id,
    ak.user_id,
    u.username,
    ak.service_name,
    ak.key_name,
    ak.scopes,
    ak.usage_count,
    ak.last_used,
    ak.expires_at,
    ak.created_at
FROM api_keys ak
JOIN users u ON ak.user_id = u.user_id
WHERE ak.is_active = 1
  AND (ak.expires_at IS NULL OR ak.expires_at > CURRENT_TIMESTAMP);

-- Settings audit summary view
CREATE VIEW IF NOT EXISTS v_recent_changes AS
SELECT
    setting_key,
    changed_by,
    change_type,
    changed_at,
    new_value
FROM settings_audit_log
ORDER BY changed_at DESC
LIMIT 100;

-- =================================================================
-- VACUUM AND OPTIMIZE
-- =================================================================

PRAGMA optimize;
