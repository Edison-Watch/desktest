import type { Meta, StoryObj } from "@storybook/react";
import DesktestQAAnimation from "./DesktestQAAnimation";

const meta: Meta<typeof DesktestQAAnimation> = {
  title: "Animations/DesktestQA",
  component: DesktestQAAnimation,
  parameters: {
    layout: "fullscreen",
  },
};

export default meta;
type Story = StoryObj<typeof DesktestQAAnimation>;

export const Default: Story = {};
