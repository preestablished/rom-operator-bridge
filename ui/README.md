# ROM Operator Bridge UI

Static TypeScript operator UI scaffold for `https://rombridge.birb.homes/`.

## Commands

```sh
npm --prefix ui ci
npm --prefix ui run dev
npm --prefix ui run typecheck
npm --prefix ui test -- --run
npm --prefix ui run build
```

The frozen bridge-stack gate also runs the service checks from the repository
root before these UI commands.

## Runtime Config

The static bundle loads `public/runtime-config.json` at `/runtime-config.json`
with `cache: "no-store"` and `credentials: "same-origin"`. The config may set
same-origin `api_base_path` and `ws_base_path` values only. It must not contain
credentials, tokens, private paths, or other operator secrets.

The scaffold does not register a service worker and does not use browser
persistence APIs.
