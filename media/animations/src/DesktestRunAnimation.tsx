import React, { useState, useEffect } from "react";

const COMMAND = "desktest run task.json";
const CHAR_COUNT = COMMAND.length; // 22
const CYCLE_MS = 5000;

export default function DesktestRunAnimation() {
  const [cycle, setCycle] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setCycle((c) => c + 1), CYCLE_MS);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="dr-scene" key={cycle}>
      <style>{`
        .dr-scene {
          position: relative;
          width: 100%;
          aspect-ratio: 16 / 9;
          background: #000;
          display: flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          gap: 24px;
          overflow: hidden;
          font-family: 'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace;
          padding: 40px;
        }

        .dr-terminal {
          width: 80%;
          max-width: 1200px;
          border-radius: 12px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow:
            0 20px 60px rgba(0, 0, 0, 0.8),
            0 8px 24px rgba(0, 0, 0, 0.5);
          opacity: 0;
          animation:
            dr-fade-in 300ms ease-out 1400ms forwards,
            dr-terminal-exit 1000ms ease-in-out 3500ms forwards;
        }

        .dr-titlebar {
          background: #2F3440;
          border-bottom: 1px solid #383838;
          padding: 11px 14px;
          display: flex;
          gap: 7px;
          align-items: center;
        }

        .dr-dot {
          width: 12px;
          height: 12px;
          border-radius: 50%;
        }

        .dr-body {
          background: #1C1C1C;
          padding: 18px 22px;
        }

        .dr-prompt-line {
          display: flex;
          align-items: center;
          font-size: 24px;
          line-height: 1.5;
        }

        .dr-prompt {
          color: #C3FFFD;
          font-weight: 700;
        }

        .dr-typed {
          color: #F9F9F9;
          overflow: hidden;
          white-space: nowrap;
          display: inline-block;
          width: 0;
          animation:
            dr-type 726ms steps(${CHAR_COUNT}) 1500ms forwards,
            dr-cmd-highlight 500ms ease-in-out 2800ms forwards;
          font-family: inherit;
        }

        .dr-cursor {
          display: inline-block;
          width: 9px;
          height: 19px;
          background: #C3FFFD;
          margin-left: 1px;
          vertical-align: text-bottom;
          opacity: 0;
          animation:
            dr-cursor-show 70ms 1500ms forwards,
            dr-cursor-hide 50ms ease-out 2700ms forwards;
        }

        .dr-output {
          font-size: 22px;
          line-height: 1.6;
          margin-top: 8px;
          opacity: 0;
          animation: dr-fade-in 200ms ease-out 3000ms forwards;
        }

        .dr-output-arrow {
          color: #C3FFFD;
        }

        .dr-output-text {
          color: #9BA4A6;
        }

        .dr-json-card {
          width: 80%;
          max-width: 1200px;
          border-radius: 12px;
          border: 1px solid #D0D0D0;
          overflow: hidden;
          box-shadow:
            0 20px 60px rgba(0, 0, 0, 0.5),
            0 8px 24px rgba(0, 0, 0, 0.3);
          opacity: 0;
          animation:
            dr-fade-in 400ms ease-out 200ms forwards,
            dr-json-exit 600ms ease-in 3500ms forwards;
        }

        .dr-json-titlebar {
          background: #E8E8E8;
          border-bottom: 1px solid #D0D0D0;
          padding: 10px 14px;
          display: flex;
          align-items: center;
          gap: 8px;
        }

        .dr-json-filename {
          color: #555;
          font-size: 12px;
          margin-left: 8px;
        }

        .dr-json-body {
          background: #F5F5F5;
          padding: 20px 24px;
          font-size: 22px;
          line-height: 1.7;
        }

        .dr-json-brace {
          color: #333;
        }

        .dr-json-key {
          color: #999;
          opacity: 0.4;
        }

        .dr-json-value {
          color: #999;
          opacity: 0.4;
        }

        .dr-json-key-highlight {
          color: #0969da;
          font-weight: 700;
        }

        .dr-json-value-highlight {
          color: #1a1a1a;
        }

        .dr-json-dim {
          color: #999;
          opacity: 0.35;
        }

        .dr-json-comma {
          color: #999;
          opacity: 0.4;
        }

        .dr-json-line {
          display: block;
        }

        .dr-json-indent {
          display: inline-block;
          width: 2ch;
        }

        @keyframes dr-fade-in {
          from { opacity: 0; }
          to { opacity: 1; }
        }

        @keyframes dr-type {
          from { width: 0; }
          to { width: ${CHAR_COUNT}ch; }
        }

        @keyframes dr-cursor-show {
          from { opacity: 0; }
          to { opacity: 1; }
        }

        @keyframes dr-cursor-hide {
          from { opacity: 1; }
          to { opacity: 0; }
        }

        @keyframes dr-cmd-highlight {
          0%   { color: #F9F9F9; text-shadow: none; }
          50%  { color: #C3FFFD; text-shadow: 0 0 12px rgba(195, 255, 253, 0.8), 0 0 24px rgba(195, 255, 253, 0.3); }
          100% { color: #C3FFFD; text-shadow: 0 0 8px rgba(195, 255, 253, 0.5), 0 0 16px rgba(195, 255, 253, 0.2); }
        }

        @keyframes dr-terminal-exit {
          0%   { transform: none; opacity: 1; }
          100% { transform: none; opacity: 0; }
        }

        @keyframes dr-json-exit {
          0%   { transform: none; opacity: 1; }
          100% { transform: translateY(40px) scale(0.95); opacity: 0; }
        }

        .dr-title {
          position: absolute;
          top: 80px;
          left: 50%;
          transform: translateX(-50%);
          font-size: 64px;
          color: #F9F9F9;
          font-weight: 700;
          white-space: nowrap;
          opacity: 0;
          animation: dr-fade-in 500ms ease-out 100ms forwards;
          z-index: 10;
        }

        .dr-title-accent { color: #C3FFFD; }

        .dr-tagline {
          position: absolute;
          bottom: 16px;
          left: 50%;
          transform: translateX(-50%);
          font-size: 14px;
          color: #9BA4A6;
          opacity: 0;
          animation: dr-fade-in 500ms ease-out 4000ms forwards;
          white-space: nowrap;
          z-index: 10;
        }

        .dr-tagline-em { color: #C3FFFD; font-weight: 700; }

        @media (prefers-reduced-motion: reduce) {
          .dr-terminal   { animation: none; opacity: 1; }
          .dr-typed      { animation: none; width: ${CHAR_COUNT}ch; }
          .dr-cursor     { animation: none; opacity: 1; }
          .dr-json-card  { animation: none; opacity: 1; }
          .dr-output     { animation: none; opacity: 1; }
          .dr-title, .dr-tagline { animation: none; opacity: 1; }
        }
      `}</style>

      <div className="dr-title">
        <span className="dr-title-accent">Natural language</span> desktop testing
      </div>

      <div className="dr-terminal">
        <div className="dr-titlebar">
          <div className="dr-dot" style={{ background: "#FF3B4D" }} />
          <div className="dr-dot" style={{ background: "#E3B341" }} />
          <div className="dr-dot" style={{ background: "#00C781" }} />
        </div>
        <div className="dr-body">
          <div className="dr-prompt-line">
            <span className="dr-prompt">$&nbsp;</span>
            <span className="dr-typed">{COMMAND}</span>
            <span className="dr-cursor" />
          </div>
          <div className="dr-output">
            <span className="dr-output-arrow">{"▸ "}</span>
            <span className="dr-output-text">Starting agent loop...</span>
          </div>
        </div>
      </div>

      <div className="dr-json-card">
        <div className="dr-json-titlebar">
          <div className="dr-dot" style={{ background: "#FF3B4D" }} />
          <div className="dr-dot" style={{ background: "#E3B341" }} />
          <div className="dr-dot" style={{ background: "#00C781" }} />
          <span className="dr-json-filename">task.json</span>
        </div>
        <div className="dr-json-body">
          <span className="dr-json-line">
            <span className="dr-json-brace">{"{"}</span>
          </span>
          <span className="dr-json-line">
            <span className="dr-json-indent" />
            <span className="dr-json-key-highlight">{'"instruction"'}</span>
            <span className="dr-json-key-highlight">: </span>
            <span className="dr-json-value-highlight">
              {'"Add a todo item called \'Buy groceries\' and verify it appears in the list"'}
            </span>
            <span className="dr-json-comma">,</span>
          </span>
          <span className="dr-json-line">
            <span className="dr-json-indent" />
            <span className="dr-json-dim" style={{ opacity: 0.6, color: '#333' }}>...</span>
          </span>
          <span className="dr-json-line">
            <span className="dr-json-brace">{"}"}</span>
          </span>
        </div>
      </div>

      <div className="dr-tagline">
        Describe the test in plain English — <span className="dr-tagline-em">desktest handles the rest</span>
      </div>
    </div>
  );
}
