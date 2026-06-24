# Tests And Quality Gates

## Baseline Checks

Before changing any bridge code, confirm the existing RestoreSnapshot mock
coverage still passes:

```bash
cd /home/infra-admin/git/preestablished/rom-operator-bridge
cargo test --manifest-path service/Cargo.toml --test real-backend real_restore_snapshot_lifecycle_calls_worker_and_stays_sanitized
```

Run the wider real-backend test target if time allows:

```bash
cargo test --manifest-path service/Cargo.toml --test real-backend
```

## If No Code Changes Are Needed

If the live acceptance run passes without repository code changes, no new test
is required. Update the bead with sanitized evidence and complete the normal bd
and git handoff.

Still run a repository status check before committing plan or note changes:

```bash
git status --short
```

## If Code Changes Are Needed

Keep code changes scoped to the observed bridge-owned failure. Then run:

```bash
cargo fmt --all --manifest-path service/Cargo.toml
cargo test --manifest-path service/Cargo.toml --test real-backend
cargo test --manifest-path service/Cargo.toml
```

If the repository has a current quality-gate script, run it after targeted tests:

```bash
./scripts/quality-gate.sh
```

If a gate cannot run because the host lacks a dependency, update the bead with
the exact sanitized reason and keep the failure visible in the handoff.

## External Component Checks

If diagnosing external readiness, keep checks in the owning repository:

```bash
cd /home/infra-admin/git/preestablished/snapshot-store
cargo test -p snapstore-cli

cd /home/infra-admin/git/preestablished/determinism-hypervisor
cargo test -p dh-worker
```

Do not commit unrelated external repository changes as part of this bead.

## Leak Checks

Before any commit, run the forbidden literal sweep described in
`04-evidence-sanitization-and-failure-handling.md` against:

- this repository;
- any sanitized evidence file planned for commit;
- any bead note body before sending, if you drafted it in a local file.

Use the quiet `rg -q` sweep form from that file so matches do not print private
values. The sweep must find no matches.
