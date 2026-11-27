import type { Transport } from "./transport.ts";

export class WsTransport implements Transport {
  private ws: WebSocket | null = null;
  private messageHandler: ((data: Uint8Array) => void) | null = null;
  private openHandler: (() => void) | null = null;
  private closeHandler: ((ev: CloseEvent) => void) | null = null;

  connect(url: string): void {
    this.close();
    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";

    ws.onopen = () => {
      this.openHandler?.();
    };

    ws.onmessage = (ev: MessageEvent) => {
      if (ev.data instanceof ArrayBuffer) {
        this.messageHandler?.(new Uint8Array(ev.data));
      }
    };

    ws.onclose = (ev: CloseEvent) => {
      this.closeHandler?.(ev);
    };

    this.ws = ws;
  }

  send(data: Uint8Array): void {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(data);
    }
  }

  onMessage(handler: (data: Uint8Array) => void): void {
    this.messageHandler = handler;
  }

  onOpen(handler: () => void): void {
    this.openHandler = handler;
  }

  onClose(handler: (ev: CloseEvent) => void): void {
    this.closeHandler = handler;
  }

  close(): void {
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }
}
