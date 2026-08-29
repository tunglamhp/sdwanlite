# REVIEW-UI — Frontend standards audit & design philosophy

**Date:** 2026-08-29 · **Scope:** `web-ui/` (React/TS dashboard) vs backend v1 controller API

## Audit findings

### Blocking (push gate)

| # | Finding | Standard |
|---|---|---|
| F1 | Test suite red: **7/10 fail** (Dashboard test mocks stale, api tests broken) | CI must be green |
| F2 | JSON `<pre>` debug dumps rendered to operators (Dashboard, Devices, Policies, Topology, Diagnostics), raw error strings shown | Usability: no raw debug output |
| F3 | Auth token broken: Settings writes Zustand store, `api.ts` reads `sessionStorage` — token never sent | Feature correctness |
| F4 | Google Fonts external import — operator browser leaks to Google; offline-fragile; CSP-hostile | Privacy, self-contained appliance UI |
| F5 | Theme toggle button has **no accessible name** (confirmed via a11y snapshot) | WCAG 4.1.2 |
| F6 | No `:focus-visible` styling — keyboard focus invisible | WCAG 2.4.7 |
| F7 | Dead scaffold committed: `App.css` (Vite template, never imported), `assets/{hero.png,react.svg,vite.svg}`, `useRealtime` hook, `AdvancedSection`, Vite-template `README.md` | Lean repo |

### Non-blocking

| # | Finding | Standard |
|---|---|---|
| F8 | No `aria-current="page"` on active nav | WCAG 4.1.2 |
| F9 | Theme not persisted; ignores `prefers-color-scheme`; no `color-scheme` meta | Platform convention |
| F10 | Destructive "Deregister" without confirmation | Error prevention |
| F11 | `*` route silently renders Dashboard (no 404) | Wayfinding |
| F12 | Fixed 220px sidebar, `100vh` — unusable on narrow screens | WCAG 1.4.10 reflow |
| F13 | No `prefers-reduced-motion` handling | WCAG 2.3.3 |
| F14 | Playwright config imports unused `devices` (lint warning) | Clean |
| F15 | `title` = "web-ui" (not product name) | Identity |
| F16 | `loadDeviceConfig`/`startConfigStream` exist in store but no page calls them — config pages permanently empty | Feature wiring |
| F17 | `web-ui` not built in CI | Build hygiene |
| F18 | Release zips (`sdwanlited-*-windows-x64.zip`, ~3 MB) tracked in git; `*.zip` ignored but committed before | Repo size |

## Design philosophy — "ops console, not marketing page"

Governing principle: the dashboard is a **control surface for a network appliance**,
read by tired operators under incident pressure. Every pixel serves a state or an action.

1. **Dense but calm.** Information density like a good spreadsheet; whitespace
   like a technical manual. No hero sections, no illustration, no decorative imagery.
2. **Semantic color only.** Status colors (ok/warn/error) and neutral ink. No
   gradients, no glassmorphism, no purple/blue AI-signature palettes, no emoji.
   Color never carries information alone — always paired with text or glyph.
3. **Real data, real states.** Tables and stat cards fed from live API; explicit
   empty states ("No devices registered"), explicit error alerts. Never raw JSON,
   never silent `catch(() => undefined)`.
4. **Keyboard-first.** Every interactive element focusable, labeled, and operable;
   `:focus-visible` ring on all controls; `aria-current` on nav; confirm on
   destructive actions.
5. **Self-contained.** System font stack, no external requests. Works offline,
   ships behind the appliance's own CSP.
6. **One pattern per job.** Card (grouped stat), table (row data), alert (error),
   empty state (absence), badge (status). No bespoke components where a table
   suffices; delete any component that has no caller.

### Component spec (implemented)

| Component | Rules |
|---|---|
| Card | 1px `--border`, 6px radius, no shadow; label 11px uppercase muted, value 20px ink |
| DataTable | `<table>` semantics; `th` 11px uppercase muted; row borders; 13px cells |
| Alert | Inline, `role="alert"`, error styling, plain-language message |
| EmptyState | Muted single line; states what is absent and why |
| Badge | 8px status dot + text; ok/warn/error only |
| Nav | Active item: subtle bg + `aria-current="page"`; toggle labeled "Toggle theme" |

## Implementation state

- [x] Dead code removed, token fix, a11y pass, responsive sidebar, real components
- [x] Dashboard/Devices tables + status cards; config pages wired to device selection
- [x] Tests green, CI builds web-ui
- [x] Pushed to `origin/dev`

## Follow-ups (not in this push)

- i18n mechanism when multi-language is required (labels centralized today)
- Real-time wiring: v1 controller has no SSE `/api/events`; WS `/stream/config` is the channel
- E2E Playwright suite in CI once the controller API is deployed alongside
