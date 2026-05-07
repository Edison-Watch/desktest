import type { Meta, StoryObj } from "@storybook/react";
import DesktestOrchestrationAnimation from "./DesktestOrchestrationAnimation";

const meta: Meta<typeof DesktestOrchestrationAnimation> = {
  title: "Animations/DesktestOrchestration",
  component: DesktestOrchestrationAnimation,
  parameters: {
    layout: "fullscreen",
  },
};

export default meta;
type Story = StoryObj<typeof DesktestOrchestrationAnimation>;

export const Default: Story = {};
