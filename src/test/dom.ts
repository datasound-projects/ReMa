/**
 * Browser APIs that jsdom lacks, for component tests
 * (`// @vitest-environment jsdom`).
 */
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

afterEach(() => cleanup());

const proto = HTMLDialogElement.prototype as HTMLDialogElement & { showModal?: () => void };
if (typeof proto.showModal !== 'function') {
  proto.showModal = function showModal(this: HTMLDialogElement) {
    this.setAttribute('open', '');
  };
  proto.close = function close(this: HTMLDialogElement) {
    this.removeAttribute('open');
  };
}

if (!('ResizeObserver' in globalThis)) {
  class ResizeObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = ResizeObserverStub;
}

Element.prototype.scrollIntoView ??= function scrollIntoView() {};
