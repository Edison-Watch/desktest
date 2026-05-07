import type { Meta, StoryObj } from "@storybook/react";
import DesktestDebugAnimation from "./DesktestDebugAnimation";

const meta: Meta<typeof DesktestDebugAnimation> = {
  title: "Animations/DesktestDebug",
  component: DesktestDebugAnimation,
  parameters: {
    layout: "fullscreen",
  },
};

export default meta;
type Story = StoryObj<typeof DesktestDebugAnimation>;

export const Default: Story = {};
