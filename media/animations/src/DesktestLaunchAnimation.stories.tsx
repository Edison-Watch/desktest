import type { Meta, StoryObj } from "@storybook/react";
import DesktestLaunchAnimation from "./DesktestLaunchAnimation";

const meta: Meta<typeof DesktestLaunchAnimation> = {
  title: "Animations/DesktestLaunch",
  component: DesktestLaunchAnimation,
  parameters: {
    layout: "fullscreen",
  },
};

export default meta;
type Story = StoryObj<typeof DesktestLaunchAnimation>;

export const Default: Story = {};
