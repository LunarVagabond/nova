import type { Meta, StoryObj } from "@storybook/vue3-vite";
import Sidebar from "./Sidebar.vue";
import type { GitStatusMap, NovaProject } from "../types/nova";

const SAMPLE_PROJECT: NovaProject = {
  root: "/home/chris/projects/nova-fixtures",
  manifest: {
    version: 1,
    project: { name: "nova-fixtures" },
    defaults: { environment: "local", timeout: null },
    collections: { path: "collections" },
    environments: { path: "envs" },
  },
  environments: [
    { name: "local", variables: { base_url: "http://localhost:3000" }, secrets: [], auth: null, path: "envs/local.yaml" },
    { name: "staging", variables: { base_url: "https://staging.example.com" }, secrets: [], auth: null, path: "envs/staging.yaml" },
  ],
  environments_dir: "/home/chris/projects/nova-fixtures/nova/envs",
  collections: {
    name: "collections",
    path: "/home/chris/projects/nova-fixtures/nova/collections",
    children: [
      {
        name: "auth",
        path: "/home/chris/projects/nova-fixtures/nova/collections/auth",
        children: [],
        requests: [
          { name: "login", path: "/home/chris/projects/nova-fixtures/nova/collections/auth/login.nova", method: "POST", protocol: "http" },
        ],
      },
      {
        name: "users",
        path: "/home/chris/projects/nova-fixtures/nova/collections/users",
        children: [],
        requests: [
          { name: "create", path: "/home/chris/projects/nova-fixtures/nova/collections/users/create.nova", method: "POST", protocol: "http" },
          { name: "get", path: "/home/chris/projects/nova-fixtures/nova/collections/users/get.nova", method: "GET", protocol: "http" },
        ],
      },
    ],
    requests: [],
  },
};

const GIT_STATUS: GitStatusMap = {
  "/home/chris/projects/nova-fixtures/nova/collections/users/create.nova": "unstaged",
};

const meta: Meta<typeof Sidebar> = {
  title: "Components/Sidebar",
  component: Sidebar,
  args: {
    project: SAMPLE_PROJECT,
    selectedEnvironment: "local",
  },
  argTypes: {
    selectRequest: { action: "selectRequest" },
    createRequest: { action: "createRequest" },
    createCollection: { action: "createCollection" },
    renameCollection: { action: "renameCollection" },
    deleteCollection: { action: "deleteCollection" },
    renameRequest: { action: "renameRequest" },
    duplicateRequest: { action: "duplicateRequest" },
    deleteRequest: { action: "deleteRequest" },
    createEnvironment: { action: "createEnvironment" },
    manageEnvironment: { action: "manageEnvironment" },
  },
};

export default meta;
type Story = StoryObj<typeof Sidebar>;

export const Default: Story = {
  args: { selectedRequestPath: "/home/chris/projects/nova-fixtures/nova/collections/users/get.nova" },
};

export const WithGitStatus: Story = {
  args: { gitStatus: GIT_STATUS },
};

export const NoEnvironments: Story = {
  args: {
    project: { ...SAMPLE_PROJECT, environments: [] },
    selectedEnvironment: null,
  },
};
