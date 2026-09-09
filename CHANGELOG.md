# Changelog

All notable changes to this project will be documented in this file.
Dates are in UTC. Tags use semantic versioning.

## [2.3.0] - 2026-09-08

### Added
- WAN failover algorithm (`failover`) with unhealthy/recovery transition logging.
- Dashboard unhealthy-backend indicator when failover pools exist.
- Firewall rules UI: list, create, update, delete via web UI.
- Samples page with import-ready JSON for devices, telemetry, firewall rules, and LB snapshot.
- Auth integration coverage for Basic, Bearer, and non-loopback hard-fail.

### Changed
- Exposed HTTP route algorithm in `/api/lb`.
- Improved health-checker transition logging for backend state changes.

### Fixed
- Resolved lock contention on test env helpers with recovery-aware mutex guard.

## [2.2.0] - 2026-09-07

### Added
- React web UI replacing legacy Dioxus dashboard.
- Device CRUD, config apply/versioning, telemetry, and alerts API.
- Diagnostics, topology, path labels, policies, BGP UI.
- Bearer auth with loopback default and live-action gating.

### Changed
- Consolidated dashboard rendering and state loading paths.

### Fixed
- Auth middleware edge cases for browser-based mutation flows.
