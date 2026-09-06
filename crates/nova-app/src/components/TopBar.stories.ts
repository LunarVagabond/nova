import type { Meta, StoryObj } from "@storybook/vue3-vite";
import TopBar from "./TopBar.vue";

const meta: Meta<typeof TopBar> = {
  title: "Components/TopBar",
  component: TopBar,
  args: {
    environments: [{ name: "local" }, { name: "staging" }],
    selectedEnvironment: "local",
    showingProjectSettings: false,
    showingHistory: false,
    showingCookies: false,
    showingMockLog: false,
    showingChanges: false,
    runningTests: false,
    mockServerBusy: false,
    sidebarHidden: false,
    themePreference: "system",
    mockServerStatus: { running: false, host: null, port: null },
  },
  argTypes: {
    "onUpdate:selectedEnvironment": { action: "update:selectedEnvironment" },
    onSwitchProject: { action: "switchProject" },
    onProjectSettings: { action: "projectSettings" },
    onShowHistory: { action: "showHistory" },
    onShowCookies: { action: "showCookies" },
    onShowMockLog: { action: "showMockLog" },
    onShowChanges: { action: "showChanges" },
    onRunTests: { action: "runTests" },
    onImportExport: { action: "importExport" },
    onToggleMockServer: { action: "toggleMockServer" },
    onToggleSidebar: { action: "toggleSidebar" },
    onCycleTheme: { action: "cycleTheme" },
  },
};

export default meta;
type Story = StoryObj<typeof TopBar>;

export const NoProjectOpen: Story = {
  args: { projectName: null, environments: [] },
};

export const ProjectOpen: Story = {
  args: { projectName: "nova-fixtures" },
};

export const MockServerRunning: Story = {
  args: {
    projectName: "nova-fixtures",
    mockServerStatus: { running: true, host: "127.0.0.1", port: 4010 },
  },
};

export const TestsRunning: Story = {
  args: {
    projectName: "nova-fixtures",
    runningTests: true,
  },
};
