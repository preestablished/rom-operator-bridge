# Verified Deployment State

## Release Deployment

The operator successfully built and deployed commit `b4bcc8fdba06`:

```text
install-release: PASS deployed release_id=20260701T155351Z commit=b4bcc8fdba06
install-release: PASS service and static current symlinks updated
install-release: PASS rom-operator-bridge.service active
```

The service is active and listens on:

```text
10.0.0.106:7410
```

The health endpoint reports real backend mode:

```json
{"schema_version":1,"ok":true,"service_version":"0.1.0","backend_mode":"real","runtime_api":1}
```

## Static UI

Verified good:

```text
https://rombridge.birb.homes/     -> 200 OK, text/html
http://tailrombridge.birb.homes/  -> 200 OK, text/html
```

The Tailscale route returns the expected CSP:

```text
connect-src 'self' ws://tailrombridge.birb.homes
```

The primary HTTPS route returns the expected CSP:

```text
connect-src 'self' wss://rombridge.birb.homes
```

## Tailscale Host Routing

These probes were important:

```sh
curl -i http://100.82.43.93/api/session
```

Result: Apache 404. This is expected because the bare IP hits the fallback
Apache route.

```sh
curl -i \
  -H 'Host: tailrombridge.birb.homes' \
  -H 'Origin: http://tailrombridge.birb.homes' \
  http://100.82.43.93/api/session
```

Result: bridge API `401 session_inactive`. This confirms the route is
Host-header based and healthy when addressed as the Tailscale hostname.

## Tailscale Profile Env Update

The operator added the non-secret deployment profile keys for the Tailscale
HTTP origin to the private env file and restarted the service. After that:

```text
Host: tailrombridge.birb.homes -> 200 OK, text/html
```

Do not print or commit the private env file. The non-secret shape added was:

```text
ROM_OPERATOR_BRIDGE_DEPLOYMENT_PROFILES=https-origin,tailscale-http
ROM_OPERATOR_BRIDGE_PROFILE_HTTPS_ORIGIN_PUBLIC_ORIGIN=https://rombridge.birb.homes
ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_PUBLIC_ORIGIN=http://tailrombridge.birb.homes
ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_ALLOWED_ORIGINS=http://tailrombridge.birb.homes
ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_COOKIE_SECURE=false
ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_EXPOSURE_MODE=tailscale-http
```

