// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  mountOperatorApp,
  type RuntimeEventClient,
  type RuntimeInputClient,
  type RuntimePreviewClient,
  type RuntimeRunClient
} from "../../src/app";
import type { RuntimeSessionClient } from "../../src/authSession";
import type { RunStatusResponse, RuntimeWsMessage } from "../../src/runtimeClient";
import type { RuntimeConfig } from "../../src/runtimeConfig";

const config: RuntimeConfig = {
  schema_version: 1,
  api_base_path: "/api",
  ws_base_path: "/ws",
  allow_persistence: false
};

const capabilities = {
  input: true,
  preview: true,
  capture: true,
  labels: true,
  privileged_features: false,
  validation_runner: false
};

describe("mounted input UX", () => {
  beforeEach(() => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    setDocumentHidden(false);
  });

  afterEach(() => {
    document.body.replaceChildren();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    setDocumentHidden(false);
  });

  it("captures focused keyboard input, prevents defaults, and displays button names", async () => {
    const { root, socketClient } = await mountActiveInput();
    const surface = focusInputSurface(root);

    const down = dispatchKey(surface, "keydown", "KeyX");
    const up = dispatchKey(window, "keyup", "KeyX");

    expect(down.defaultPrevented).toBe(true);
    expect(up.defaultPrevented).toBe(true);
    expect(socketClient.sendInput).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ source: "combined", buttons: ["A"] })
    );
    expect(socketClient.sendInput).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ source: "combined", buttons: [] })
    );
    expect(root.textContent).toContain("Pressed: none");
    expect(root.textContent).toContain("console16-12btn-v1");
  });

  it("ignores repeated mapped keydown events after preventing their defaults", async () => {
    const { root, socketClient } = await mountActiveInput();
    dispatchKey(focusInputSurface(root), "keydown", "KeyZ");
    socketClient.sendInput.mockClear();

    const repeat = dispatchKey(focusInputSurface(root), "keydown", "KeyZ", { repeat: true });

    expect(repeat.defaultPrevented).toBe(true);
    expect(socketClient.sendInput).not.toHaveBeenCalled();
  });

  it("releases pressed input on focus loss, blur, page hidden, and session stop", async () => {
    await expectLifecycleRelease((root) => {
      const outside = document.createElement("button");
      document.body.append(outside);
      focusInputSurface(root);
      outside.focus();
    });
    await expectLifecycleRelease(() => {
      window.dispatchEvent(new Event("blur"));
    });
    await expectLifecycleRelease(() => {
      setDocumentHidden(true);
      document.dispatchEvent(new Event("visibilitychange"));
    });
    setDocumentHidden(false);

    const { root, client, socketClient } = await mountActiveInput();
    dispatchKey(focusInputSurface(root), "keydown", "KeyX");
    socketClient.sendInput.mockClear();
    root.querySelector<HTMLButtonElement>("[data-session-action='logout']")?.click();

    expect(client.stopSession).toHaveBeenCalledWith("session-001");
    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: [] })
    );
  });

  it("releases held input when runtime events stop or disable input", async () => {
    const { root, socketClient } = await mountActiveInput();
    dispatchKey(focusInputSurface(root), "keydown", "KeyX");
    socketClient.sendInput.mockClear();

    socketClient.emitEvent(runUpdated({ state: "stopped" }));

    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: [] })
    );
    expect(root.querySelector("form[data-session-form='start']")).not.toBeNull();

    const paused = await mountActiveInput();
    dispatchKey(focusInputSurface(paused.root), "keydown", "KeyZ");
    paused.socketClient.sendInput.mockClear();
    paused.socketClient.emitEvent(runUpdated({ state: "paused" }));

    expect(paused.socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: [] })
    );
    expect(paused.root.textContent).toContain("Pressed: none");

    const stale = await mountActiveInput();
    stale.root
      .querySelector<HTMLButtonElement>("[data-pad-button='B']")
      ?.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true }));
    stale.socketClient.sendInput.mockClear();
    stale.socketClient.emitEvent(runUpdated({ preview_stale: true }));

    expect(stale.socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: [] })
    );
    expect(stale.root.textContent).toContain("Pressed: none");
  });

  it("clears displayed input state on input socket close and reconnect", async () => {
    const { root, socketClient } = await mountActiveInput();
    dispatchKey(focusInputSurface(root), "keydown", "KeyX");
    expect(root.textContent).toContain("Pressed: A");

    socketClient.triggerClose();
    expect(root.textContent).toContain("Pressed: none");

    dispatchKey(focusInputSurface(root), "keydown", "KeyZ");
    expect(root.textContent).toContain("Pressed: B");

    socketClient.triggerReconnect();

    expect(root.textContent).toContain("Pressed: none");
  });

  it("restarts focused gamepad polling after an idle input socket reconnect", async () => {
    const raf = installAnimationFrame();
    let currentGamepads: Array<Gamepad | null> = [standardGamepad({ pressed: [] })];
    Object.defineProperty(navigator, "getGamepads", {
      configurable: true,
      value: vi.fn(() => currentGamepads)
    });
    const { root, socketClient } = await mountActiveInput();

    focusInputSurface(root);
    socketClient.triggerClose();
    currentGamepads = [standardGamepad({ pressed: [1] })];
    socketClient.triggerReconnect();
    raf.runNext();

    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: ["A"] })
    );
  });

  it("polls Standard Gamepad state, applies analog release, and clears on disconnect", async () => {
    const raf = installAnimationFrame();
    let currentGamepads: Array<Gamepad | null> = [standardGamepad({ pressed: [0] })];
    Object.defineProperty(navigator, "getGamepads", {
      configurable: true,
      value: vi.fn(() => currentGamepads)
    });
    const { root, socketClient } = await mountActiveInput();

    focusInputSurface(root);
    raf.runNext();
    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: ["B"] })
    );
    expect(root.textContent).toContain("Pressed: B");

    currentGamepads = [standardGamepad({ axes: [0.6, 0] })];
    raf.runNext();
    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: ["Right"] })
    );

    currentGamepads = [standardGamepad({ axes: [0.49, 0] })];
    raf.runNext();
    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: [] })
    );

    currentGamepads = [standardGamepad({ pressed: [1] })];
    raf.runNext();
    socketClient.sendInput.mockClear();
    window.dispatchEvent(new Event("gamepaddisconnected"));
    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: [] })
    );
  });

  it("merges keyboard and gamepad sources while displaying neutralized opposite directions", async () => {
    const raf = installAnimationFrame();
    let currentGamepads: Array<Gamepad | null> = [standardGamepad({ pressed: [13, 14] })];
    Object.defineProperty(navigator, "getGamepads", {
      configurable: true,
      value: vi.fn(() => currentGamepads)
    });
    const { root, socketClient } = await mountActiveInput();

    dispatchKey(focusInputSurface(root), "keydown", "ArrowUp");
    raf.runNext();

    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: ["Left"] })
    );
    expect(root.textContent).toContain("Pressed: Left");
    expect(root.textContent).toContain("Neutralized: Up/Down");

    currentGamepads = [standardGamepad({ pressed: [] })];
    raf.runNext();
    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: ["Up"] })
    );
  });

  it("restarts Standard Gamepad polling when a focused input surface becomes enabled again", async () => {
    const raf = installAnimationFrame();
    let currentGamepads: Array<Gamepad | null> = [standardGamepad({ pressed: [0] })];
    Object.defineProperty(navigator, "getGamepads", {
      configurable: true,
      value: vi.fn(() => currentGamepads)
    });
    const { root, socketClient } = await mountActiveInput();

    focusInputSurface(root);
    raf.runNext();
    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: ["B"] })
    );

    socketClient.sendInput.mockClear();
    socketClient.emitEvent(runUpdated({ state: "paused" }));
    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: [] })
    );

    currentGamepads = [standardGamepad({ pressed: [1] })];
    socketClient.sendInput.mockClear();
    socketClient.emitEvent(runUpdated({ state: "running", preview_stale: false }));
    raf.runNext();

    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: ["A"] })
    );
  });

  it("keeps fixed keyboard mappings active when a visible pad button is focused", async () => {
    const { root, socketClient } = await mountActiveInput();
    const button = root.querySelector<HTMLButtonElement>("[data-pad-button='Start']");
    expect(button).not.toBeNull();
    button!.focus();
    const focusedButton = root.querySelector<HTMLButtonElement>("[data-pad-button='Start']");
    expect(focusedButton).not.toBeNull();

    const mappedDown = dispatchKey(focusedButton!, "keydown", "KeyX");
    expect(mappedDown.defaultPrevented).toBe(true);
    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: ["A"] })
    );

    dispatchKey(window, "keyup", "KeyX");
    socketClient.sendInput.mockClear();
    const activationButton = root.querySelector<HTMLButtonElement>("[data-pad-button='Start']");
    expect(activationButton).not.toBeNull();
    activationButton!.focus();
    const enterDown = dispatchKey(activationButton!, "keydown", "Enter");
    window.dispatchEvent(new KeyboardEvent("keyup", { key: "Enter", bubbles: true }));

    expect(enterDown.defaultPrevented).toBe(true);
    expect(socketClient.sendInput).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ source: "combined", buttons: ["Start"] })
    );
    expect(socketClient.sendInput).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ source: "combined", buttons: [] })
    );
  });
});

type MockRuntimeClient = RuntimeSessionClient & Partial<RuntimePreviewClient & RuntimeRunClient>;

async function expectLifecycleRelease(trigger: (root: HTMLElement) => void | Promise<void>): Promise<void> {
  const { root, socketClient } = await mountActiveInput();
  dispatchKey(focusInputSurface(root), "keydown", "KeyX");
  socketClient.sendInput.mockClear();
  await trigger(root);
  await flushPromises();

  expect(socketClient.sendInput).toHaveBeenLastCalledWith(
    expect.objectContaining({ source: "combined", buttons: [] })
  );
  expect(root.textContent).toContain("Pressed: none");
}

async function mountActiveInput(): Promise<{
  root: HTMLElement;
  client: MockRuntimeClient;
  socketClient: ReturnType<typeof mockSocketClient>;
}> {
  const socketClient = mockSocketClient();
  const client = mockClient({
    sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
    runStatus: vi.fn().mockResolvedValue(runStatusResponse({ preview_stale: false })),
    currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse(12, false))
  });
  const root = document.createElement("div");
  document.body.append(root);

  mountOperatorApp(root, config, client, socketClient.client);
  await flushPromises();
  socketClient.sendInput.mockClear();
  return { root, client, socketClient };
}

function focusInputSurface(root: HTMLElement): HTMLElement {
  const surface = root.querySelector<HTMLElement>("[data-input-focus-surface]");
  expect(surface).not.toBeNull();
  surface!.focus();
  const focusedSurface = root.querySelector<HTMLElement>("[data-input-focus-surface]");
  expect(focusedSurface).not.toBeNull();
  return focusedSurface!;
}

function dispatchKey(
  target: EventTarget,
  type: "keydown" | "keyup",
  code: string,
  options: { repeat?: boolean } = {}
): KeyboardEvent {
  const event = new KeyboardEvent(type, {
    bubbles: true,
    cancelable: true,
    code,
    key: keyForCode(code),
    repeat: options.repeat ?? false
  });
  target.dispatchEvent(event);
  return event;
}

function keyForCode(code: string): string {
  if (code === "Enter") {
    return "Enter";
  }
  if (code === "Space") {
    return " ";
  }
  if (code.startsWith("Arrow")) {
    return code;
  }
  return "";
}

function installAnimationFrame(): { runNext: () => void } {
  let nextId = 1;
  const callbacks = new Map<number, FrameRequestCallback>();
  vi.stubGlobal(
    "requestAnimationFrame",
    vi.fn((callback: FrameRequestCallback) => {
      const id = nextId;
      nextId += 1;
      callbacks.set(id, callback);
      return id;
    })
  );
  vi.stubGlobal(
    "cancelAnimationFrame",
    vi.fn((id: number) => {
      callbacks.delete(id);
    })
  );
  return {
    runNext: () => {
      const [id, callback] = callbacks.entries().next().value ?? [];
      if (id !== undefined && callback) {
        callbacks.delete(id);
        callback(performance.now());
      }
    }
  };
}

function mockClient(overrides: Partial<MockRuntimeClient> = {}): MockRuntimeClient {
  return {
    startSession: vi.fn(),
    sessionStatus: vi.fn().mockResolvedValue({ schema_version: 1, active: false, state: "idle" }),
    stopSession: vi.fn().mockResolvedValue({ schema_version: 1, session_id: "session-001", state: "stopped", final_frame: 12 }),
    runStatus: vi.fn().mockResolvedValue(runStatusResponse()),
    pauseRun: vi.fn(),
    resumeRun: vi.fn(),
    currentFrame: vi.fn(),
    triggerCapture: vi.fn(),
    captureJob: vi.fn(),
    ...overrides
  } as MockRuntimeClient;
}

function mockSocketClient(): {
  client: RuntimeEventClient & RuntimeInputClient;
  sendInput: ReturnType<typeof vi.fn>;
  triggerClose: () => void;
  triggerReconnect: () => void;
  emitEvent: (message: RuntimeWsMessage) => void;
  emitInput: (message: RuntimeWsMessage) => void;
} {
  let onEventMessage: ((message: RuntimeWsMessage) => void) | undefined;
  let onInputMessage: ((message: RuntimeWsMessage) => void) | undefined;
  let onInputClose: (() => void) | undefined;
  let onInputReconnect: (() => void) | undefined;
  const sendInput = vi.fn();
  return {
    client: {
      eventSocket: vi.fn((handlers = {}) => {
        onEventMessage = handlers.onMessage;
        return { close: vi.fn() } as unknown as ReturnType<RuntimeEventClient["eventSocket"]>;
      }),
      inputSocket: vi.fn((_, __, handlers = {}) => {
        onInputMessage = handlers.onMessage;
        onInputClose = handlers.onClose;
        onInputReconnect = handlers.onReconnect;
        return {
          close: vi.fn(),
          sendInput
        } as unknown as ReturnType<RuntimeInputClient["inputSocket"]>;
      })
    },
    sendInput,
    triggerClose: () => onInputClose?.(),
    triggerReconnect: () => onInputReconnect?.(),
    emitEvent: (message) => onEventMessage?.(message),
    emitInput: (message) => onInputMessage?.(message)
  };
}

function activeSessionResponse() {
  return {
    schema_version: 1,
    active: true,
    session_id: "session-001",
    run_id: "run-001",
    state: "running",
    current_frame: 12,
    backend_mode: "synthetic"
  };
}

function runStatusResponse(overrides: Partial<RunStatusResponse> = {}): RunStatusResponse {
  return {
    schema_version: 1,
    session_id: "session-001",
    run_id: "run-001",
    state: "running",
    backend_mode: "synthetic",
    current_frame: 12,
    last_applied_input_frame: 10,
    last_preview_frame: 11,
    preview_stale: false,
    active_capture_job_id: null,
    capabilities,
    ...overrides
  };
}

function frameCurrentResponse(frame = 21, stale = false) {
  return {
    schema_version: 1,
    frame,
    captured_at: "1970-01-01T00:00:00Z",
    stale,
    width: 256,
    height: 224,
    format: "image/png",
    image_url: `/api/frame/current/image?frame=${frame}`,
    preview_hash: "sha256:0123456789abcdef"
  };
}

function runUpdated(
  overrides: Partial<Extract<RuntimeWsMessage, { type: "run_updated" }>["payload"]> = {}
): RuntimeWsMessage {
  return {
    schema_version: 1,
    type: "run_updated",
    session_id: "session-001",
    client_seq: null,
    source_id: "server",
    server_seq: 1,
    payload: {
      state: "running",
      current_frame: 12,
      preview_stale: false,
      active_capture_job_id: null,
      ...overrides
    }
  };
}

function standardGamepad(input: { pressed?: number[]; axes?: number[] }): Gamepad {
  const pressedButtons = new Set(input.pressed ?? []);
  return {
    id: "standard-gamepad",
    index: 0,
    connected: true,
    mapping: "standard",
    timestamp: performance.now(),
    buttons: Array.from({ length: 16 }, (_, index) => ({
      pressed: pressedButtons.has(index),
      touched: pressedButtons.has(index),
      value: pressedButtons.has(index) ? 1 : 0
    })),
    axes: input.axes ?? [0, 0, 0, 0],
    vibrationActuator: null
  } as unknown as Gamepad;
}

function setDocumentHidden(hidden: boolean): void {
  Object.defineProperty(document, "hidden", {
    configurable: true,
    value: hidden
  });
}

async function flushPromises(): Promise<void> {
  for (let index = 0; index < 5; index += 1) {
    await Promise.resolve();
  }
}
