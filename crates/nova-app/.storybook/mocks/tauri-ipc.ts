// Mocks the Tauri IPC transport itself (via `@tauri-apps/api/mocks`), not
// `src/api/nova.ts`. Every component keeps calling the real `invoke()`
// wrapper unmodified — the response is just intercepted before it would
// otherwise hit a (nonexistent, in Storybook) Rust backend.
//
// Stories opt in per-command via `parameters.tauriMocks` on the story or
// its meta:
//
//   export const WithHistory: Story = {
//     parameters: {
//       tauriMocks: {
//         get_history: () => historyFixture,
//       },
//     },
//   };
//
// A handler receives the command's raw payload object and can return a
// value or a Promise. Reject a command by throwing inside the handler.
//
// A command invoked with no handler registered throws immediately (loudly,
// in the browser console) rather than hanging forever — hanging is the
// default `mockIPC` behavior for unrecognized commands and it's a
// miserable thing to debug from a blank UI.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";

export type TauriCommandHandler = (payload: Record<string, unknown>) => unknown;
export type TauriMockMap = Record<string, TauriCommandHandler>;

export function registerTauriMocks(handlers: TauriMockMap): void {
  clearMocks();
  mockIPC((cmd, payload) => {
    const handler = handlers[cmd];
    if (!handler) {
      throw new Error(
        `[storybook] no mock registered for Tauri command "${cmd}" — add one to this story's ` +
          `\`parameters.tauriMocks\` (see .storybook/mocks/tauri-ipc.ts).`,
      );
    }
    return handler((payload ?? {}) as Record<string, unknown>);
  });
}

export function resetTauriMocks(): void {
  clearMocks();
}
