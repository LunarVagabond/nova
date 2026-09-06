import type { Meta, StoryObj } from "@storybook/vue3-vite";
import Icon from "./Icon.vue";

const ALL_NAMES: string[] = [
  "chevron-down", "folder", "folder-open", "folder-plus", "file-plus", "pencil", "trash",
  "plus", "x", "settings", "check", "swap", "wand", "copy", "play", "history", "transfer",
  "server", "sun", "moon", "monitor", "sidebar", "cookie", "eye", "eye-off", "lock",
  "git-branch", "upload", "download", "sparkle", "shield", "users", "code", "help-circle",
];

const meta: Meta<typeof Icon> = {
  title: "Components/Icon",
  component: Icon,
};

export default meta;
type Story = StoryObj<typeof Icon>;

export const Single: Story = {
  args: { name: "sparkle" },
};

export const AllIcons: Story = {
  render: () => ({
    components: { Icon },
    setup() {
      return { names: ALL_NAMES };
    },
    template: `
      <div style="display: grid; grid-template-columns: repeat(6, 1fr); gap: 1rem; color: var(--color-text);">
        <div v-for="name in names" :key="name" style="display: flex; flex-direction: column; align-items: center; gap: 0.35rem; font-size: 0.7rem;">
          <Icon :name="name" style="width: 20px; height: 20px;" />
          <span>{{ name }}</span>
        </div>
      </div>
    `,
  }),
};
