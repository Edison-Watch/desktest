import type { Meta, StoryObj } from "@storybook/react";
import DesktestLoopAnimation from "./DesktestLoopAnimation";

const meta: Meta<typeof DesktestLoopAnimation> = {
  title: "Animations/DesktestLoop",
  component: DesktestLoopAnimation,
  parameters: {
    layout: "fullscreen",
  },
};

export default meta;
type Story = StoryObj<typeof DesktestLoopAnimation>;

export const Default: Story = {};
