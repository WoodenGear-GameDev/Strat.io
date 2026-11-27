export interface Transport {
  connect(url: string): void;
  send(data: Uint8Array): void;
  onMessage(handler: (data: Uint8Array) => void): void;
  onOpen(handler: () => void): void;
  onClose(handler: (ev: CloseEvent) => void): void;
  close(): void;
}
