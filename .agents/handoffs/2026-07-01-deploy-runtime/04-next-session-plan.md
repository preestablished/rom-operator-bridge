# Next Session Plan

## Goal

Resolve or explain the browser-visible state:

```text
Session faulted / synthetic / Runtime unavailable.
```

Close bead `rom-operator-bridge-xta` only after the user-visible behavior is
verified.

## Recommended Steps

1. Reconfirm current deployed health.

```sh
curl -i http://tailrombridge.birb.homes/ | head -40
curl -i http://tailrombridge.birb.homes/health
curl -i -H 'Origin: http://tailrombridge.birb.homes' \
  http://tailrombridge.birb.homes/api/session
```

Expected:

- UI returns `200 OK text/html`.
- Health returns `backend_mode: real`.
- Session returns `401 session_inactive` when no operator session cookie exists.

2. Ask or verify which URL the user opened.

Correct:

```text
http://tailrombridge.birb.homes/
https://rombridge.birb.homes/
```

Incorrect for browser use:

```text
http://100.82.43.93/
```

3. Reproduce in a browser-equivalent environment.

Use Playwright if available. Capture:

- console errors,
- failed network requests,
- response status and content type for `/runtime-config.json`, `/health`,
  `/api/session`, and `/api/session/start`,
- whether the page is using the current asset path from the deployed HTML.

4. Try a clean browser state.

Have the user hard refresh or open a private/incognito window at:

```text
http://tailrombridge.birb.homes/
```

Then try the Start button once. If the UI still shows `Runtime unavailable`,
inspect the exact network failure.

5. If `/api/session/start` succeeds in curl but fails in the browser, compare:

- request URL,
- Host header,
- Origin header,
- method,
- cookie behavior,
- response body/content type,
- CORS headers,
- whether the request is going to Apache fallback instead of the bridge.

6. If the browser startup state is simply misleading, consider a UI fix:

- call `/health` on startup before first settled render so the pill shows
  `real` instead of the initial `synthetic` fallback;
- treat `session_inactive` during initial refresh as `locked`/startable rather
  than surfacing scary recovery copy;
- add a targeted test around startup when `/api/session` returns
  `session_inactive`.

7. If browser Start creates a session but the UI later faults, inspect WebSocket
routes:

```text
ws://tailrombridge.birb.homes/ws/events
ws://tailrombridge.birb.homes/ws/input
wss://rombridge.birb.homes/ws/events
wss://rombridge.birb.homes/ws/input
```

The static CSP is already profile-specific for these schemes.

## Closeout

Before ending the next session:

- update or close `rom-operator-bridge-xta`;
- run focused UI tests for any frontend change;
- run the repo quality gate if code changed;
- commit and push code/docs;
- run `bd dolt push`;
- verify `git status --short --branch` is clean and up to date.

