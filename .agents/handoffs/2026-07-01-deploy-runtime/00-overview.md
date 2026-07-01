# 2026-07-01 Deploy And Runtime Handoff

## Purpose

This handoff captures the work done in this session to deploy the
`rom-operator-bridge` changes, fix the static UI routing, and begin diagnosing
the remaining browser-visible runtime state:

```text
Session
faulted
synthetic
Runtime unavailable.
```

Use this handoff for the next coding-agent session. Do not treat it as a
private evidence store; instantiated secrets, cookies, private env values, raw
logs, and operator-private paths are intentionally omitted.

## Current Bead

- `rom-operator-bridge-xta`
- Type: bug
- Status at handoff creation: `IN_PROGRESS`
- Title: `Diagnose deployed runtime unavailable state`

## Repo State At Handoff

- Branch: `main`
- Remote status before creating this handoff: up to date with `origin/main`
- Recent relevant commits:
  - `b4bcc8f Check Node version before release build`
  - `22b1ec0 Normalize deployed env file syntax`
  - `9b944b8 Add narrow deployment helper`
  - `54eb016 Remove operator credential flow`
  - `f9c49df Add Tailscale HTTP ingress template`

## Important URLs

- Primary HTTPS UI: `https://rombridge.birb.homes/`
- Tailscale HTTP UI: `http://tailrombridge.birb.homes/`
- Tailscale IP observed in this session: `100.82.43.93`

Important: do not use the bare Tailscale IP as the browser URL for this app.
The route is Host-header based. Bare `http://100.82.43.93/` routes to the
default Apache site, while `Host: tailrombridge.birb.homes` routes to the
bridge.

