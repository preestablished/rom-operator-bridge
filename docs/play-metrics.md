# Play Metrics

Continuous Play emits aggregate structured tracing events. These events contain
ephemeral ordinals only; they never include cookies, session IDs, run IDs,
capture IDs, peer addresses, or private paths.

## `play_loop_summary`

Emitted once when a streaming or fallback Play loop exits.

| Field | Meaning |
|---|---|
| `play_ordinal` | Process-local ephemeral correlation ordinal |
| `path` | `streaming` or `fallback` |
| `elapsed_ms` | Loop lifetime |
| `produced_frames` | Frames produced by the backend and published to the retained slot |
| `png_bytes` | PNG bytes produced, excluding the eight-byte frame prefix |
| `pacer_deadline_misses` | Streaming ticks whose absolute deadline was already past |
| `pacer_resyncs` | Deadline misses beyond the 250 ms resynchronization threshold |

Existing `play stream budget segment reopened` events supply successful and
failed stream-restart counts, reopen duration, and private aggregate icount
inputs for the EQB boundary calculation.

## `play_frame_socket_summary`

Emitted once when a `/ws/frames` connection exits.

| Field | Meaning |
|---|---|
| `socket_ordinal` | Process-local ephemeral subscriber ordinal |
| `elapsed_ms` | Connection lifetime |
| `sent_frames` | Frames accepted by the server websocket sink; not proof of browser receipt |
| `websocket_payload_bytes` | Eight-byte prefixes plus PNG bytes accepted by the sink |
| `png_bytes` | PNG bytes accepted by the sink |
| `inferred_counter_gaps` | Positive gaps between successive sink-accepted frame counters |
| `retained_depth_max` | Always `1`, reflecting the Tokio `watch` newest-frame slot |
| `active_subscribers` | Remaining connected frame sockets when this socket exits |

Counter gaps indicate that intermediate frames were not sent on that socket,
but cannot distinguish producer counter jumps from `watch` overwrites. Multiple
subscribers have independent socket summaries and do not inflate the
per-Play producer count.

The authenticated EQB client remains authoritative for delivered fps,
disconnects, client-visible counter gaps, latency, and link bandwidth. Compute
aggregate bandwidth as:

```text
png_mbps = png_bytes * 8 / observation_seconds / 1_000_000
payload_mbps = websocket_payload_bytes * 8 / observation_seconds / 1_000_000
projected_mbps = mean_png_bytes * target_fps * 8 / 1_000_000
```

State separately whether websocket and TLS framing overhead was measured.
