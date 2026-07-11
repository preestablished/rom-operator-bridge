# Play Metrics And Bandwidth Decision

## 1. Instrument The Required Signals

Add stable structured tracing around one Play session. Use an ephemeral
play-instance ordinal or random identifier created for metrics, never a cookie,
session ID, run ID, capture ID, or private path. Include backend mode and
cumulative counters where appropriate.

Required signals:

| Signal | Measurement point |
|---|---|
| frames produced | per-Play successful `PlayStreamEvent::Frame` / fallback `play_step` |
| server send inputs | per-socket successful binary sends, plus elapsed observation time |
| pacer overruns | deadline missed; distinguish ordinary lateness from `PLAY_PACER_RESYNC` reanchors |
| stream restarts | existing budget-end/reopen telemetry, retaining success/failure classification |
| frame bytes | websocket payload length, separating the 8-byte counter from PNG bytes |
| subscriber pressure | active subscribers and per-socket frame-counter gaps, described as retained-depth 0/1 with attribution limits |
| disconnects/send failures | websocket close/error classification without cookies or peer identity |

Do not label produced frames or socket-sink acceptance as delivered frames.
Multiple subscribers must not inflate per-Play producer totals. Maintain
per-Play producer counters and per-socket counters with an ephemeral subscriber
ordinal; emit aggregates periodically and at disconnect/terminal rather than a
60 Hz tracing event. The scripted client remains the authority for end-to-end
delivered count, counter gaps, latency, and link bandwidth.

Add tests that capture tracing output or exercise an extracted counter object.
They must prove counters increment once per event, pacer resync is distinct
from a small miss, byte accounting excludes/includes the prefix as documented,
and socket counter gaps are not misreported as exact watch-overwrite counts.

Document the field names and interpretation in a public sanitized operations
note under `docs/` or the eventual request resolution so the private EQB script
does not depend on source archaeology.

## 2. Gather Decision Inputs

During EQB record, privately, for both the contained and raised-budget runs:

- observation seconds, received frames, delivered fps, counter gaps, and
  disconnects;
- p50/p95/max client inter-arrival and the rider's boundary-stall series;
- total websocket bytes and PNG-only bytes, yielding Mbps and bytes/frame;
- produced-frame and successful-send counts from bridge telemetry;
- pacer miss/resync counts, reopen counts, and subscriber coalescing; and
- worker/bridge builds and effective stream budget.

Publish only aggregate sanitized values. Compute bandwidth consistently:

```text
png_mbps = png_bytes * 8 / observation_seconds / 1_000_000
wire_payload_mbps = websocket_payload_bytes * 8 / observation_seconds / 1_000_000
projected_mbps_at_rate = mean_png_bytes * target_fps * 8 / 1_000_000
```

State whether websocket/TLS framing overhead is included. Do not reuse the old
172 KB/frame or 81 Mbps estimates when measured values are available.

## 3. Make And Record The `pea` Decision

Choose among the following based on evidence, recording rejected alternatives:

- retain current PNG behavior if real-link utilization has safe headroom at
  current fps and the next credible performance step still does not threaten
  the link;
- tune PNG compression only if a release-build benchmark shows material byte
  savings without reducing delivered fps or increasing pacer/boundary stalls;
- add adaptive frame skipping only if producer rate exceeds sustainable client
  delivery and newest-frame semantics remain explicit; or
- plan downscale/another format only if measured bandwidth, browser decode, or
  CPU evidence shows PNG cannot meet the next owned target.

Do not implement a format/protocol change speculatively inside `pea`. Such a
change needs compatibility, browser decode, CPU, and privacy tests and should
be a separately accepted bead. If current PNG is adequate, close `pea` with
the aggregate table, metric-field documentation, and a numeric revisit
trigger—for example a sustained target fps or measured link-utilization
threshold—not a vague “revisit later.”

Fold the metrics/readout portion of `qh4` into `pea`. Keep the zero-copy fanout
and LIVE/buffering UI polish on `qh4` unless this work actually implements and
tests them; update its description/notes so there is no duplicate metrics item.
