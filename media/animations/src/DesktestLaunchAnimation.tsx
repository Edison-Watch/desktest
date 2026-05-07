import React, { useState, useEffect, useRef } from "react";
import DesktestTitleAnimation from "./DesktestTitleAnimation";
import DesktestRunAnimation from "./DesktestRunAnimation";
import DesktestLoopAnimation from "./DesktestLoopAnimation";
import DesktestDebugAnimation from "./DesktestDebugAnimation";
import DesktestOrchestrationAnimation from "./DesktestOrchestrationAnimation";
import DesktestCodifyAnimation from "./DesktestCodifyAnimation";
import DesktestQAAnimation from "./DesktestQAAnimation";
import DesktestClosingAnimation from "./DesktestClosingAnimation";

const SCENES: { component: React.FC; duration: number }[] = [
  { component: DesktestTitleAnimation, duration: 3500 },
  { component: DesktestRunAnimation, duration: 3500 },
  { component: DesktestLoopAnimation, duration: 11000 },
  { component: DesktestDebugAnimation, duration: 13000 },
  { component: DesktestCodifyAnimation, duration: 18000 },
  { component: DesktestQAAnimation, duration: 11500 },
  { component: DesktestOrchestrationAnimation, duration: 11000 },
  { component: DesktestClosingAnimation, duration: 8000 },
];

const FADE_MS = 800;

type Layer = { index: number; key: number; exiting: boolean };

export default function DesktestLaunchAnimation() {
  const [layers, setLayers] = useState<Layer[]>([
    { index: 0, key: 0, exiting: false },
  ]);
  const keyRef = useRef(1);

  const active = layers.find((l) => !l.exiting)!;

  useEffect(() => {
    const dur = SCENES[active.index].duration;

    if (active.index === SCENES.length - 1) return;

    const timer = setTimeout(() => {
      const nextIndex = active.index + 1;
      const newKey = keyRef.current++;

      setLayers((prev) => [
        ...prev.map((l) => ({ ...l, exiting: true })),
        { index: nextIndex, key: newKey, exiting: false },
      ]);

      setTimeout(() => {
        setLayers((prev) => prev.filter((l) => !l.exiting));
      }, FADE_MS);
    }, dur - FADE_MS);

    return () => clearTimeout(timer);
  }, [active.index, active.key]);

  return (
    <div className="dseq-root">
      <style>{`
        .dseq-root {
          position: relative;
          width: 100%;
          aspect-ratio: 16 / 9;
          background: #000;
          overflow: hidden;
        }
        .dseq-layer {
          position: absolute;
          inset: 0;
        }
        .dseq-enter {
          z-index: 2;
          animation: dseq-in ${FADE_MS}ms ease-in-out forwards;
        }
        .dseq-exit {
          z-index: 1;
          animation: dseq-out ${FADE_MS}ms ease-in-out forwards;
        }
        @keyframes dseq-in {
          from { opacity: 0; }
          to   { opacity: 1; }
        }
        @keyframes dseq-out {
          from { opacity: 1; }
          to   { opacity: 0; }
        }
      `}</style>
      {layers.map((layer) => {
        const Scene = SCENES[layer.index].component;
        return (
          <div
            key={layer.key}
            className={`dseq-layer ${layer.exiting ? "dseq-exit" : "dseq-enter"}`}
          >
            <Scene />
          </div>
        );
      })}
    </div>
  );
}
