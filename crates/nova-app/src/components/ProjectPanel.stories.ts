import type { Meta, StoryObj } from "@storybook/vue3-vite";
import ProjectPanel from "./ProjectPanel.vue";
import type { NovaProject } from "../types/nova";

const SAMPLE_PROJECT: NovaProject = {
  root: "/home/chris/projects/nova-fixtures",
  manifest: {
    version: 1,
    project: { name: "nova-fixtures" },
    defaults: { environment: "local", timeout: "30s" },
    collections: { path: "collections" },
    environments: { path: "envs" },
  },
  environments: [
    { name: "local", variables: {}, secrets: [], auth: null, path: "envs/local.yaml" },
    { name: "staging", variables: {}, secrets: [], auth: null, path: "envs/staging.yaml" },
  ],
  environments_dir: "/home/chris/projects/nova-fixtures/nova/envs",
  collections: { name: "collections", path: "/home/chris/projects/nova-fixtures/nova/collections", children: [], requests: [] },
};

const meta: Meta<typeof ProjectPanel> = {
  title: "Components/ProjectPanel",
  component: ProjectPanel,
  args: {
    project: SAMPLE_PROJECT,
  },
  argTypes: {
    onDirtyChange: { action: "dirtyChange" },
    onSaved: { action: "saved" },
  },
};

export default meta;
type Story = StoryObj<typeof ProjectPanel>;

export const NoIssues: Story = {
  args: { validationIssues: [] },
};

export const WithValidationIssues: Story = {
  args: {
    validationIssues: [
      "collections/users/create.nova: header value looks like a hardcoded API key",
      "envs/staging.yaml: references undefined variable {{legacy_token}}",
    ],
  },
};
