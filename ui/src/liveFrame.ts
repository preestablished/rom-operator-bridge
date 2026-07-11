export type LiveFrameBitmap = Pick<ImageBitmap, "width" | "height" | "close">;

export type LiveFrameDecoder<T extends LiveFrameBitmap = ImageBitmap> = (
  pngBytes: ArrayBuffer
) => Promise<T>;

export class LiveFrameController<T extends LiveFrameBitmap = ImageBitmap> {
  private runId: string | null = null;
  private highestReceived: bigint | null = null;
  private retained: T | null = null;

  constructor(
    private readonly decode: LiveFrameDecoder<T>,
    private readonly onBitmap: (bitmap: T | null) => void
  ) {}

  get bitmap(): T | null {
    return this.retained;
  }

  setRun(runId: string | null): void {
    if (runId === this.runId) {
      return;
    }
    this.runId = runId;
    this.highestReceived = null;
    this.replace(null);
  }

  clear(): void {
    this.runId = null;
    this.highestReceived = null;
    this.replace(null);
  }

  async receive(buffer: ArrayBuffer, runId: string | null): Promise<void> {
    this.setRun(runId);
    if (buffer.byteLength < 9) {
      return;
    }

    const frameCounter = new DataView(buffer).getBigUint64(0, true);
    if (this.highestReceived !== null && frameCounter <= this.highestReceived) {
      return;
    }

    // Newest-received semantics: a same-counter retransmission stays ignored
    // even if decoding fails. A later counter can still replace the bitmap.
    this.highestReceived = frameCounter;
    const decodeRunId = this.runId;
    let bitmap: T;
    try {
      bitmap = await this.decode(buffer.slice(8));
    } catch {
      return;
    }

    if (decodeRunId !== this.runId || frameCounter !== this.highestReceived) {
      bitmap.close();
      return;
    }
    this.replace(bitmap);
  }

  private replace(bitmap: T | null): void {
    if (this.retained === bitmap) {
      return;
    }
    this.retained?.close();
    this.retained = bitmap;
    this.onBitmap(bitmap);
  }
}
