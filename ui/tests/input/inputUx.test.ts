import { describe, expect, it } from "vitest";
import type { PadButton } from "../../src/runtimeClient";
import {
  buttonsFromStandardGamepad,
  keyboardButtonForCode,
  mergeInputButtons
} from "../../src/inputUx";

describe("input UX mappings", () => {
  it("maps every fixed keyboard code to console16-12btn-v1 names", () => {
    const mappings: Array<readonly [string, PadButton]> = [
      ["ArrowUp", "Up"],
      ["ArrowDown", "Down"],
      ["ArrowLeft", "Left"],
      ["ArrowRight", "Right"],
      ["KeyZ", "B"],
      ["KeyX", "A"],
      ["KeyA", "Y"],
      ["KeyS", "X"],
      ["KeyQ", "L"],
      ["KeyW", "R"],
      ["Enter", "Start"],
      ["ShiftRight", "Select"]
    ];

    for (const [code, button] of mappings) {
      expect(keyboardButtonForCode(code)).toBe(button);
    }
    expect(keyboardButtonForCode("ShiftLeft")).toBeNull();
  });

  it("maps every Standard Gamepad button to console16-12btn-v1 names", () => {
    const mappings: Array<readonly [number, PadButton]> = [
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

    for (const [index, button] of mappings) {
      expect(buttonsFromStandardGamepad(gamepad({ pressed: [index] }))).toEqual([button]);
    }
  });

  it("applies the analog deadzone to Standard Gamepad axes", () => {
    expect(buttonsFromStandardGamepad(gamepad({ axes: [0.49, -0.49] }))).toEqual([]);
    expect(buttonsFromStandardGamepad(gamepad({ axes: [-0.5, -0.5] }))).toEqual(["Up", "Left"]);
    expect(buttonsFromStandardGamepad(gamepad({ axes: [0.5, 0.5] }))).toEqual(["Down", "Right"]);
  });

  it("merges sources, neutralizes opposite directions, and releases zero-button states", () => {
    expect(
      mergeInputButtons([
        ["A", "Up"],
        ["B", "Down"],
        ["Left"],
        ["Right"]
      ])
    ).toEqual({
      buttons: ["A", "B"],
      neutralizedDirections: ["Up/Down", "Left/Right"]
    });
    expect(buttonsFromStandardGamepad(gamepad({ pressed: [0] }))).toEqual(["B"]);
    expect(buttonsFromStandardGamepad(gamepad({ pressed: [] }))).toEqual([]);
  });
});

function gamepad(input: { pressed?: number[]; axes?: number[] }): Pick<Gamepad, "buttons" | "axes"> {
  const pressedButtons = new Set(input.pressed ?? []);
  return {
    buttons: Array.from({ length: 16 }, (_, index) => ({
      pressed: pressedButtons.has(index),
      touched: pressedButtons.has(index),
      value: pressedButtons.has(index) ? 1 : 0
    })),
    axes: input.axes ?? [0, 0, 0, 0]
  };
}
