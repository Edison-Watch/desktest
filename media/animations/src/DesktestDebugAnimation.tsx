import React, { useState, useEffect } from "react";

const CYCLE_MS = 13000;
const CMD_LOGS = "desktest logs artifacts/ --steps 3-5";

export default function DesktestDebugAnimation() {
  const [cycle, setCycle] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setCycle((c) => c + 1), CYCLE_MS);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="dres-scene" key={cycle}>
      <style>{`
        .dres-scene {
          position: relative;
          width: 100%;
          aspect-ratio: 16 / 9;
          background: #000;
          overflow: hidden;
          font-family: 'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace;
          display: flex;
          flex-direction: column;
          align-items: center;
          padding: 80px 32px 44px;
        }

        /* --- Heading --- */
        .dres-heading {
          font-size: 64px;
          color: #F9F9F9;
          font-weight: 700;
          white-space: nowrap;
          opacity: 0;
          animation: dres-fade-in 500ms ease-out 100ms forwards;
          margin-bottom: 6px;
          flex-shrink: 0;
        }

        .dres-heading-em {
          color: #C3FFFD;
        }

        /* --- Two-column layout --- */
        .dres-columns {
          display: flex;
          gap: 36px;
          flex: 1;
          width: 95%;
          max-width: 1800px;
          min-height: 0;
        }

        .dres-col {
          flex: 1;
          display: flex;
          flex-direction: column;
          align-items: center;
          min-height: 0;
        }

        /* --- Icon rows (top of each column) --- */
        .dres-icons {
          display: flex;
          gap: 18px;
          align-items: center;
          opacity: 0;
          animation: dres-fade-in 400ms ease-out 200ms forwards;
          flex-shrink: 0;
        }

        .dres-agent-icon {
          width: 52px;
          height: 52px;
          border-radius: 10px;
          background: #1C1C1C;
          border: 1px solid #383838;
          display: flex;
          align-items: center;
          justify-content: center;
          padding: 8px;
        }

        .dres-agent-icon svg {
          width: 100%;
          height: 100%;
        }

        .dres-user-icon {
          width: 52px;
          height: 52px;
          border-radius: 10px;
          background: #1C1C1C;
          border: 1px solid #383838;
          display: flex;
          align-items: center;
          justify-content: center;
          padding: 8px;
        }

        .dres-user-icon svg {
          width: 100%;
          height: 100%;
        }

        /* --- Arrow + label --- */
        .dres-arrow-label {
          display: flex;
          flex-direction: column;
          align-items: center;
          flex-shrink: 0;
          opacity: 0;
          animation: dres-fade-in 300ms ease-out 600ms forwards;
        }

        .dres-arrow-path {
          stroke: #C3FFFD;
          stroke-width: 2.5;
          fill: none;
          stroke-dasharray: 24;
          stroke-dashoffset: 24;
          animation: dres-arrow-draw 400ms ease-out 800ms forwards;
        }

        .dres-arrow-path-user {
          stroke: #E3B341;
          stroke-width: 2.5;
          fill: none;
          stroke-dasharray: 24;
          stroke-dashoffset: 24;
          animation: dres-arrow-draw 400ms ease-out 800ms forwards;
        }

        .dres-arrow-head {
          fill: #C3FFFD;
          opacity: 0;
          animation: dres-fade-in 150ms ease-out 1200ms forwards;
        }

        .dres-arrow-head-user {
          fill: #E3B341;
          opacity: 0;
          animation: dres-fade-in 150ms ease-out 1200ms forwards;
        }

        @keyframes dres-arrow-draw {
          to { stroke-dashoffset: 0; }
        }

        .dres-arrow-label-text {
          font-size: 24px;
          white-space: nowrap;
          margin-top: 4px;
          margin-bottom: 10px;
        }

        .dres-arrow-label-text-cyan {
          color: #C3FFFD;
        }

        .dres-arrow-label-text-yellow {
          color: #E3B341;
        }

        /* --- Terminal (left panel) --- */
        .dres-terminal {
          width: 100%;
          border-radius: 10px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow:
            0 16px 48px rgba(0, 0, 0, 0.8),
            0 6px 20px rgba(0, 0, 0, 0.5);
          opacity: 0;
          animation: dres-fade-in 400ms ease-out 1300ms forwards;
          display: flex;
          flex-direction: column;
          flex: 1;
          min-height: 0;
        }

        .dres-titlebar {
          background: #2F3440;
          border-bottom: 1px solid #383838;
          padding: 10px 16px;
          display: flex;
          gap: 6px;
          align-items: center;
          flex-shrink: 0;
        }

        .dres-dot {
          width: 12px;
          height: 12px;
          border-radius: 50%;
        }

        .dres-body {
          background: #1C1C1C;
          padding: 18px 20px;
          font-size: 30px;
          line-height: 1.7;
          flex: 1;
          overflow: hidden;
        }

        .dres-prompt {
          color: #C3FFFD;
          font-weight: 700;
        }

        .dres-cmd {
          color: #F9F9F9;
          overflow: hidden;
          white-space: nowrap;
          display: inline-block;
          vertical-align: bottom;
          width: 0;
          animation: dres-type ${CMD_LOGS.length * 22}ms steps(${CMD_LOGS.length}) 1700ms forwards;
        }

        @keyframes dres-type {
          to { width: ${CMD_LOGS.length}ch; }
        }

        .dres-line {
          display: block;
          white-space: nowrap;
          opacity: 0;
        }

        .dres-l1 { animation: dres-fade-in 120ms ease-out 2600ms forwards; }
        .dres-l2 { animation: dres-fade-in 120ms ease-out 2800ms forwards; }
        .dres-l3 { animation: dres-fade-in 120ms ease-out 3000ms forwards; }
        .dres-l4 { animation: dres-fade-in 120ms ease-out 3300ms forwards; }
        .dres-l5 { animation: dres-fade-in 120ms ease-out 3550ms forwards; }
        .dres-l6 { animation: dres-fade-in 120ms ease-out 3800ms forwards; }
        .dres-l7 { animation: dres-fade-in 120ms ease-out 4100ms forwards; }
        .dres-l8 { animation: dres-fade-in 120ms ease-out 4350ms forwards; }
        .dres-l9 { animation: dres-fade-in 120ms ease-out 4600ms forwards; }
        .dres-l10 { animation: dres-fade-in 120ms ease-out 5000ms forwards; }
        .dres-l11 { animation: dres-fade-in 120ms ease-out 5250ms forwards; }
        .dres-l12 { animation: dres-fade-in 120ms ease-out 5500ms forwards; }

        .dres-step-fail {
          background: rgba(255, 59, 77, 0.08);
          border-left: 2px solid #FF3B4D;
          padding-left: 8px;
          margin-left: -10px;
        }

        /* --- Browser monitor panel (right) --- */
        .dres-browser {
          width: 100%;
          border-radius: 10px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow:
            0 16px 48px rgba(0, 0, 0, 0.8),
            0 6px 20px rgba(0, 0, 0, 0.5);
          opacity: 0;
          animation: dres-slide-in 500ms cubic-bezier(0.16, 1, 0.3, 1) 1500ms forwards;
          display: flex;
          flex-direction: column;
          flex: 1;
          min-height: 0;
        }

        @keyframes dres-slide-in {
          from { opacity: 0; transform: translateX(20px); }
          to { opacity: 1; transform: translateX(0); }
        }

        .dres-browser-bar {
          background: #2F3440;
          border-bottom: 1px solid #383838;
          padding: 10px 16px;
          display: flex;
          align-items: center;
          gap: 8px;
          flex-shrink: 0;
        }

        .dres-browser-dots {
          display: flex;
          gap: 5px;
        }

        .dres-browser-url {
          flex: 1;
          background: #1C1C1C;
          border-radius: 4px;
          padding: 6px 12px;
          font-size: 26px;
          color: #9BA4A6;
          display: flex;
          align-items: center;
          gap: 4px;
        }

        .dres-browser-url-text {
          color: #E3B341;
        }

        .dres-browser-body {
          background: #1C1C1C;
          padding: 16px;
          flex: 1;
          overflow: hidden;
          display: flex;
          flex-direction: column;
          gap: 10px;
        }

        .dres-monitor-section {
          opacity: 0;
        }

        .dres-mon-1 { animation: dres-fade-in 200ms ease-out 3800ms forwards; }
        .dres-mon-2 { animation: dres-fade-in 200ms ease-out 5800ms forwards; }

        .dres-mon-label {
          font-size: 24px;
          color: #9BA4A6;
          margin-bottom: 8px;
          display: flex;
          align-items: center;
          gap: 5px;
        }

        .dres-mon-label-icon {
          font-size: 24px;
        }

        .dres-screenshots {
          display: flex;
          gap: 6px;
        }

        .dres-ss {
          flex: 1;
          aspect-ratio: 16 / 10;
          border-radius: 4px;
          border: 1px solid #383838;
          background: #2a2a2a;
          position: relative;
          overflow: hidden;
          opacity: 0;
        }

        .dres-ss-1 { animation: dres-fade-in 200ms ease-out 4000ms forwards; }
        .dres-ss-2 { animation: dres-fade-in 200ms ease-out 4300ms forwards; }
        .dres-ss-3 { animation: dres-fade-in 200ms ease-out 4600ms forwards; }

        .dres-ss-content {
          width: 100%;
          height: 100%;
          display: flex;
          align-items: center;
          justify-content: center;
        }

        .dres-mini-desktop {
          width: 80%;
          height: 70%;
          background: #1a1a2e;
          border-radius: 2px;
          position: relative;
        }

        .dres-mini-bar {
          height: 12%;
          background: #2F3440;
          border-radius: 2px 2px 0 0;
        }

        .dres-mini-cursor {
          position: absolute;
          width: 3px;
          height: 3px;
          background: #fff;
          border-radius: 50%;
        }

        .dres-ss-label {
          position: absolute;
          bottom: 4px;
          left: 6px;
          font-size: 24px;
          color: #9BA4A6;
          opacity: 0.7;
        }

        .dres-ss-fail { border-color: #FF3B4D; }

        .dres-ss-fail-badge {
          position: absolute;
          top: 2px;
          right: 2px;
          width: 9px;
          height: 9px;
          border-radius: 50%;
          background: #FF3B4D;
          display: flex;
          align-items: center;
          justify-content: center;
        }

        .dres-video-area {
          border: 1px solid #383838;
          border-radius: 6px;
          overflow: hidden;
          background: #0d0d0d;
          position: relative;
          flex: 1;
          min-height: 0;
        }

        .dres-play-btn {
          position: absolute;
          top: 50%;
          left: 50%;
          transform: translate(-50%, -50%);
          width: 28px;
          height: 28px;
          border-radius: 50%;
          background: rgba(195, 255, 253, 0.15);
          border: 1.5px solid #C3FFFD;
          display: flex;
          align-items: center;
          justify-content: center;
          opacity: 0;
          animation:
            dres-fade-in 300ms ease-out 6200ms forwards,
            dres-play-hide 200ms ease-out 6700ms forwards;
        }

        @keyframes dres-play-hide {
          to { opacity: 0; transform: translate(-50%, -50%) scale(0.8); }
        }

        /* --- Video playback content --- */
        .dres-vid-app {
          position: absolute;
          top: 15%;
          left: 12%;
          width: 76%;
          height: 70%;
          background: #F5F5F5;
          border-radius: 2px;
          overflow: hidden;
          display: flex;
          flex-direction: column;
        }

        .dres-vid-app-bar {
          height: 14%;
          background: #E0E0E0;
          display: flex;
          align-items: center;
          padding: 0 5%;
          gap: 3%;
        }

        .dres-vid-app-tab {
          width: 18%;
          height: 40%;
          border-radius: 1px;
          background: #ccc;
        }

        .dres-vid-app-tab-active {
          background: #0969da;
        }

        .dres-vid-app-body {
          flex: 1;
          padding: 6% 8%;
          display: flex;
          flex-direction: column;
          gap: 6%;
          position: relative;
        }

        .dres-vid-app-row {
          display: flex;
          gap: 4%;
          align-items: center;
        }

        .dres-vid-app-input {
          flex: 1;
          height: 22%;
          background: #fff;
          border: 1px solid #ddd;
          border-radius: 1px;
          display: flex;
          align-items: center;
          padding: 0 4%;
          overflow: hidden;
        }

        .dres-vid-app-input-text {
          font-size: 3px;
          font-family: inherit;
          color: #333;
          opacity: 0;
          animation: dres-fade-in 150ms ease-out 7600ms forwards;
        }

        .dres-vid-input-focus {
          animation: dres-vid-focus 200ms ease-out 7200ms forwards;
        }

        @keyframes dres-vid-focus {
          to { border-color: #0969da; box-shadow: 0 0 0 1px rgba(9,105,218,0.3); }
        }

        .dres-vid-app-btn {
          width: 16%;
          height: 22%;
          background: #0969da;
          border-radius: 1px;
          animation: dres-vid-btn-press 200ms ease-out 8200ms forwards;
        }

        @keyframes dres-vid-btn-press {
          0% { transform: scale(1); }
          50% { transform: scale(0.85); }
          100% { transform: scale(1); }
        }

        .dres-vid-app-item {
          height: 16%;
          border-radius: 1px;
          background: #e0e0e0;
        }

        .dres-vid-app-item-new {
          background: #d4edda;
          border: 1px solid #28a745;
          opacity: 0;
          animation: dres-fade-in 150ms ease-out 8400ms forwards;
        }

        .dres-vid-cursor {
          position: absolute;
          width: 6px;
          height: 8px;
          z-index: 5;
          pointer-events: none;
          filter: drop-shadow(0 1px 2px rgba(0,0,0,0.8));
          opacity: 0;
          animation:
            dres-fade-in 100ms ease-out 6900ms forwards,
            dres-vid-cursor-move 3000ms ease-in-out 6900ms forwards;
        }

        @keyframes dres-vid-cursor-move {
          0%   { top: 25%; left: 20%; }
          15%  { top: 48%; left: 35%; }
          17%  { top: 49%; left: 35%; }
          19%  { top: 48%; left: 35%; }
          50%  { top: 48%; left: 35%; }
          70%  { top: 48%; left: 65%; }
          72%  { top: 49%; left: 65%; }
          74%  { top: 48%; left: 65%; }
          100% { top: 48%; left: 65%; }
        }

        .dres-vid-click {
          position: absolute;
          width: 8px;
          height: 8px;
          border-radius: 50%;
          border: 1.5px solid rgba(0,0,0,0.5);
          opacity: 0;
          pointer-events: none;
          z-index: 4;
          transform: translate(-50%, -50%) scale(0);
        }

        .dres-vid-click-1 {
          top: 48%;
          left: 35%;
          animation: dres-vid-click-pulse 350ms ease-out 7200ms forwards;
        }

        .dres-vid-click-2 {
          top: 48%;
          left: 65%;
          animation: dres-vid-click-pulse 350ms ease-out 8200ms forwards;
        }

        @keyframes dres-vid-click-pulse {
          0% { opacity: 0.8; transform: translate(-50%, -50%) scale(0.3); }
          100% { opacity: 0; transform: translate(-50%, -50%) scale(2); }
        }

        .dres-vid-check {
          position: absolute;
          bottom: 12%;
          right: 8%;
          width: 10px;
          height: 10px;
          opacity: 0;
          z-index: 5;
          animation: dres-vid-check-pop 400ms cubic-bezier(0.34, 1.56, 0.64, 1) 8600ms forwards;
        }

        @keyframes dres-vid-check-pop {
          0%   { opacity: 0; transform: scale(0); }
          60%  { opacity: 1; transform: scale(1.2); }
          100% { opacity: 1; transform: scale(1); }
        }

        .dres-video-timeline {
          position: absolute;
          bottom: 5px;
          left: 8px;
          right: 8px;
          height: 2px;
          background: #383838;
          border-radius: 2px;
          overflow: hidden;
        }

        .dres-video-progress {
          height: 100%;
          width: 0%;
          background: #C3FFFD;
          border-radius: 2px;
          animation: dres-video-fill 3500ms linear 6700ms forwards;
        }

        @keyframes dres-video-fill {
          to { width: 70%; }
        }

        .dres-video-time {
          position: absolute;
          bottom: 10px;
          right: 8px;
          font-size: 18px;
          color: #9BA4A6;
          opacity: 0;
          animation: dres-fade-in 200ms ease-out 6700ms forwards;
        }

        /* --- Colors --- */
        .dres-dim { color: #9BA4A6; opacity: 0.6; }
        .dres-white { color: #F9F9F9; }
        .dres-cyan { color: #C3FFFD; }
        .dres-green { color: #00C781; }
        .dres-red { color: #FF3B4D; }
        .dres-red-bold { color: #FF3B4D; font-weight: 700; }
        .dres-yellow { color: #E3B341; }

        /* --- Tagline --- */
        .dres-tagline {
          position: absolute;
          bottom: 16px;
          left: 50%;
          transform: translateX(-50%);
          font-size: 24px;
          color: #9BA4A6;
          opacity: 0;
          animation: dres-fade-in 500ms ease-out 10500ms forwards;
          white-space: nowrap;
        }

        .dres-tagline-em {
          color: #C3FFFD;
          font-weight: 700;
        }

        @keyframes dres-fade-in {
          from { opacity: 0; }
          to { opacity: 1; }
        }

        @media (prefers-reduced-motion: reduce) {
          .dres-heading, .dres-icons, .dres-arrow-label,
          .dres-terminal, .dres-browser, .dres-line,
          .dres-ss, .dres-monitor-section,
          .dres-play-btn, .dres-tagline, .dres-cmd {
            animation: none !important;
            opacity: 1 !important;
            width: auto !important;
            transform: none !important;
          }
        }
      `}</style>

      {/* Heading */}
      <div className="dres-heading">
        <span className="dres-heading-em">Agent-first</span> debugging and observability
      </div>

      {/* Two-column layout */}
      <div className="dres-columns">
        {/* Left column: agents → terminal */}
        <div className="dres-col">
          <div className="dres-icons">
            {/* Claude Code */}
            <div className="dres-agent-icon">
              <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                <path clipRule="evenodd" d="M20.998 10.949H24v3.102h-3v3.028h-1.487V20H18v-2.921h-1.487V20H15v-2.921H9V20H7.488v-2.921H6V20H4.487v-2.921H3V14.05H0V10.95h3V5h17.998v5.949zM6 10.949h1.488V8.102H6v2.847zm10.51 0H18V8.102h-1.49v2.847z" fill="#D97757" fillRule="evenodd"/>
              </svg>
            </div>
            {/* Codex */}
            <div className="dres-agent-icon">
              <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                <path d="M19.503 0H4.496A4.496 4.496 0 000 4.496v15.007A4.496 4.496 0 004.496 24h15.007A4.496 4.496 0 0024 19.503V4.496A4.496 4.496 0 0019.503 0z" fill="#fff"/>
                <path d="M9.064 3.344a4.578 4.578 0 012.285-.312c1 .115 1.891.54 2.673 1.275.01.01.024.017.037.021a.09.09 0 00.043 0 4.55 4.55 0 013.046.275l.047.022.116.057a4.581 4.581 0 012.188 2.399c.209.51.313 1.041.315 1.595a4.24 4.24 0 01-.134 1.223.123.123 0 00.03.115c.594.607.988 1.33 1.183 2.17.289 1.425-.007 2.71-.887 3.854l-.136.166a4.548 4.548 0 01-2.201 1.388.123.123 0 00-.081.076c-.191.551-.383 1.023-.74 1.494-.9 1.187-2.222 1.846-3.711 1.838-1.187-.006-2.239-.44-3.157-1.302a.107.107 0 00-.105-.024c-.388.125-.78.143-1.204.138a4.441 4.441 0 01-1.945-.466 4.544 4.544 0 01-1.61-1.335c-.152-.202-.303-.392-.414-.617a5.81 5.81 0 01-.37-.961 4.582 4.582 0 01-.014-2.298.124.124 0 00.006-.056.085.085 0 00-.027-.048 4.467 4.467 0 01-1.034-1.651 3.896 3.896 0 01-.251-1.192 5.189 5.189 0 01.141-1.6c.337-1.112.982-1.985 1.933-2.618.212-.141.413-.251.601-.33.215-.089.43-.164.646-.227a.098.098 0 00.065-.066 4.51 4.51 0 01.829-1.615 4.535 4.535 0 011.837-1.388zm3.482 10.565a.637.637 0 000 1.272h3.636a.637.637 0 100-1.272h-3.636zM8.462 9.23a.637.637 0 00-1.106.631l1.272 2.224-1.266 2.136a.636.636 0 101.095.649l1.454-2.455a.636.636 0 00.005-.64L8.462 9.23z" fill="url(#dres-codex-grad)"/>
                <defs>
                  <linearGradient gradientUnits="userSpaceOnUse" id="dres-codex-grad" x1="12" x2="12" y1="3" y2="21">
                    <stop stopColor="#B1A7FF"/>
                    <stop offset=".5" stopColor="#7A9DFF"/>
                    <stop offset="1" stopColor="#3941FF"/>
                  </linearGradient>
                </defs>
              </svg>
            </div>
            {/* Cursor */}
            <div className="dres-agent-icon">
              <svg viewBox="0 0 466.73 532.09" xmlns="http://www.w3.org/2000/svg">
                <path d="M457.43,125.94L244.42,2.96c-6.84-3.95-15.28-3.95-22.12,0L9.3,125.94c-5.75,3.32-9.3,9.46-9.3,16.11v247.99c0,6.65,3.55,12.79,9.3,16.11l213.01,122.98c6.84,3.95,15.28,3.95,22.12,0l213.01-122.98c5.75-3.32,9.3-9.46,9.3-16.11v-247.99c0-6.65-3.55-12.79-9.3-16.11h-.01ZM444.05,151.99l-205.63,356.16c-1.39,2.4-5.06,1.42-5.06-1.36v-233.21c0-4.66-2.49-8.97-6.53-11.31L24.87,145.67c-2.4-1.39-1.42-5.06,1.36-5.06h411.26c5.84,0,9.49,6.33,6.57,11.39h-.01Z" fill="#F9F9F9"/>
              </svg>
            </div>
            {/* OpenClaw */}
            <div className="dres-agent-icon">
              <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                <path d="M12 2.568c-6.33 0-9.495 5.275-9.495 9.495 0 4.22 3.165 8.44 6.33 9.494v2.11h2.11v-2.11s1.055.422 2.11 0v2.11h2.11v-2.11c3.165-1.055 6.33-5.274 6.33-9.494S18.33 2.568 12 2.568z" fill="url(#dres-claw-0)"/>
                <path d="M3.56 9.953C.396 8.898-.66 11.008.396 13.118c1.055 2.11 3.164 1.055 4.22-1.055.632-1.477 0-2.11-1.056-2.11z" fill="url(#dres-claw-1)"/>
                <path d="M20.44 9.953c3.164-1.055 4.22 1.055 3.164 3.165-1.055 2.11-3.164 1.055-4.22-1.055-.632-1.477 0-2.11 1.056-2.11z" fill="url(#dres-claw-2)"/>
                <path d="M5.507 1.875c.476-.285 1.036-.233 1.615.037.577.27 1.223.774 1.937 1.488a.316.316 0 01-.447.447c-.693-.693-1.279-1.138-1.757-1.361-.475-.222-.795-.205-1.022-.069a.317.317 0 01-.326-.542zM16.877 1.913c.58-.27 1.14-.323 1.616-.038a.317.317 0 01-.326.542c-.227-.136-.547-.153-1.022.069-.478.223-1.064.668-1.756 1.361a.316.316 0 11-.448-.447c.714-.714 1.36-1.218 1.936-1.487z" fill="#FF4D4D"/>
                <path d="M8.835 9.109a1.266 1.266 0 100-2.532 1.266 1.266 0 000 2.532zM15.165 9.109a1.266 1.266 0 100-2.532 1.266 1.266 0 000 2.532z" fill="#050810"/>
                <path d="M9.046 8.16a.527.527 0 100-1.056.527.527 0 000 1.055zM15.376 8.16a.527.527 0 100-1.055.527.527 0 000 1.054z" fill="#00E5CC"/>
                <defs>
                  <linearGradient gradientUnits="userSpaceOnUse" id="dres-claw-0" x1="-.659" x2="27.023" y1=".458" y2="22.855">
                    <stop stopColor="#FF4D4D"/>
                    <stop offset="1" stopColor="#991B1B"/>
                  </linearGradient>
                  <linearGradient gradientUnits="userSpaceOnUse" id="dres-claw-1" x1="0" x2="4.311" y1="9.672" y2="14.949">
                    <stop stopColor="#FF4D4D"/>
                    <stop offset="1" stopColor="#991B1B"/>
                  </linearGradient>
                  <linearGradient gradientUnits="userSpaceOnUse" id="dres-claw-2" x1="19.385" x2="24.399" y1="9.953" y2="14.462">
                    <stop stopColor="#FF4D4D"/>
                    <stop offset="1" stopColor="#991B1B"/>
                  </linearGradient>
                </defs>
              </svg>
            </div>
          </div>

          {/* Arrow + label */}
          <div className="dres-arrow-label">
            <svg width="8" height="20" viewBox="0 0 8 20">
              <path className="dres-arrow-path" d="M4 0 L4 14" />
              <polygon className="dres-arrow-head" points="4,20 1,14 7,14" />
            </svg>
            <span className="dres-arrow-label-text dres-arrow-label-text-cyan">desktest logs</span>
          </div>

          {/* Terminal */}
          <div className="dres-terminal">
            <div className="dres-titlebar">
              <div className="dres-dot" style={{ background: "#FF3B4D" }} />
              <div className="dres-dot" style={{ background: "#E3B341" }} />
              <div className="dres-dot" style={{ background: "#00C781" }} />
            </div>
            <div className="dres-body">
              <span style={{ display: "block" }}>
                <span className="dres-prompt">$ </span>
                <span className="dres-cmd">{CMD_LOGS}</span>
              </span>
              <span className="dres-line dres-l1">&nbsp;</span>
              <span className="dres-line dres-l2">
                <span className="dres-cyan">{"Trajectory"}</span>
                <span className="dres-dim">{" — 5 steps, 42s total"}</span>
              </span>
              <span className="dres-line dres-l3">&nbsp;</span>
              <span className="dres-line dres-l4">
                <span className="dres-dim">{"  ┌ "}</span>
                <span className="dres-white">{"Step 3"}</span>
                <span className="dres-dim">{" (8.2s)"}</span>
              </span>
              <span className="dres-line dres-l5">
                <span className="dres-dim">{"  │ "}</span>
                <span className="dres-green">{"▸ "}</span>
                <span className="dres-dim">{"click(180, 245) → type('Buy groceries')"}</span>
              </span>
              <span className="dres-line dres-l6">
                <span className="dres-dim">{"  └ "}</span>
                <span className="dres-green">{"✓ "}</span>
                <span className="dres-dim">{"🌄 screenshot → agent context"}</span>
              </span>
              <span className="dres-line dres-l7">
                <span className="dres-dim">{"  ┌ "}</span>
                <span className="dres-white">{"Step 4"}</span>
                <span className="dres-dim">{" (12.1s)"}</span>
              </span>
              <span className="dres-line dres-l8">
                <span className="dres-dim">{"  │ "}</span>
                <span className="dres-green">{"▸ "}</span>
                <span className="dres-dim">{"click(380, 245) → wait(2s)"}</span>
              </span>
              <span className="dres-line dres-l9">
                <span className="dres-dim">{"  └ "}</span>
                <span className="dres-green">{"✓ "}</span>
                <span className="dres-dim">{"🌄 screenshot → agent context"}</span>
              </span>
              <span className="dres-line dres-l10" style={{ marginTop: "2px" }}>
                <span className="dres-step-fail" style={{ display: "inline-block", padding: "2px 8px" }}>
                  <span className="dres-dim">{"┌ "}</span>
                  <span className="dres-white">{"Step 5"}</span>
                  <span className="dres-dim">{" (3.4s)"}</span>
                </span>
              </span>
              <span className="dres-line dres-l11">
                <span className="dres-step-fail" style={{ display: "inline-block", padding: "2px 8px" }}>
                  <span className="dres-dim">{"│ "}</span>
                  <span className="dres-red">{"▸ "}</span>
                  <span className="dres-dim">{"click(520, 310) → "}</span>
                  <span className="dres-red">{"element not found"}</span>
                </span>
              </span>
              <span className="dres-line dres-l12">
                <span className="dres-step-fail" style={{ display: "inline-block", padding: "2px 8px" }}>
                  <span className="dres-dim">{"└ "}</span>
                  <span className="dres-red-bold">{"✗ "}</span>
                  <span className="dres-red">{"FAILED"}</span>
                  <span className="dres-dim">{" — timeout waiting for selector"}</span>
                </span>
              </span>
            </div>
          </div>
        </div>

        {/* Right column: user → browser */}
        <div className="dres-col">
          <div className="dres-icons">
            <div className="dres-user-icon">
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="#9BA4A6">
                <path d="M230.92,212c-15.23-26.33-38.7-45.21-66.09-54.16a72,72,0,1,0-73.66,0C63.78,166.78,40.31,185.66,25.08,212a8,8,0,1,0,13.85,8c18.84-32.56,52.14-52,89.07-52s70.23,19.44,89.07,52a8,8,0,1,0,13.85-8ZM72,96a56,56,0,1,1,56,56A56.06,56.06,0,0,1,72,96Z"/>
              </svg>
            </div>
          </div>

          {/* Arrow + label */}
          <div className="dres-arrow-label">
            <svg width="8" height="20" viewBox="0 0 8 20">
              <path className="dres-arrow-path-user" d="M4 0 L4 14" />
              <polygon className="dres-arrow-head-user" points="4,20 1,14 7,14" />
            </svg>
            <span className="dres-arrow-label-text dres-arrow-label-text-yellow">--monitor</span>
          </div>

          {/* Browser */}
          <div className="dres-browser">
            <div className="dres-browser-bar">
              <div className="dres-browser-dots">
                <div className="dres-dot" style={{ background: "#FF3B4D", width: 9, height: 9 }} />
                <div className="dres-dot" style={{ background: "#E3B341", width: 9, height: 9 }} />
                <div className="dres-dot" style={{ background: "#00C781", width: 9, height: 9 }} />
              </div>
              <div className="dres-browser-url">
                <span className="dres-browser-url-text">localhost:8420</span>
                <span>/monitor</span>
              </div>
            </div>
            <div className="dres-browser-body">
              {/* Screenshots */}
              <div className="dres-monitor-section dres-mon-1">
                <div className="dres-mon-label">
                  <span className="dres-mon-label-icon">{"📸"}</span>
                  <span>Screenshots</span>
                </div>
                <div className="dres-screenshots">
                  <div className="dres-ss dres-ss-1">
                    <div className="dres-ss-content">
                      <div className="dres-mini-desktop">
                        <div className="dres-mini-bar" />
                        <div className="dres-mini-cursor" style={{ top: "45%", left: "25%" }} />
                      </div>
                    </div>
                    <span className="dres-ss-label">step 3</span>
                  </div>
                  <div className="dres-ss dres-ss-2">
                    <div className="dres-ss-content">
                      <div className="dres-mini-desktop">
                        <div className="dres-mini-bar" />
                        <div className="dres-mini-cursor" style={{ top: "45%", left: "55%" }} />
                      </div>
                    </div>
                    <span className="dres-ss-label">step 4</span>
                  </div>
                  <div className="dres-ss dres-ss-3 dres-ss-fail">
                    <div className="dres-ss-content">
                      <div className="dres-mini-desktop">
                        <div className="dres-mini-bar" />
                        <div className="dres-mini-cursor" style={{ top: "60%", left: "65%" }} />
                      </div>
                    </div>
                    <div className="dres-ss-fail-badge">
                      <svg width="5" height="5" viewBox="0 0 10 10" fill="none">
                        <path d="M2 2L8 8M8 2L2 8" stroke="#fff" strokeWidth="2" strokeLinecap="round"/>
                      </svg>
                    </div>
                    <span className="dres-ss-label" style={{ color: "#FF3B4D" }}>step 5</span>
                  </div>
                </div>
              </div>

              {/* Recording */}
              <div className="dres-monitor-section dres-mon-2" style={{ display: "flex", flexDirection: "column", flex: 1, minHeight: 0 }}>
                <div className="dres-mon-label">
                  <span className="dres-mon-label-icon">{"🎬"}</span>
                  <span>recording.mp4</span>
                </div>
                <div className="dres-video-area">
                  <div className="dres-vid-app">
                    <div className="dres-vid-app-bar">
                      <div className="dres-vid-app-tab" />
                      <div className="dres-vid-app-tab dres-vid-app-tab-active" />
                      <div className="dres-vid-app-tab" />
                    </div>
                    <div className="dres-vid-app-body">
                      <div className="dres-vid-app-row">
                        <div className="dres-vid-app-input dres-vid-input-focus">
                          <span className="dres-vid-app-input-text">Buy groceries</span>
                        </div>
                        <div className="dres-vid-app-btn" />
                      </div>
                      <div className="dres-vid-app-item" style={{ width: "70%" }} />
                      <div className="dres-vid-app-item" style={{ width: "55%" }} />
                      <div className="dres-vid-app-item dres-vid-app-item-new" style={{ width: "65%" }} />
                    </div>
                  </div>
                  <svg className="dres-vid-cursor" viewBox="0 0 24 24" fill="none">
                    <path d="M5 3l14 8-6 2-4 6-4-16z" fill="#fff" stroke="#000" strokeWidth="1.5" strokeLinejoin="round" />
                  </svg>
                  <div className="dres-vid-click dres-vid-click-1" />
                  <div className="dres-vid-click dres-vid-click-2" />
                  <svg className="dres-vid-check" viewBox="0 0 24 24" fill="none">
                    <circle cx="12" cy="12" r="11" fill="#28a745" />
                    <path d="M7 12.5L10.5 16L17 8.5" stroke="#fff" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                  <div className="dres-play-btn">
                    <svg width="10" height="10" viewBox="0 0 14 14" fill="none">
                      <path d="M4 2.5L11 7L4 11.5V2.5Z" fill="#C3FFFD"/>
                    </svg>
                  </div>
                  <div className="dres-video-timeline">
                    <div className="dres-video-progress" />
                  </div>
                  <span className="dres-video-time">0:38 / 0:42</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Tagline */}
      <div className="dres-tagline">
        <span className="dres-tagline-em">Agent-friendly first</span>
        <span> — structured logs, screenshots, and video for every run</span>
      </div>
    </div>
  );
}
