import { describe, expect, it, vi } from "vitest";
import { LiveFrameController, type LiveFrameBitmap } from "../../src/liveFrame";

type FakeBitmap = LiveFrameBitmap & { name: string };

describe("LiveFrameController", () => {
  it("drops reordered frames and preserves u64 ordering above Number.MAX_SAFE_INTEGER", async () => {
    const painted: Array<FakeBitmap | null> = [];
    const decode = vi.fn(async (png: ArrayBuffer) => bitmap(String(new Uint8Array(png)[0])));
    const controller = new LiveFrameController<FakeBitmap>(decode, (value) => painted.push(value));

    await controller.receive(frame(9_007_199_254_740_992n, 1), "run-a");
    await controller.receive(frame(9_007_199_254_740_993n, 2), "run-a");
    await controller.receive(frame(9_007_199_254_740_992n, 3), "run-a");

    expect(decode).toHaveBeenCalledTimes(2);
    expect(controller.bitmap?.name).toBe("2");
    expect(painted.filter(Boolean).map((value) => value?.name)).toEqual(["1", "2"]);
  });

  it("does not paint backward when a newer decode resolves first", async () => {
    const first = deferred<FakeBitmap>();
    const second = deferred<FakeBitmap>();
    const painted: Array<FakeBitmap | null> = [];
    const decode = vi.fn((png: ArrayBuffer) =>
      new Uint8Array(png)[0] === 1 ? first.promise : second.promise
    );
    const controller = new LiveFrameController<FakeBitmap>(decode, (value) => painted.push(value));

    const older = controller.receive(frame(10n, 1), "run-a");
    const newer = controller.receive(frame(11n, 2), "run-a");
    const newerBitmap = bitmap("newer");
    second.resolve(newerBitmap);
    await newer;
    const olderBitmap = bitmap("older");
    first.resolve(olderBitmap);
    await older;

    expect(controller.bitmap).toBe(newerBitmap);
    expect(olderBitmap.close).toHaveBeenCalledOnce();
    expect(painted.filter(Boolean)).toEqual([newerBitmap]);
  });

  it("clears retained and pending old-run bitmaps when the run changes", async () => {
    const pending = deferred<FakeBitmap>();
    const firstBitmap = bitmap("first");
    const oldPending = bitmap("pending-old");
    const painted: Array<FakeBitmap | null> = [];
    const decode = vi
      .fn<(png: ArrayBuffer) => Promise<FakeBitmap>>()
      .mockResolvedValueOnce(firstBitmap)
      .mockReturnValueOnce(pending.promise);
    const controller = new LiveFrameController<FakeBitmap>(decode, (value) => painted.push(value));

    await controller.receive(frame(20n, 1), "run-a");
    const oldDecode = controller.receive(frame(21n, 2), "run-a");
    controller.setRun("run-b");

    expect(controller.bitmap).toBeNull();
    expect(firstBitmap.close).toHaveBeenCalledOnce();
    expect(painted.at(-1)).toBeNull();
    pending.resolve(oldPending);
    await oldDecode;
    expect(oldPending.close).toHaveBeenCalledOnce();

    const newBitmap = bitmap("new");
    decode.mockResolvedValueOnce(newBitmap);
    await controller.receive(frame(1n, 3), "run-b");
    expect(controller.bitmap).toBe(newBitmap);
  });

  it("rejects prefix-only messages and keeps newest-received semantics after decode failure", async () => {
    const current = bitmap("current");
    const decode = vi
      .fn<(png: ArrayBuffer) => Promise<FakeBitmap>>()
      .mockResolvedValueOnce(current)
      .mockRejectedValueOnce(new Error("bad png"));
    const controller = new LiveFrameController<FakeBitmap>(decode, () => undefined);

    await controller.receive(new ArrayBuffer(8), "run-a");
    await controller.receive(frame(1n, 1), "run-a");
    await controller.receive(frame(2n, 2), "run-a");
    await controller.receive(frame(2n, 3), "run-a");

    expect(decode).toHaveBeenCalledTimes(2);
    expect(controller.bitmap).toBe(current);
  });
});

function frame(counter: bigint, byte: number): ArrayBuffer {
  const buffer = new ArrayBuffer(9);
  new DataView(buffer).setBigUint64(0, counter, true);
  new Uint8Array(buffer)[8] = byte;
  return buffer;
}

function bitmap(name: string): FakeBitmap {
  return { name, width: 256, height: 224, close: vi.fn() };
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}
