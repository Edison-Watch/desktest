import type { Meta, StoryObj } from "@storybook/react";
import DesktestTitleAnimation from "./DesktestTitleAnimation";

const meta: Meta<typeof DesktestTitleAnimation> = {
  title: "Animations/DesktestTitle",
  component: DesktestTitleAnimation,
  parameters: {
    layout: "fullscreen",
  },
};

export default meta;
type Story = StoryObj<typeof DesktestTitleAnimation>;

export const Default: Story = {};

export const Contained: Story = {
  decorators: [
    (Story) => (
      <div style={{ maxWidth: 1200, margin: "40px auto" }}>
        <Story />
      </div>
    ),
  ],
};
