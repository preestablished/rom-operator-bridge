# Tech Stack Options

## Recommended: Current Rust Service And TypeScript UI

Use the current stack unless implementation proves it cannot satisfy the HTTP
route requirements.

Reasons:

- the Rust service already owns auth, Origin checks, session state, private
  file permissions, static UI serving, runtime API, and WebSockets;
- the TypeScript UI already uses same-origin API paths and selects `ws:` on
  HTTP pages;
- existing tests cover auth, runtime API, WebSockets, static headers, redaction,
  real backend flows, capture labels, and synthetic smoke;
- changing stacks would still need the same backend trust decisions for cookies,
  Origin, and private evidence.

Expected implementation cost is moderate and localized:

- config parsing;
- auth/cookie/CORS;
- CSP generation;
- deploy validation scripts;
- docs and tests.

If preserving the existing HTTPS route in the same process, this also includes
per-route profile selection by Host/Origin. If that becomes too invasive, use a
separate Tailscale service instance rather than changing the HTTPS service into
HTTP mode.

## Tauri Option

Use Tauri only if browser HTTP is rejected as a product decision after the
Tailscale route is explored.

Tauri can avoid browser insecure-origin UX by packaging a desktop operator app,
but it does not remove backend security requirements. It also introduces new
work:

- desktop app packaging and update flow;
- explicit host selection for tailnet or localhost;
- a decision about whether auth stays cookie-based or moves to an app-held
  bearer/session token;
- OS keychain or local config handling;
- new smoke tests outside the current browser test harness.

Acceptable Tauri shape:

```text
Tauri shell
  -> bundled or hosted web UI
  -> existing Rust bridge service over localhost or tailnet HTTP
```

Avoid embedding private credentials in the app bundle. Use operator entry at
runtime.

## Flutter Option

Flutter is not recommended for this specific change. It would be useful only if
the operator workflow needs a larger native/mobile redesign.

Costs:

- reimplement the existing SPA interaction model;
- reimplement WebSocket event/input handling;
- recreate capture review and label UI;
- add a separate release/test/deploy path;
- still depend on the Rust bridge backend for private runtime work.

If chosen later, keep the Rust service as the backend and treat Flutter as a
client replacement only.

## Java Option

Java is not recommended for this route change.

Reasonable Java uses:

- a small desktop operator client if Java is required by the operator
  environment;
- a deployment helper for private validation if shell/Python is unsuitable.

Avoid rewriting the bridge backend in Java unless a separate bead explicitly
changes the architecture. The current Rust backend already encodes privacy and
runtime constraints that would be expensive to port safely.

## Direct Service On Port 80

Directly binding the Rust service to `<tailscale-ip>:80` is a fallback, not the
default.

It requires one of:

- running as root, which is not acceptable for this service;
- granting `CAP_NET_BIND_SERVICE` through systemd;
- relaxing the env validator's fixed port assumption.

It also exposes the application process directly on the tailnet interface. The
reverse-proxy topology is easier to inspect, restrict, and roll back.

## Tailscale Serve Option

Tailscale Serve may be considered only if it can provide a plain HTTP listener
for this custom hostname without enabling TLS or Funnel-style public exposure.
Do not assume this. Verify current Tailscale behavior on the host before
planning around it.

If Tailscale Serve is used, validation requirements remain the same:

- Host must be `tailrombridge.birb.homes`;
- public Internet exposure must be disabled;
- WebSocket upgrades must work;
- HTTP Origin and cookie behavior must match this plan;
- raw Tailscale config output must stay private if it contains device or
  network identifiers.

## Decision Rule

Choose the smallest stack that satisfies:

- tailnet-only HTTP access at `http://tailrombridge.birb.homes/`;
- no public non-Tailscale exposure;
- working cookie auth in browsers;
- same-origin API and WebSockets;
- no weakening of the existing HTTPS route;
- either per-route profiles in one bridge process or a separate isolated
  Tailscale service instance;
- repeatable validation and rollback.

Under that rule, the current Rust/Vite stack plus a local HTTP reverse proxy is
the preferred path.
