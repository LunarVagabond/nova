import type { Meta, StoryObj } from "@storybook/vue3-vite";
import CookiesPanel from "./CookiesPanel.vue";
import { sampleCookies } from "../../.storybook/mocks/fixtures";

const meta: Meta<typeof CookiesPanel> = {
  title: "Components/CookiesPanel",
  component: CookiesPanel,
  args: {
    projectRoot: "/home/chris/projects/nova-fixtures",
    active: true,
  },
};

export default meta;
type Story = StoryObj<typeof CookiesPanel>;

export const Populated: Story = {
  parameters: {
    tauriMocks: {
      get_cookies: () => sampleCookies,
    },
  },
};

export const Empty: Story = {
  parameters: {
    tauriMocks: {
      get_cookies: () => [],
    },
  },
};

export const LoadError: Story = {
  parameters: {
    tauriMocks: {
      get_cookies: () => {
        throw new Error("failed to read session cookie jar");
      },
    },
  },
};
