import type { Meta, StoryObj } from "@storybook/react";
import DesktestCodifyAnimation from "./DesktestCodifyAnimation";

const meta: Meta<typeof DesktestCodifyAnimation> = {
  title: "Animations/DesktestCodify",
  component: DesktestCodifyAnimation,
  parameters: {
    layout: "fullscreen",
  },
};

export default meta;
type Story = StoryObj<typeof DesktestCodifyAnimation>;

export const Default: Story = {};
