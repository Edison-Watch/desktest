import type { Meta, StoryObj } from "@storybook/react";
import DesktestRunAnimation from "./DesktestRunAnimation";

const meta: Meta<typeof DesktestRunAnimation> = {
  title: "Animations/DesktestRun",
  component: DesktestRunAnimation,
  parameters: {
    layout: "fullscreen",
  },
};

export default meta;
type Story = StoryObj<typeof DesktestRunAnimation>;

export const Default: Story = {};
