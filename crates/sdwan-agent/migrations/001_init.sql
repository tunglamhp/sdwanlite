-- migrations/001_init.sql
-- Device registry + config history for sdwan-agent controller (P1).

PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS devices (
    device_id   TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL,
    site_id     TEXT NOT NULL,
    hostname    TEXT,
    status      TEXT NOT NULL DEFAULT 'provisioned',
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS configs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id   TEXT NOT NULL REFERENCES devices(device_id),
    version     TEXT NOT NULL,
    config_json TEXT NOT NULL,
    pushed_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS apply_audit (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id   TEXT NOT NULL REFERENCES devices(device_id),
    version     TEXT NOT NULL,
    state       TEXT NOT NULL,
    detail      TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_configs_device ON configs(device_id);
CREATE INDEX IF NOT EXISTS idx_apply_audit_device ON apply_audit(device_id);
