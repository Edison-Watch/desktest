import type { Meta, StoryObj } from "@storybook/react";
import DesktestClosingAnimation from "./DesktestClosingAnimation";

const meta: Meta<typeof DesktestClosingAnimation> = {
  title: "Animations/DesktestClosing",
  component: DesktestClosingAnimation,
  parameters: {
    layout: "fullscreen",
  },
};

export default meta;
type Story = StoryObj<typeof DesktestClosingAnimation>;

export const Default: Story = {};
