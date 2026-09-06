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
  // The real app supplies height via its own flex layout (`.history-panel`
  // is `height: 100%`, which needs a sized ancestor) — without a fixed
  // height here it collapses to nothing, which reads as "cramped" but is
  // a story-wrapper issue, not a component bug.
  decorators: [(story) => ({ components: { story }, template: '<div style="height: 32rem;"><story /></div>' })],
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
