import type { Preview } from '@storybook/vue3-vite'
import '../src/styles/main.scss'
import { registerTauriMocks, type TauriMockMap } from './mocks/tauri-ipc'

const preview: Preview = {
  decorators: [
    (story, context) => {
      // Re-registered before every story (not just ones that declare
      // `tauriMocks`) so a component's `invoke()` call always fails with
      // our own clear "no mock registered" error instead of Tauri's
      // generic "not running in Tauri" one. `parameters` has an untyped
      // index signature, so `tauriMocks` is set from a story via
      // `parameters: { tauriMocks: {...} as TauriMockMap }` — see
      // ./mocks/tauri-ipc.ts for the shape.
      registerTauriMocks((context.parameters.tauriMocks as TauriMockMap | undefined) ?? {})
      return story()
    },
  ],
  parameters: {
    controls: {
      matchers: {
       color: /(background|color)$/i,
       date: /Date$/i,
      },
    },

    a11y: {
      // 'todo' - show a11y violations in the test UI only
      // 'error' - fail CI on a11y violations
      // 'off' - skip a11y checks entirely
      test: 'todo'
    }
  },
};

export default preview;