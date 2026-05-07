import React, { useState, useEffect } from "react";

const CYCLE_MS = 11000;
const TYPED_TEXT = "Buy groceries";

export default function DesktestLoopAnimation() {
  const [cycle, setCycle] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setCycle((c) => c + 1), CYCLE_MS);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="dl-scene" key={cycle}>
      <style>{`
        .dl-scene {
          position: relative;
          width: 100%;
          aspect-ratio: 16 / 9;
          background: #000;
          overflow: hidden;
          font-family: 'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace;
        }

        /* --- Mini terminal carried from Run scene --- */
        .dl-mini-term {
          position: absolute;
          top: 50%;
          left: 50%;
          transform: translate(-50%, -50%);
          width: 42%;
          max-width: 560px;
          border-radius: 10px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow:
            0 16px 48px rgba(0, 0, 0, 0.7),
            0 6px 18px rgba(0, 0, 0, 0.4);
          z-index: 5;
          animation: dl-term-settle 1200ms cubic-bezier(0.16, 1, 0.3, 1) 400ms forwards;
        }

        .dl-mini-term-bar {
          background: #2F3440;
          border-bottom: 1px solid #383838;
          padding: 8px 12px;
          display: flex;
          gap: 6px;
          align-items: center;
        }

        .dl-mini-term-body {
          background: #1C1C1C;
          padding: 12px 16px;
        }

        .dl-mini-term-line {
          font-size: 28px;
          line-height: 1.6;
          display: flex;
          align-items: center;
        }

        .dl-mini-term-prompt {
          color: #C3FFFD;
          font-weight: 700;
        }

        .dl-mini-term-cmd {
          color: #C3FFFD;
          text-shadow: 0 0 8px rgba(195, 255, 253, 0.5);
        }

        .dl-mini-term-output {
          font-size: 26px;
          line-height: 1.6;
          margin-top: 4px;
        }

        .dl-mini-term-arrow {
          color: #C3FFFD;
        }

        .dl-mini-term-text {
          color: #9BA4A6;
        }

        @keyframes dl-term-settle {
          0%   { top: 50%; left: 50%; transform: translate(-50%, -50%); width: 42%; opacity: 1; border-radius: 10px; }
          100% { top: 12px; left: 20px; transform: translate(0, 0); width: 28%; opacity: 0.85; border-radius: 8px;
                 box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4); }
        }

        /* --- Desktop mockup (left side) --- */
        .dl-desktop {
          position: absolute;
          top: 55%;
          left: 6%;
          width: 40%;
          transform: translateY(-50%);
          border-radius: 14px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow:
            0 20px 60px rgba(0, 0, 0, 0.8),
            0 8px 24px rgba(0, 0, 0, 0.5);
          opacity: 0;
          animation: dl-fade-in 500ms ease-out 400ms forwards;
        }

        .dl-desktop-titlebar {
          background: #2F3440;
          border-bottom: 1px solid #383838;
          padding: 12px 16px;
          display: flex;
          gap: 7px;
          align-items: center;
        }

        .dl-dot {
          width: 11px;
          height: 11px;
          border-radius: 50%;
        }

        .dl-desktop-titlebar-label {
          color: #9BA4A6;
          font-size: 24px;
          margin-left: 10px;
        }

        .dl-desktop-body {
          background: #1C1C1C;
          padding: 24px;
          display: flex;
          flex-direction: column;
          gap: 14px;
          position: relative;
        }

        .dl-app-header {
          color: #F9F9F9;
          font-size: 28px;
          font-weight: 700;
          margin-bottom: 2px;
        }

        .dl-app-input {
          display: flex;
          gap: 10px;
          align-items: center;
          position: relative;
        }

        .dl-app-field {
          flex: 1;
          height: 44px;
          border-radius: 6px;
          border: 1px solid #383838;
          background: #2a2a2a;
          display: flex;
          align-items: center;
          padding: 0 14px;
          font-size: 24px;
          color: #9BA4A6;
          overflow: hidden;
          position: relative;
        }

        /* Input field focus state after click */
        .dl-app-field-focus {
          position: absolute;
          inset: 0;
          border-radius: 6px;
          border: 1px solid #C3FFFD;
          opacity: 0;
          pointer-events: none;
          animation: dl-fade-in-flat 100ms ease-out 4000ms forwards;
        }

        /* Typing animation: reveal text character by character */
        .dl-app-field-text {
          color: #F9F9F9;
          overflow: hidden;
          white-space: nowrap;
          display: inline-block;
          width: 0;
          animation: dl-type-text 780ms steps(${TYPED_TEXT.length}) 4200ms forwards;
        }

        /* Text cursor blink in input */
        .dl-input-cursor {
          display: inline-block;
          width: 1px;
          height: 14px;
          background: #F9F9F9;
          margin-left: 1px;
          vertical-align: middle;
          opacity: 0;
          animation:
            dl-fade-in-flat 50ms 4000ms forwards,
            dl-input-blink 600ms step-end 4000ms infinite,
            dl-fade-out 50ms 6800ms forwards;
        }

        /* Clear input text after Add is clicked */
        .dl-app-field-text-clear {
          animation: dl-type-text 780ms steps(${TYPED_TEXT.length}) 4200ms forwards,
                     dl-fade-out 100ms ease-out 6900ms forwards;
        }

        .dl-app-btn {
          height: 44px;
          padding: 0 18px;
          border-radius: 6px;
          background: #C3FFFD;
          color: #000;
          font-size: 24px;
          font-weight: 700;
          display: flex;
          align-items: center;
        }

        /* Button press effect */
        .dl-app-btn-press {
          animation: dl-btn-press 200ms ease-out 6800ms forwards;
        }

        .dl-app-list {
          display: flex;
          flex-direction: column;
          gap: 8px;
          margin-top: 6px;
        }

        .dl-app-item {
          height: 40px;
          border-radius: 6px;
          background: #2a2a2a;
          border: 1px solid #383838;
          display: flex;
          align-items: center;
          padding: 0 14px;
          font-size: 24px;
          color: #9BA4A6;
          opacity: 0.5;
        }

        .dl-app-item-new {
          opacity: 0;
          color: #F9F9F9;
          border-color: #C3FFFD;
          animation: dl-fade-in-flat 200ms ease-out 7000ms forwards;
        }

        /* --- Mouse cursor --- */
        .dl-cursor {
          position: absolute;
          width: 20px;
          height: 24px;
          z-index: 10;
          pointer-events: none;
          filter: drop-shadow(0 2px 4px rgba(0,0,0,0.9));
          opacity: 0;
          /* Start off-screen left, move to input, pause, move to button */
          animation:
            dl-fade-in-flat 150ms ease-out 3400ms forwards,
            dl-cursor-path 4500ms ease-in-out 3400ms forwards;
          /* Position controlled by dl-cursor-path */
          top: 47%;
          left: 10%;
        }

        @keyframes dl-cursor-path {
          0%   { top: 47%; left: 10%; }
          13%  { top: 51%; left: 20%; }
          14%  { top: 51.3%; left: 20%; }
          15%  { top: 51%; left: 20%; }
          55%  { top: 51%; left: 20%; }
          75%  { top: 51%; left: 39%; }
          76%  { top: 51.3%; left: 39%; }
          77%  { top: 51%; left: 39%; }
          100% { top: 51%; left: 39%; }
        }

        /* Click ripple on input field */
        .dl-click-1 {
          position: absolute;
          top: 51%;
          left: 20%;
          width: 20px;
          height: 20px;
          border-radius: 50%;
          border: 2px solid #C3FFFD;
          transform: translate(-50%, -50%) scale(0);
          opacity: 0;
          pointer-events: none;
          animation: dl-ripple 400ms ease-out 4000ms forwards;
        }

        /* Click ripple on Add button */
        .dl-click-2 {
          position: absolute;
          top: 51%;
          left: 39%;
          width: 20px;
          height: 20px;
          border-radius: 50%;
          border: 2px solid #C3FFFD;
          transform: translate(-50%, -50%) scale(0);
          opacity: 0;
          pointer-events: none;
          animation: dl-ripple 400ms ease-out 6800ms forwards;
        }

        /* Screenshot flash on desktop */
        .dl-flash {
          position: absolute;
          inset: 0;
          border-radius: 14px;
          border: 3px solid #C3FFFD;
          opacity: 0;
          pointer-events: none;
          animation: dl-flash 500ms ease-out 1200ms forwards;
        }

        /* --- LLM icon (right side) --- */
        .dl-llm {
          position: absolute;
          top: 55%;
          right: 10%;
          transform: translateY(-50%);
          display: flex;
          flex-direction: column;
          align-items: center;
          gap: 16px;
          opacity: 0;
          animation: dl-fade-in 500ms ease-out 600ms forwards;
        }

        .dl-llm-icon {
          width: 140px;
          height: 140px;
          animation: dl-llm-pulse 2.5s ease-in-out infinite 1200ms;
        }

        .dl-llm-label {
          color: #9BA4A6;
          font-size: 28px;
          font-weight: 700;
          letter-spacing: 0.5px;
        }

        /* --- Arrow SVG overlay --- */
        .dl-arrows-svg {
          position: absolute;
          inset: 0;
          width: 100%;
          height: 100%;
          pointer-events: none;
        }

        .dl-arrow-path {
          fill: none;
          stroke: #C3FFFD;
          stroke-width: 2.5;
          opacity: 0.7;
          stroke-dasharray: 8 4;
        }

        .dl-arrow-top-group {
          opacity: 0;
          animation: dl-fade-in-flat 500ms ease-out 1200ms forwards;
        }

        .dl-arrow-bottom-group {
          opacity: 0;
          animation: dl-fade-in-flat 500ms ease-out 3200ms forwards;
        }

        .dl-arrow-head {
          fill: #C3FFFD;
          opacity: 0.8;
        }

        .dl-arrow-label {
          font-family: 'JetBrains Mono', monospace;
          font-size: 22px;
          fill: #9BA4A6;
        }

        .dl-arrow-label-code {
          font-family: 'JetBrains Mono', monospace;
          font-size: 20px;
          fill: #C3FFFD;
        }

        /* --- Step counter --- */
        .dl-step {
          position: absolute;
          bottom: 36px;
          left: 27%;
          transform: translateX(-50%);
          display: flex;
          align-items: center;
          gap: 10px;
          opacity: 0;
          animation: dl-fade-in-center 400ms ease-out 7300ms forwards;
        }

        .dl-step-label {
          color: #9BA4A6;
          font-size: 26px;
        }

        .dl-step-num {
          color: #C3FFFD;
          font-size: 26px;
          font-weight: 700;
        }

        .dl-step-dots {
          display: flex;
          gap: 5px;
          margin-left: 6px;
        }

        .dl-step-dot {
          width: 10px;
          height: 10px;
          border-radius: 50%;
          background: #383838;
        }

        .dl-step-dot-active {
          background: #C3FFFD;
        }

        /* --- Keyframes --- */
        @keyframes dl-fade-in {
          from { opacity: 0; transform: translateY(-50%) scale(0.97); }
          to { opacity: 1; transform: translateY(-50%) scale(1); }
        }

        @keyframes dl-fade-in-flat {
          from { opacity: 0; }
          to { opacity: 1; }
        }

        @keyframes dl-fade-in-center {
          from { opacity: 0; transform: translateX(-50%) translateY(6px); }
          to { opacity: 1; transform: translateX(-50%) translateY(0); }
        }

        @keyframes dl-fade-out {
          from { opacity: 1; }
          to { opacity: 0; }
        }

        @keyframes dl-type-text {
          from { width: 0; }
          to { width: ${TYPED_TEXT.length}ch; }
        }

        @keyframes dl-input-blink {
          0%, 50% { opacity: 1; }
          50.001%, 100% { opacity: 0; }
        }

        @keyframes dl-btn-press {
          0% { transform: scale(1); }
          50% { transform: scale(0.93); }
          100% { transform: scale(1); }
        }

        @keyframes dl-ripple {
          0% { transform: translate(-50%, -50%) scale(0); opacity: 0.9; }
          100% { transform: translate(-50%, -50%) scale(2.5); opacity: 0; }
        }

        @keyframes dl-llm-pulse {
          0%, 100% { transform: scale(1); filter: drop-shadow(0 0 0px transparent); }
          50% { transform: scale(1.05); filter: drop-shadow(0 0 20px rgba(195, 255, 253, 0.25)); }
        }

        @keyframes dl-flash {
          0% { opacity: 0.9; }
          100% { opacity: 0; }
        }

        @media (prefers-reduced-motion: reduce) {
          .dl-desktop, .dl-llm, .dl-mini-term,
          .dl-arrow-top-group, .dl-arrow-bottom-group,
          .dl-step, .dl-app-field-text, .dl-app-item-new,
          .dl-cursor, .dl-click-1, .dl-click-2 {
            animation: none !important;
            opacity: 1 !important;
          }
          .dl-llm-icon, .dl-flash, .dl-input-cursor { animation: none !important; }
          .dl-app-field-text { width: ${TYPED_TEXT.length}ch !important; }
          .dl-title, .dl-tagline, .dl-vm-box, .dl-host-label { animation: none !important; opacity: 1 !important; }
        }

        .dl-title {
          position: absolute;
          top: 80px;
          left: 0;
          right: 0;
          text-align: center;
          font-size: 64px;
          color: #F9F9F9;
          font-weight: 700;
          white-space: nowrap;
          opacity: 0;
          animation: dl-fade-in-flat 500ms ease-out 100ms forwards;
          z-index: 10;
        }

        .dl-title-accent { color: #C3FFFD; }

        /* --- VM container boundary --- */
        .dl-vm-box {
          position: absolute;
          top: 24%;
          left: 1%;
          width: 52%;
          bottom: 3%;
          border: 1.5px dashed rgba(195, 255, 253, 0.25);
          border-radius: 16px;
          pointer-events: none;
          opacity: 0;
          animation: dl-fade-in-flat 600ms ease-out 300ms forwards;
          z-index: 0;
        }

        .dl-vm-label {
          position: absolute;
          top: -9px;
          left: 20px;
          background: #000;
          padding: 0 10px;
          font-size: 24px;
          color: rgba(195, 255, 253, 0.5);
          font-weight: 700;
          letter-spacing: 1.5px;
          text-transform: uppercase;
        }

        .dl-host-label {
          position: absolute;
          top: 2px;
          left: 24px;
          font-size: 22px;
          color: #666;
          font-weight: 700;
          letter-spacing: 1px;
          text-transform: uppercase;
          opacity: 0;
          animation: dl-fade-in-flat 400ms ease-out 1600ms forwards;
          z-index: 6;
        }

        .dl-tagline {
          position: absolute;
          bottom: 16px;
          left: 50%;
          transform: translateX(-50%);
          font-size: 24px;
          color: #9BA4A6;
          opacity: 0;
          animation: dl-fade-in-flat 500ms ease-out 9500ms forwards;
          white-space: nowrap;
          z-index: 10;
        }

        .dl-tagline-em { color: #C3FFFD; font-weight: 700; }
      `}</style>

      <div className="dl-title">
        <span className="dl-title-accent">Computer-use</span> Architecture
      </div>

      {/* VM boundary */}
      <div className="dl-vm-box">
        <span className="dl-vm-label">Virtual Desktop (Docker / VM)</span>
      </div>

      {/* Host label next to terminal */}
      <div className="dl-host-label">HOST</div>

      {/* Mini terminal carried from Run scene */}
      <div className="dl-mini-term">
        <div className="dl-mini-term-bar">
          <div className="dl-dot" style={{ background: "#FF3B4D" }} />
          <div className="dl-dot" style={{ background: "#E3B341" }} />
          <div className="dl-dot" style={{ background: "#00C781" }} />
        </div>
        <div className="dl-mini-term-body">
          <div className="dl-mini-term-line">
            <span className="dl-mini-term-prompt">$&nbsp;</span>
            <span className="dl-mini-term-cmd">desktest run task.json</span>
          </div>
          <div className="dl-mini-term-output">
            <span className="dl-mini-term-arrow">{"▸ "}</span>
            <span className="dl-mini-term-text">Starting agent loop...</span>
          </div>
        </div>
      </div>

      {/* Desktop mockup */}
      <div className="dl-desktop">
        <div className="dl-desktop-titlebar">
          <div className="dl-dot" style={{ background: "#FF3B4D" }} />
          <div className="dl-dot" style={{ background: "#E3B341" }} />
          <div className="dl-dot" style={{ background: "#00C781" }} />
          <span className="dl-desktop-titlebar-label">Electron Todo App</span>
        </div>
        <div className="dl-desktop-body">
          <div className="dl-app-header">My Todos</div>
          <div className="dl-app-input">
            <div className="dl-app-field">
              <span className="dl-app-field-text dl-app-field-text-clear">
                {TYPED_TEXT}
              </span>
              <span className="dl-input-cursor" />
              <div className="dl-app-field-focus" />
            </div>
            <div className="dl-app-btn dl-app-btn-press">Add</div>
          </div>
          <div className="dl-app-list">
            <div className="dl-app-item">Take out trash</div>
            <div className="dl-app-item">Walk the dog</div>
            <div className="dl-app-item dl-app-item-new">Buy groceries</div>
          </div>
        </div>
        <div className="dl-flash" />
      </div>

      {/* Mouse cursor */}
      <svg className="dl-cursor" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
        <path d="M5 3l14 8-6 2-4 6-4-16z" fill="#fff" stroke="#000" strokeWidth="1.5" strokeLinejoin="round"/>
      </svg>
      <div className="dl-click-1" />
      <div className="dl-click-2" />

      {/* Arrows */}
      <svg className="dl-arrows-svg" viewBox="0 0 1920 1080" preserveAspectRatio="xMidYMid meet">
        {/* Top arrow: Desktop → LLM */}
        <g className="dl-arrow-top-group">
          <path
            className="dl-arrow-path"
            d="M 940 394 C 1060 214, 1460 214, 1580 394"
          />
          <polygon
            className="dl-arrow-head"
            points="1570,379 1590,399 1568,399"
          />
          <text className="dl-arrow-label" x="1260" y="234" textAnchor="middle">
            Screenshot + A11y Tree
          </text>
        </g>

        {/* Bottom arrow: LLM → Desktop */}
        <g className="dl-arrow-bottom-group">
          <path
            className="dl-arrow-path"
            d="M 1580 734 C 1460 914, 1060 914, 940 734"
          />
          <polygon
            className="dl-arrow-head"
            points="950,749 930,729 952,729"
          />
          <text className="dl-arrow-label-code" x="1260" y="914" textAnchor="middle">
            pyautogui.click(180, 245)
          </text>
          <text className="dl-arrow-label-code" x="1260" y="944" textAnchor="middle">
            typewrite(&apos;Buy groceries&apos;)
          </text>
          <text className="dl-arrow-label-code" x="1260" y="974" textAnchor="middle">
            pyautogui.click(380, 245)
          </text>
        </g>
      </svg>

      {/* LLM icon */}
      <div className="dl-llm">
        <svg
          className="dl-llm-icon"
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 256 256"
          fill="#C3FFFD"
        >
          <circle cx="128" cy="24" r="10" fill="none" stroke="#C3FFFD" strokeWidth="6" />
          <rect x="125" y="34" width="6" height="20" />
          <path d="M40,144a88,88,0,0,1,176,0v44a44,44,0,0,1-44,44H84a44,44,0,0,1-44-44Z" />
          <rect x="64" y="116" width="128" height="80" rx="28" fill="#1C1C1C" />
          <rect x="88" y="138" width="24" height="36" rx="12" fill="#C3FFFD" />
          <rect x="144" y="138" width="24" height="36" rx="12" fill="#C3FFFD" />
        </svg>
        <span className="dl-llm-label">LLM</span>
      </div>

      {/* Step counter */}
      <div className="dl-step">
        <span className="dl-step-label">Step</span>
        <span className="dl-step-num">1</span>
        <span className="dl-step-label">/ 15</span>
        <div className="dl-step-dots">
          <div className="dl-step-dot dl-step-dot-active" />
          <div className="dl-step-dot" />
          <div className="dl-step-dot" />
          <div className="dl-step-dot" />
          <div className="dl-step-dot" />
        </div>
      </div>

    </div>
  );
}
