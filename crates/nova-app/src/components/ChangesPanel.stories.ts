import type { Meta, StoryObj } from "@storybook/vue3-vite";
import ChangesPanel from "./ChangesPanel.vue";
import type { GitStatusMap } from "../types/nova";

const SAMPLE_STATUS: GitStatusMap = {
  "/home/chris/projects/nova-fixtures/nova/collections/users/create.nova": "unstaged",
  "/home/chris/projects/nova-fixtures/nova/collections/users/get.nova": "staged",
  "/home/chris/projects/nova-fixtures/nova/collections/auth/login.nova": "untracked",
};

const SAMPLE_DIFF = `--- a/nova/collections/users/get.nova
+++ b/nova/collections/users/get.nova
@@ -1,4 +1,5 @@
 [request]
 method: GET
-url: {{base_url}}/users
+url: {{base_url}}/users/{{user_id}}
+protocol: http
`;

const meta: Meta<typeof ChangesPanel> = {
  title: "Components/ChangesPanel",
  component: ChangesPanel,
  args: {
    projectRoot: "/home/chris/projects/nova-fixtures",
    active: true,
  },
  argTypes: {
    onChanged: { action: "changed" },
  },
};

export default meta;
type Story = StoryObj<typeof ChangesPanel>;

export const WithChanges: Story = {
  parameters: {
    tauriMocks: {
      git_status: () => SAMPLE_STATUS,
      git_diff_file: () => SAMPLE_DIFF,
      git_stage_files: () => undefined,
      git_unstage_files: () => undefined,
    },
  },
};

export const CleanWorkingTree: Story = {
  parameters: {
    tauriMocks: {
      git_status: () => ({}),
    },
  },
};

export const NotAGitRepository: Story = {
  parameters: {
    tauriMocks: {
      git_status: () => null,
    },
  },
};
