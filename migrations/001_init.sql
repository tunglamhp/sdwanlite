-- sdwanlite control-plane schema (P0 → P1).
--
-- P0: this file is the durable contract. The in-process controller uses an
-- in-memory `DeviceStore`; P1 swaps it for `rusqlite`-backed storage using
-- this exact schema. No real data is stored at P0 runtime.
--
-- All IDs are UUIDv4 stored as TEXT (RFC 4122 canonical form).
-- Timestamps are epoch seconds (INTEGER).
--
-- Address hygiene: examples in comments use RFC 5737 documentation addresses
-- (`192.0.2.x`, `198.51.100.x`, `203.0.113.x`) only.

CREATE TABLE IF NOT EXISTS organizations (
    id          TEXT PRIMARY KEY,                  -- e.g. '22222222-2222-2222-2222-222222222222'
    name        TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sites (
    id          TEXT PRIMARY KEY,                  -- e.g. '33333333-...'
    org_id      TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    UNIQUE(org_id, name)
);

CREATE TABLE IF NOT EXISTS devices (
    id          TEXT PRIMARY KEY,                  -- e.g. '11111111-...'
    org_id      TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    site_id     TEXT NOT NULL REFERENCES sites(id)    ON DELETE RESTRICT,
    hostname    TEXT NOT NULL,
    last_seen   INTEGER NOT NULL,
    UNIQUE(org_id, hostname)
);

CREATE INDEX IF NOT EXISTS idx_devices_org     ON devices(org_id);
CREATE INDEX IF NOT EXISTS idx_devices_site    ON devices(site_id);

-- Per-tunnel metadata. Wire-private keys NEVER appear here; only the public
-- half is exchanged via the `TunnelConfig::WireGuard::public_key` field.
CREATE TABLE IF NOT EXISTS tunnels (
    device_id   TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    interface   TEXT NOT NULL,                     -- 'wg0', 'wg1', ...
    public_key  TEXT NOT NULL,                     -- base64 X25519 (44 chars)
    endpoint    TEXT,                               -- '203.0.113.7:51820'
    path_label  TEXT,
    PRIMARY KEY(device_id, interface)
);

-- Append-only device configuration history. `version` is monotonic per device;
-- the agent's `apply_config` only accepts `new.version > current.version`
-- (optimistic locking). The latest row per device is the live config.
CREATE TABLE IF NOT EXISTS device_configs (
    device_id    TEXT    NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    version      INTEGER NOT NULL,
    config_json  TEXT    NOT NULL,                  -- canonical DeviceConfig payload
    committed_at INTEGER NOT NULL,
    PRIMARY KEY(device_id, version)
);

CREATE INDEX IF NOT EXISTS idx_device_configs_device ON device_configs(device_id);

-- Telemetry frames land in a partitioned roll-up table (P1). P0 schema
-- declares the columns so the migration is stable across versions.
CREATE TABLE IF NOT EXISTS telemetry_frames (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id    TEXT    NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    org_id       TEXT    NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    received_at  INTEGER NOT NULL,
    frame_json   TEXT    NOT NULL                  -- canonical TelemetryFrame payload
);

CREATE INDEX IF NOT EXISTS idx_telemetry_device_time ON telemetry_frames(device_id, received_at);

-- Health flag history (one row per raised flag). P0 keeps the table empty;
-- controllers populate it as `HealthFlag::*` arrives in telemetry frames.
CREATE TABLE IF NOT EXISTS health_flags (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id    TEXT    NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    raised_at    INTEGER NOT NULL,
    cleared_at   INTEGER,                          -- NULL while the flag is still active
    kind         TEXT    NOT NULL,                 -- 'link_down' | 'degraded'
    detail_json  TEXT    NOT NULL                  -- the HealthFlag payload
);

CREATE INDEX IF NOT EXISTS idx_health_flags_device ON health_flags(device_id);
CREATE INDEX IF NOT EXISTS idx_health_flags_active ON health_flags(cleared_at) WHERE cleared_at IS NULL;

-- File mode for SQLite database (when created via rusqlite::Connection):
-- the controller opens the file with mode 0600 in P1. The migration does
-- not enforce this — that's a runtime concern (AGENTS.md).
