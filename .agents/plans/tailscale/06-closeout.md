# Closeout

## Beads

Before implementation, create or claim a bead for the actual route work. The
planning bead for these files is not the implementation bead.

Suggested implementation bead title:

```text
Expose bridge through Tailscale HTTP hostname
```

Suggested acceptance criteria:

```text
The bridge is available at http://tailrombridge.birb.homes/ from an approved
tailnet client without TLS; runtime API and WebSockets work same-origin; HTTP
mode uses non-Secure HttpOnly SameSite=Strict cookies; wrong Origins are
rejected; wrong Host requests do not serve the bridge; the existing HTTPS
deployment remains unchanged and is revalidated; validation and redaction gates
pass; public docs contain no private endpoint addresses, credentials, cookies,
raw evidence, screenshots, or capture IDs.
```

If implementation unblocks real operator smoke, record that as a note on
`rom-operator-bridge-r77` but do not close `r77` unless the real one-capture
label smoke itself has been completed.

## Quality Gates

Minimum gates for code changes:

```sh
git diff --check
cargo fmt --manifest-path service/Cargo.toml
cargo test --manifest-path service/Cargo.toml
npm --prefix ui ci
npm --prefix ui run typecheck
npm --prefix ui test
npm --prefix ui run build
bash scripts/redaction-gate.sh
```

Preferred full gate:

```sh
bash scripts/quality-gate.sh
```

Deployment validation:

```sh
scripts/tailscale-http-check.sh
```

Run it only with private validation directory, private cookie source, private
network evidence, and private forbid-file inputs. Do not paste instantiated
command lines into committed docs.

## Review Focus

Ask reviewers to focus on:

- HTTP mode cannot accidentally disable `Secure` cookies for HTTPS origins;
- same-process coexistence uses per-route profiles, or separate-instance
  coexistence has isolated env, session secret, upstream port, and private root;
- accepted Origin is echoed only after allowlist validation;
- wrong, absent, and `null` Origins remain rejected;
- wrong Host and direct IP-literal requests do not serve the bridge route;
- WebSocket handshakes use the same Origin and cookie policy;
- CSP allows `ws://tailrombridge.birb.homes` only in Tailscale HTTP mode;
- validators make loopback binds valid only for proxy mode;
- route-specific redaction rules allow only the exact Tailscale HTTP/WS origins
  and keep them out of HTTPS output;
- deployment scripts do not print private paths, addresses, cookies, or raw
  evidence;
- existing `https://rombridge.birb.homes/` behavior stays intact.

## Commit Scope

Keep commits easy to review:

- service config/auth/CSP changes with tests;
- UI/WebSocket/security-header tests;
- deploy script and sanitized route docs;
- validation evidence summary after private host validation.

Do not mix a Tauri, Flutter, or Java rewrite into the first implementation
unless the current stack is proven unsuitable.

## Session Close Protocol

After implementation:

```sh
git status --short --branch
bd close <implementation-bead> --reason="<sanitized result>"
git pull --rebase
bd dolt push
git push
git status --short --branch
```

Final status must show the branch up to date with origin. If a deployment-only
step remains private/operator-dependent, leave the bead open or deferred with a
sanitized blocker note rather than claiming completion.
