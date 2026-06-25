# Systemd Install And Runtime

## Goal

Create committed install material and private host steps that make the bridge
service active on the selected deployment host. The committed material should be
safe to review without private values, and the host-specific env file must stay
private.

## Repo Artifacts To Add

Add `deploy/systemd/rom-operator-bridge.service` as a sanitized template for the
installed service. It should include:

- `After=network-online.target`;
- `EnvironmentFile=/etc/rom-operator-bridge/rom-operator-bridge.env`;
- `ExecStart=/opt/rom-operator-bridge/current/rom-operator-bridge`;
- restart policy suitable for a private operator service;
- hardening that does not prevent writes to the private root;
- explicit read/write allowances for the private root and static publish root
  if hardening uses path restrictions;
- no credentials or host-specific private values.

If the deployment uses a dedicated service account, document its creation in
`deploy/README.md`. If it uses an existing operator account, document that as an
operator choice and do not hardcode private user-specific paths.

## Private Env Shape

The private env file should be installed at:

```text
/etc/rom-operator-bridge/rom-operator-bridge.env
```

Required public env names:

```sh
ROM_OPERATOR_BRIDGE_BIND_ADDR=<bridge-private-ip>:7410
ROM_OPERATOR_BRIDGE_BACKEND=<synthetic-or-real>
ROM_OPERATOR_BRIDGE_PRIVATE_ROOT=<absolute-private-runtime-root>
ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT=<absolute-static-publish-root>
ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL=<operator-secret>
ROM_OPERATOR_BRIDGE_SESSION_SECRET=<session-secret>
```

Real backend mode also requires the approved real backend values documented in
`01-current-state-and-inputs.md`.

Do not commit a filled env file. If an example file is added, it must contain
only placeholders and must not be named in a way that encourages installing it
as-is.

## Release Artifact Flow

Recommended private host sequence:

```sh
cargo build --manifest-path service/Cargo.toml --release
npm --prefix ui ci
npm --prefix ui run build
```

Install the binary and static UI through a private release directory, then move
the `current` symlink atomically. Use non-interactive flags for file operations.

Sanitized command shape:

```sh
sudo install -d -m 0755 /opt/rom-operator-bridge/releases/<release-id>
sudo install -m 0755 service/target/release/rom-operator-bridge-service \
  /opt/rom-operator-bridge/releases/<release-id>/rom-operator-bridge
sudo install -d -m 0755 <absolute-static-publish-root>
sudo cp -rf ui/dist/. <absolute-static-publish-root>/
sudo ln -sfn /opt/rom-operator-bridge/current /opt/rom-operator-bridge/previous
sudo ln -sfn /opt/rom-operator-bridge/releases/<release-id> \
  /opt/rom-operator-bridge/current
```

Adjust the binary source path if the package name or built binary name differs
after implementation. Keep private instantiated paths out of committed docs.

## Host Activation

Sanitized activation shape:

```sh
sudo install -d -m 0755 /etc/rom-operator-bridge
sudo install -m 0600 <private-env-source> \
  /etc/rom-operator-bridge/rom-operator-bridge.env
sudo install -m 0644 deploy/systemd/rom-operator-bridge.service \
  /etc/systemd/system/rom-operator-bridge.service
sudo systemctl daemon-reload
sudo systemctl restart rom-operator-bridge.service
sudo systemctl status --no-pager rom-operator-bridge.service
```

Do not paste status or journal output into public docs before sanitizing it.

## Local Service Verification

Run private checks on the host before touching K3s:

```sh
curl -fsS http://<bridge-private-ip>:7410/health
curl -i http://<bridge-private-ip>:7410/api/session
curl -I http://<bridge-private-ip>:7410/
```

Expected sanitized results:

- `/health` returns sanitized JSON;
- unauthenticated `/api/session` rejects without private details;
- root UI route returns expected security and cache headers;
- service logs do not include credentials, private refs, private paths, or raw
  request bodies.

## Rollback

Document rollback in `deploy/README.md`:

```sh
sudo systemctl stop rom-operator-bridge.service
sudo ln -sfn /opt/rom-operator-bridge/previous /opt/rom-operator-bridge/current
sudo systemctl restart rom-operator-bridge.service
```

If the private env file changed, restore the operator-private backup before
restart. Never commit the backup or its contents.
