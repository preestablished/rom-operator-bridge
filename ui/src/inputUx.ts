import type { PadButton } from "./runtimeClient";
import { PAD_BUTTONS } from "./runtimeContract";

export const GAMEPAD_ANALOG_DEADZONE = 0.5;

export const KEYBOARD_CODE_TO_BUTTON: Record<string, PadButton> = {
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  KeyZ: "B",
  KeyX: "A",
  KeyA: "Y",
  KeyS: "X",
  KeyQ: "L",
  KeyW: "R",
  Enter: "Start",
  ShiftRight: "Select"
};

const GAMEPAD_BUTTON_TO_PAD: ReadonlyArray<readonly [number, PadButton]> = [
  [0, "B"],
  [1, "A"],
  [2, "Y"],
  [3, "X"],
  [4, "L"],
  [5, "R"],
  [8, "Select"],
  [9, "Start"],
  [12, "Up"],
  [13, "Down"],
  [14, "Left"],
  [15, "Right"]
];

export type NeutralizedDirection = "Up/Down" | "Left/Right";

export type MergedInputButtons = {
  buttons: PadButton[];
  neutralizedDirections: NeutralizedDirection[];
};

export function keyboardButtonForCode(code: string): PadButton | null {
  return KEYBOARD_CODE_TO_BUTTON[code] ?? null;
}

export function buttonsFromStandardGamepad(
  gamepad: Pick<Gamepad, "buttons" | "axes"> | null | undefined,
  deadzone = GAMEPAD_ANALOG_DEADZONE
): PadButton[] {
  if (!gamepad) {
    return [];
  }

  const buttons = new Set<PadButton>();
  for (const [index, button] of GAMEPAD_BUTTON_TO_PAD) {
    const gamepadButton = gamepad.buttons[index];
    if (gamepadButton?.pressed || (gamepadButton?.value ?? 0) >= 0.5) {
      buttons.add(button);
    }
  }

  const xAxis = gamepad.axes[0] ?? 0;
  const yAxis = gamepad.axes[1] ?? 0;
  if (yAxis <= -deadzone) {
    buttons.add("Up");
  }
  if (yAxis >= deadzone) {
    buttons.add("Down");
  }
  if (xAxis <= -deadzone) {
    buttons.add("Left");
  }
  if (xAxis >= deadzone) {
    buttons.add("Right");
  }

  return sortPadButtons([...buttons]);
}

export function mergeInputButtons(sources: PadButton[][]): MergedInputButtons {
  const merged = new Set<PadButton>();
  for (const source of sources) {
    for (const button of source) {
      merged.add(button);
    }
  }

  const neutralizedDirections: NeutralizedDirection[] = [];
  if (merged.has("Up") && merged.has("Down")) {
    merged.delete("Up");
    merged.delete("Down");
    neutralizedDirections.push("Up/Down");
  }
  if (merged.has("Left") && merged.has("Right")) {
    merged.delete("Left");
    merged.delete("Right");
    neutralizedDirections.push("Left/Right");
  }

  return {
    buttons: sortPadButtons([...merged]),
    neutralizedDirections
  };
}

export function samePadButtons(left: PadButton[], right: PadButton[]): boolean {
  if (left.length !== right.length) {
    return false;
  }
  return left.every((button, index) => button === right[index]);
}

export function sortPadButtons(buttons: PadButton[]): PadButton[] {
  const unique = new Set(buttons);
  return PAD_BUTTONS.filter((button) => unique.has(button));
}
