import { describe, expect, it } from "vitest";
import {
  BACKEND_MODES,
  CAPABILITY_NAMES,
  CAPTURE_STATUSES,
  ERROR_CODES,
  INPUT_SOURCES,
  LABEL_ROLES,
  PAD_LAYOUT_ID,
  PAD_LAYOUT_VERSION,
  PAD_BUTTONS,
  RUNTIME_API_SCHEMA_VERSION,
  SESSION_STATES,
  VALIDATION_STATUSES,
  WS_MESSAGE_TYPES
} from "../src/runtimeContract";

describe("runtime contract constants", () => {
  it("comes from the shared schema", () => {
    expect(RUNTIME_API_SCHEMA_VERSION).toBe(1);
    expect(PAD_LAYOUT_ID).toBe("console16-12btn-v1");
    expect(PAD_LAYOUT_VERSION).toBe(1);
    expect(BACKEND_MODES).toEqual(["synthetic", "real"]);
    expect(SESSION_STATES).toEqual([
      "idle",
      "starting",
      "running",
      "paused",
      "capture_pending",
      "stopping",
      "stopped",
      "faulted"
    ]);
    expect(VALIDATION_STATUSES).toEqual(["not_run", "running", "passed", "failed"]);
    expect(CAPABILITY_NAMES).toEqual([
      "input",
      "preview",
      "capture",
      "labels",
      "privileged_features",
      "validation_runner"
    ]);
    expect(CAPTURE_STATUSES).toEqual([
      "requested",
      "capturing",
      "completed",
      "failed",
      "not_labelable"
    ]);
    expect(LABEL_ROLES).toEqual([
      "first_boss",
      "goal_positive",
      "goal_negative",
      "needs_review",
      "rejected"
    ]);
    expect(INPUT_SOURCES).toEqual(["keyboard", "gamepad", "combined"]);
    expect(PAD_BUTTONS).toEqual([
      "A",
      "B",
      "X",
      "Y",
      "L",
      "R",
      "Up",
      "Down",
      "Left",
      "Right",
      "Start",
      "Select"
    ]);
    expect(WS_MESSAGE_TYPES).toEqual([
      "input_state",
      "input_ack",
      "input_reject",
      "session_updated",
      "run_updated",
      "capture_updated",
      "label_updated",
      "validation_updated"
    ]);
    expect(ERROR_CODES).toEqual([
      "auth_rejected",
      "origin_rejected",
      "session_inactive",
      "session_active_elsewhere",
      "backend_unavailable",
      "frame_stale",
      "capture_in_progress",
      "capture_failed",
      "label_conflict",
      "validation_failed",
      "bad_request"
    ]);
  });
});
