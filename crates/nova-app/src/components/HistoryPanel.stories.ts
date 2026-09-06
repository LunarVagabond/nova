import type { Meta, StoryObj } from "@storybook/vue3-vite";
import HistoryPanel from "./HistoryPanel.vue";
import { sampleHistorySummaries } from "../../.storybook/mocks/fixtures";

const meta: Meta<typeof HistoryPanel> = {
  title: "Components/HistoryPanel",
  component: HistoryPanel,
  args: {
    projectRoot: "/home/chris/projects/nova-fixtures",
    active: true,
  },
};

export default meta;
type Story = StoryObj<typeof HistoryPanel>;

export const ListOnly: Story = {
  parameters: {
    tauriMocks: {
      get_history: () => sampleHistorySummaries,
    },
  },
};

export const Empty: Story = {
  parameters: {
    tauriMocks: {
      get_history: () => [],
    },
  },
};
