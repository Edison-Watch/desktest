import React, { useState, useEffect } from "react";

const CYCLE_MS = 11500;
const CMD = "desktest run task.json --qa";

export default function DesktestQAAnimation() {
  const [cycle, setCycle] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setCycle((c) => c + 1), CYCLE_MS);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="dqa-scene" key={cycle}>
      <style>{`
        .dqa-scene {
          position: relative;
          width: 100%;
          aspect-ratio: 16 / 9;
          background: #000;
          display: flex;
          flex-direction: column;
          align-items: center;
          overflow: hidden;
          font-family: 'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace;
          padding: 80px 16px 48px;
        }

        .dqa-heading {
          font-size: 64px;
          color: #F9F9F9;
          font-weight: 700;
          white-space: nowrap;
          opacity: 0;
          animation: dqa-fade-in 500ms ease-out 100ms forwards;
          margin-bottom: 18px;
          flex-shrink: 0;
        }

        .dqa-heading-em {
          color: #C3FFFD;
        }

        .dqa-main {
          display: flex;
          gap: 14px;
          flex: 1;
          width: 98%;
          max-width: 1800px;
          min-height: 0;
        }

        /* --- Desktop panel (left) --- */
        .dqa-left {
          flex: 1;
          min-width: 0;
          animation: dqa-left-hide 500ms ease-in 5200ms forwards;
        }

        @keyframes dqa-left-hide {
          to { flex: 0; width: 0; opacity: 0; overflow: hidden; padding: 0; margin: 0; }
        }

        .dqa-desktop {
          width: 100%;
          height: 100%;
          border-radius: 10px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow: 0 12px 40px rgba(0,0,0,0.5);
          opacity: 0;
          animation: dqa-fade-in 400ms ease-out 200ms forwards;
          display: flex;
          flex-direction: column;
        }

        .dqa-desk-bar {
          background: #2F3440;
          height: 14px;
          border-bottom: 1px solid #383838;
          flex-shrink: 0;
        }

        .dqa-desk-body {
          background: #1a1a2e;
          flex: 1;
          position: relative;
          display: flex;
          align-items: center;
          justify-content: center;
        }

        .dqa-mini-app {
          width: 75%;
          height: 65%;
          background: #F5F5F5;
          border-radius: 3px;
          overflow: hidden;
          display: flex;
          flex-direction: column;
        }

        .dqa-mini-app-bar {
          background: #E0E0E0;
          height: 10px;
          display: flex;
          align-items: center;
          padding: 0 4px;
          gap: 2px;
          flex-shrink: 0;
        }

        .dqa-mini-dot { width: 3px; height: 3px; border-radius: 50%; }

        .dqa-mini-app-body {
          padding: 4% 5%;
          flex: 1;
          display: flex;
          flex-direction: column;
          gap: 3%;
        }

        .dqa-mini-nav {
          display: flex;
          gap: 2px;
          padding: 0 2px;
          border-bottom: 1px solid #ddd;
        }

        .dqa-mini-tab {
          font-size: 4.5px;
          font-family: inherit;
          color: #999;
          padding: 3px 5px;
          border-bottom: 1.5px solid transparent;
          white-space: nowrap;
        }

        .dqa-mini-tab-active {
          color: #333;
          font-weight: 700;
          border-bottom-color: #0969da;
        }

        .dqa-mini-tab-click {
          animation: dqa-tab-activate 200ms ease-out 2276ms forwards;
        }

        @keyframes dqa-tab-activate {
          from { color: #999; border-bottom-color: transparent; }
          to { color: #333; font-weight: 700; border-bottom-color: #0969da; }
        }

        .dqa-mini-page-home {
          flex: 1;
          display: flex;
          flex-direction: column;
          gap: 3px;
          padding: 4px 0 0;
          animation: dqa-page-out 150ms ease-out 2276ms forwards;
        }

        @keyframes dqa-page-out {
          to { opacity: 0; height: 0; padding: 0; overflow: hidden; }
        }

        .dqa-mini-placeholder {
          height: 6px;
          background: #e0e0e0;
          border-radius: 2px;
        }

        .dqa-mini-page-todos {
          flex: 1;
          display: flex;
          flex-direction: column;
          gap: 4%;
          padding: 10% 0 0;
          opacity: 0;
          animation: dqa-fade-in 150ms ease-out 2380ms forwards;
        }

        .dqa-mini-row { display: flex; gap: 3px; align-items: center; }

        .dqa-mini-input {
          flex: 1;
          height: 14px;
          background: #fff;
          border: 1px solid #ddd;
          border-radius: 2px;
          display: flex;
          align-items: center;
          padding: 0 4px;
          overflow: hidden;
          animation: dqa-input-focus 200ms ease-out 2700ms forwards;
        }

        @keyframes dqa-input-focus {
          from { border-color: #ddd; box-shadow: none; }
          to { border-color: #0969da; box-shadow: 0 0 0 1px rgba(9,105,218,0.3); }
        }

        .dqa-mini-typed {
          font-size: 5px;
          font-family: inherit;
          color: #333;
          white-space: nowrap;
          opacity: 0;
          animation: dqa-fade-in 200ms ease-out 2900ms forwards;
        }

        .dqa-mini-btn {
          width: 20px;
          height: 14px;
          background: #0969da;
          border-radius: 2px;
          animation: dqa-btn-fail 1000ms ease-out 3172ms forwards;
        }

        @keyframes dqa-btn-fail {
          0%  { transform: scale(1); }
          3%  { transform: scale(0.85); }
          7%  { transform: scale(1); }
          39% { transform: scale(1); }
          42% { transform: scale(0.85); }
          46% { transform: scale(1); }
          64% { transform: scale(1); }
          67% { transform: scale(0.85); }
          71% { transform: scale(1); }
          81% { transform: scale(1); }
          84% { transform: scale(0.85); }
          88% { transform: scale(1); }
          100% { transform: scale(1); background: #dc3545; }
        }

        .dqa-mini-item { height: 8px; border-radius: 2px; }
        .dqa-mini-item-existing { width: 55%; background: #e0e0e0; }
        .dqa-mini-item-expected { width: 62%; border: 1px dashed rgba(255, 59, 77, 0.4); opacity: 0; animation: dqa-fade-in 200ms ease-out 4200ms forwards; }

        /* --- Cursor pointer --- */
        .dqa-ptr {
          position: absolute;
          width: 14px;
          height: 17px;
          z-index: 4;
          pointer-events: none;
          filter: drop-shadow(0 1px 3px rgba(0,0,0,0.9));
          top: 35%;
          left: 15%;
          opacity: 0;
          animation:
            dqa-fade-in 200ms ease-out 1600ms forwards,
            dqa-ptr-move 2800ms ease-in-out 1800ms forwards,
            dqa-fade-out 200ms ease-out 4800ms forwards;
        }

        @keyframes dqa-ptr-move {
          0%   { top: 35%; left: 15%; }
          14%  { top: 22%; left: 22%; }
          17%  { top: 23.5%; left: 22%; }
          19%  { top: 22%; left: 22%; }
          32%  { top: 57%; left: 48%; }
          35%  { top: 58.5%; left: 48%; }
          37%  { top: 57%; left: 48%; }
          46%  { top: 57%; left: 82%; }
          49%  { top: 58.5%; left: 82%; }
          51%  { top: 57%; left: 82%; }
          63%  { top: 58.5%; left: 82%; }
          65%  { top: 57%; left: 82%; }
          72%  { top: 58.5%; left: 82%; }
          74%  { top: 57%; left: 82%; }
          78%  { top: 58.5%; left: 82%; }
          80%  { top: 57%; left: 82%; }
          100% { top: 57%; left: 82%; }
        }

        /* --- Warning icon on bug detect --- */
        .dqa-warn-icon {
          position: absolute;
          top: 50%;
          left: 50%;
          width: 56px;
          height: 56px;
          transform: translate(-50%, -50%) scale(0);
          z-index: 6;
          opacity: 0;
          pointer-events: none;
          animation: dqa-warn-pop 500ms cubic-bezier(0.34, 1.56, 0.64, 1) 4200ms forwards;
          filter: drop-shadow(0 2px 8px rgba(227, 179, 65, 0.6));
        }

        @keyframes dqa-warn-pop {
          0%   { opacity: 0; transform: translate(-50%, -50%) scale(0); }
          60%  { opacity: 1; transform: translate(-50%, -50%) scale(1.15); }
          100% { opacity: 1; transform: translate(-50%, -50%) scale(1); }
        }

        /* --- Screenshot flash on bug detect --- */
        .dqa-flash {
          position: absolute;
          inset: 0;
          border: 2px solid #E3B341;
          border-radius: 10px;
          opacity: 0;
          pointer-events: none;
          z-index: 3;
          animation: dqa-flash-pulse 400ms ease-out 4400ms forwards;
        }

        @keyframes dqa-flash-pulse {
          0% { opacity: 0.8; }
          100% { opacity: 0; }
        }

        .dqa-click-ring {
          position: absolute;
          width: 20px;
          height: 20px;
          border-radius: 50%;
          border: 2px solid rgba(0, 0, 0, 0.8);
          opacity: 0;
          pointer-events: none;
          z-index: 5;
          transform: translate(-50%, -50%);
        }

        .dqa-click-1 {
          top: 22%;
          left: 22%;
          animation: dqa-click-pulse 600ms ease-out 2276ms forwards;
        }

        .dqa-click-2 {
          top: 57%;
          left: 48%;
          animation: dqa-click-pulse 600ms ease-out 2780ms forwards;
        }

        .dqa-click-3 {
          top: 57%;
          left: 82%;
          animation: dqa-click-pulse 600ms ease-out 3172ms forwards;
        }

        .dqa-click-4 {
          top: 57%;
          left: 82%;
          animation: dqa-click-pulse 600ms ease-out 3564ms forwards;
        }

        .dqa-click-5 {
          top: 57%;
          left: 82%;
          animation: dqa-click-pulse 600ms ease-out 3816ms forwards;
        }

        .dqa-click-6 {
          top: 57%;
          left: 82%;
          animation: dqa-click-pulse 600ms ease-out 3984ms forwards;
        }

        @keyframes dqa-click-pulse {
          0% { opacity: 1; transform: translate(-50%, -50%) scale(0.3); }
          100% { opacity: 0; transform: translate(-50%, -50%) scale(2.5); }
        }

        /* --- Terminal (center) --- */
        .dqa-terminal {
          flex: 1;
          border-radius: 12px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow: 0 20px 60px rgba(0,0,0,0.8), 0 8px 24px rgba(0,0,0,0.5);
          opacity: 0;
          animation: dqa-fade-in 300ms ease-out 200ms forwards;
          display: flex;
          flex-direction: column;
        }

        .dqa-titlebar {
          background: #2F3440;
          border-bottom: 1px solid #383838;
          padding: 10px 14px;
          display: flex;
          gap: 7px;
          align-items: center;
          flex-shrink: 0;
        }

        .dqa-dot { width: 10px; height: 10px; border-radius: 50%; }

        .dqa-body {
          background: #1C1C1C;
          padding: 16px 20px;
          font-size: clamp(18px, 2.5vw, 28px);
          line-height: 1.8;
          flex: 1;
          overflow: hidden;
        }

        .dqa-prompt { color: #C3FFFD; font-weight: 700; }

        .dqa-cmd {
          color: #F9F9F9;
          overflow: hidden;
          white-space: nowrap;
          display: inline-block;
          vertical-align: bottom;
          width: 0;
          animation: dqa-type ${CMD.length * 25}ms steps(${CMD.length}) 400ms forwards;
        }

        @keyframes dqa-type {
          from { width: 0; }
          to { width: ${CMD.length}ch; }
        }

        .dqa-line { display: block; opacity: 0; }
        .dqa-l-qa   { animation: dqa-fade-in 150ms ease-out 1400ms forwards; }
        .dqa-l-gap1 { animation: dqa-fade-in 100ms ease-out 1800ms forwards; }
        .dqa-l-s1   { animation: dqa-fade-in 120ms ease-out 2000ms forwards; }
        .dqa-l-s2   { animation: dqa-fade-in 120ms ease-out 2600ms forwards; }
        .dqa-l-s3   { animation: dqa-fade-in 120ms ease-out 3200ms forwards; }
        .dqa-l-s4   { animation: dqa-fade-in 120ms ease-out 3800ms forwards; }
        .dqa-l-warn { animation: dqa-fade-in 150ms ease-out 4400ms forwards; }
        .dqa-l-bug  { animation: dqa-fade-in 150ms ease-out 5200ms forwards; }

        .dqa-check { color: #00C781; opacity: 0; }
        .dqa-c1 { animation: dqa-fade-in 120ms ease-out 2400ms forwards; }
        .dqa-c2 { animation: dqa-fade-in 120ms ease-out 3000ms forwards; }
        .dqa-c3 { animation: dqa-fade-in 120ms ease-out 3600ms forwards; }


        .dqa-dim { color: #9BA4A6; opacity: 0.6; }
        .dqa-white { color: #F9F9F9; }
        .dqa-cyan { color: #C3FFFD; }
        .dqa-green { color: #00C781; }
        .dqa-green-bold { color: #00C781; font-weight: 700; }
        .dqa-yellow { color: #E3B341; }
        .dqa-yellow-bold { color: #E3B341; font-weight: 700; }

        .dqa-step-warn {
          background: rgba(227, 179, 65, 0.06);
          border-left: 2px solid #E3B341;
          padding-left: 8px;
          margin-left: -10px;
        }

        /* --- Slack panel (right) --- */
        .dqa-slack {
          width: 0;
          flex-shrink: 0;
          border-radius: 10px;
          overflow: hidden;
          box-shadow: 0 12px 40px rgba(0,0,0,0.5);
          opacity: 0;
          display: flex;
          flex-direction: column;
          font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
          background: #1C1C1C;
          animation: dqa-slack-expand 700ms cubic-bezier(0.16,1,0.3,1) 5200ms forwards;
        }

        @keyframes dqa-slack-expand {
          from { opacity: 0; width: 0; }
          to { opacity: 1; width: 60%; }
        }

        /* macOS title bar */
        .dqa-sl-titlebar {
          background: #2B2B2B;
          padding: 8px 14px;
          display: flex;
          align-items: center;
          flex-shrink: 0;
          position: relative;
          border-bottom: 1px solid #3A3A3A;
        }

        .dqa-sl-dots {
          display: flex;
          gap: 6px;
          flex-shrink: 0;
        }

        .dqa-sl-dot {
          width: 10px;
          height: 10px;
          border-radius: 50%;
          background: #4A4A4A;
        }

        .dqa-sl-titlebar-text {
          position: absolute;
          left: 50%;
          transform: translateX(-50%);
          font-size: 20px;
          color: #999;
          font-weight: 500;
        }

        /* Channel header */
        .dqa-sl-channel-header {
          padding: 14px 18px 12px;
          border-bottom: 1px solid #3A3A3A;
          display: flex;
          align-items: flex-start;
          justify-content: space-between;
          flex-shrink: 0;
        }

        .dqa-sl-channel-info {
          display: flex;
          flex-direction: column;
          gap: 2px;
        }

        .dqa-sl-channel-name {
          font-size: 24px;
          font-weight: 700;
          color: #F9F9F9;
          white-space: nowrap;
        }

        .dqa-sl-channel-members {
          font-size: 20px;
          color: #7C9A92;
          white-space: nowrap;
        }

        .dqa-sl-member-avatars {
          display: flex;
          align-items: center;
          flex-shrink: 0;
          padding-top: 2px;
        }

        .dqa-sl-member-av {
          width: 22px;
          height: 22px;
          border-radius: 50%;
          border: 2px solid #1C1C1C;
          margin-left: -6px;
        }

        .dqa-sl-member-av:first-child { margin-left: 0; }

        .dqa-sl-av1 {
          background: radial-gradient(circle at 50% 30%, #E8C4A0 30%, #C4543A 55%, #8B2E1A 100%);
        }
        .dqa-sl-av2 {
          background: radial-gradient(circle at 50% 32%, #D4B896 28%, #5C4033 52%, #2C2420 100%);
        }
        .dqa-sl-av3 {
          background: radial-gradient(circle at 50% 30%, #F0D5B8 30%, #3B5C78 55%, #1E3448 100%);
        }

        /* Messages area */
        .dqa-sl-messages {
          flex: 1;
          padding: 14px 18px;
          overflow: hidden;
        }

        .dqa-sl-msg {
          display: flex;
          gap: 10px;
          opacity: 0;
          animation: dqa-fade-in 200ms ease-out 5600ms forwards;
        }

        .dqa-sl-avatar {
          width: 32px;
          height: 32px;
          border-radius: 6px;
          background: #C3FFFD;
          display: flex;
          align-items: center;
          justify-content: center;
          flex-shrink: 0;
        }

        .dqa-sl-avatar-text {
          font-size: 22px;
          color: #1C1C1C;
          font-weight: 700;
          font-family: 'JetBrains Mono', monospace;
        }

        .dqa-sl-msg-body { flex: 1; min-width: 0; }

        .dqa-sl-msg-header {
          display: flex;
          align-items: baseline;
          gap: 6px;
          margin-bottom: 2px;
        }

        .dqa-sl-sender { font-size: 28px; font-weight: 700; color: #F9F9F9; }

        .dqa-sl-badge {
          font-size: 18px;
          color: #D1D2D3;
          background: rgba(255,255,255,0.08);
          padding: 1px 4px;
          border-radius: 2px;
          font-weight: 600;
          vertical-align: middle;
        }

        .dqa-sl-time { font-size: 18px; color: #616061; }

        .dqa-sl-attach {
          border-left: 3px solid #E3B341;
          background: rgba(227, 179, 65, 0.04);
          border-radius: 0 6px 6px 0;
          padding: 8px 12px;
          margin-top: 4px;
        }

        .dqa-sl-attach-title {
          font-size: 28px;
          font-weight: 700;
          color: #E3B341;
          margin-bottom: 6px;
          opacity: 0;
          animation: dqa-fade-in 100ms ease-out 5700ms forwards;
        }

        .dqa-sl-line { display: block; font-size: 26px; line-height: 1.8; opacity: 0; }
        .dqa-sl-key { color: #ABABAD; }
        .dqa-sl-val { color: #D1D2D3; }
        .dqa-sl-sev-high { color: #FF3B4D; font-weight: 700; }

        .dqa-sl-1 { animation: dqa-fade-in 80ms ease-out 5900ms forwards; }
        .dqa-sl-2 { animation: dqa-fade-in 80ms ease-out 6050ms forwards; }
        .dqa-sl-3 { animation: dqa-fade-in 80ms ease-out 6200ms forwards; }
        .dqa-sl-4 { animation: dqa-fade-in 80ms ease-out 6350ms forwards; }
        .dqa-sl-5 { animation: dqa-fade-in 80ms ease-out 6500ms forwards; }
        .dqa-sl-6 { animation: dqa-fade-in 80ms ease-out 6650ms forwards; }

        .dqa-sl-video {
          margin-top: 8px;
          border-radius: 6px;
          overflow: hidden;
          border: 1px solid #35373B;
          opacity: 0;
          animation: dqa-fade-in 200ms ease-out 7000ms forwards;
        }

        .dqa-sl-video-thumb {
          position: relative;
          background: #0D1117;
          aspect-ratio: 16 / 9;
          display: flex;
          align-items: center;
          justify-content: center;
        }

        .dqa-sl-video-scene {
          width: 45%;
          height: 50%;
          background: #F5F5F5;
          border-radius: 2px;
          opacity: 0.5;
          display: flex;
          flex-direction: column;
          overflow: hidden;
        }

        .dqa-sl-video-scene-bar {
          height: 20%;
          background: #E0E0E0;
        }

        .dqa-sl-video-scene-body {
          flex: 1;
          padding: 8%;
          display: flex;
          flex-direction: column;
          gap: 6%;
        }

        .dqa-sl-video-scene-line {
          height: 12%;
          width: 60%;
          background: #D0D0D0;
          border-radius: 1px;
        }

        .dqa-sl-video-play {
          position: absolute;
          width: 24px;
          height: 24px;
          border-radius: 50%;
          background: rgba(0,0,0,0.6);
          display: flex;
          align-items: center;
          justify-content: center;
          animation: dqa-play-hide 200ms ease-out 7400ms forwards;
        }

        @keyframes dqa-play-hide {
          to { opacity: 0; transform: scale(0.8); }
        }

        .dqa-sl-video-play-icon {
          width: 0;
          height: 0;
          border-style: solid;
          border-width: 4px 0 4px 7px;
          border-color: transparent transparent transparent #fff;
          margin-left: 2px;
        }

        .dqa-sl-vid-cursor {
          position: absolute;
          width: 8px;
          height: 10px;
          z-index: 3;
          pointer-events: none;
          filter: drop-shadow(0 1px 2px rgba(0,0,0,0.8));
          opacity: 0;
          animation:
            dqa-fade-in 100ms ease-out 7500ms forwards,
            dqa-vid-cursor-move 2000ms ease-in-out 7500ms forwards;
        }

        @keyframes dqa-vid-cursor-move {
          0%   { top: 35%; left: 30%; }
          25%  { top: 55%; left: 62%; }
          28%  { top: 56%; left: 62%; }
          30%  { top: 55%; left: 62%; }
          55%  { top: 55%; left: 62%; }
          58%  { top: 56%; left: 62%; }
          60%  { top: 55%; left: 62%; }
          80%  { top: 56%; left: 62%; }
          82%  { top: 55%; left: 62%; }
          100% { top: 55%; left: 62%; }
        }

        .dqa-sl-vid-click {
          position: absolute;
          top: 55%;
          left: 62%;
          width: 10px;
          height: 10px;
          border-radius: 50%;
          border: 1.5px solid rgba(0,0,0,0.6);
          opacity: 0;
          pointer-events: none;
          z-index: 2;
          transform: translate(-50%, -50%) scale(0);
        }

        .dqa-sl-vid-click-1 { animation: dqa-vid-click 400ms ease-out 8000ms forwards; }
        .dqa-sl-vid-click-2 { animation: dqa-vid-click 400ms ease-out 8500ms forwards; }
        .dqa-sl-vid-click-3 { animation: dqa-vid-click 400ms ease-out 8800ms forwards; }

        @keyframes dqa-vid-click {
          0% { opacity: 0.8; transform: translate(-50%, -50%) scale(0.3); }
          100% { opacity: 0; transform: translate(-50%, -50%) scale(2); }
        }

        .dqa-sl-vid-btn {
          position: absolute;
          top: 52%;
          left: 58%;
          width: 12%;
          height: 14%;
          background: #0969da;
          border-radius: 1px;
          z-index: 1;
          animation: dqa-vid-btn-fail 1200ms ease-out 8000ms forwards;
        }

        .dqa-sl-vid-warn {
          position: absolute;
          top: 40%;
          left: 45%;
          width: 18px;
          height: 18px;
          transform: translate(-50%, -50%) scale(0);
          z-index: 4;
          opacity: 0;
          pointer-events: none;
          filter: drop-shadow(0 1px 4px rgba(227, 179, 65, 0.6));
          animation: dqa-vid-warn-pop 400ms cubic-bezier(0.34, 1.56, 0.64, 1) 9100ms forwards;
        }

        @keyframes dqa-vid-warn-pop {
          0%   { opacity: 0; transform: translate(-50%, -50%) scale(0); }
          60%  { opacity: 1; transform: translate(-50%, -50%) scale(1.2); }
          100% { opacity: 1; transform: translate(-50%, -50%) scale(1); }
        }

        @keyframes dqa-vid-btn-fail {
          0%   { background: #0969da; }
          100% { background: #dc3545; }
        }

        .dqa-sl-video-bar {
          height: 3px;
          background: #222;
          position: relative;
        }

        .dqa-sl-video-progress {
          position: absolute;
          left: 0;
          top: 0;
          height: 100%;
          width: 0%;
          background: #E3B341;
          border-radius: 0 1px 1px 0;
          animation: dqa-vid-progress 3000ms linear 7400ms forwards;
        }

        @keyframes dqa-vid-progress {
          to { width: 75%; }
        }

        .dqa-sl-video-meta {
          background: #161B22;
          padding: 5px 8px;
          display: flex;
          align-items: center;
          gap: 5px;
        }

        .dqa-sl-video-icon {
          font-size: 18px;
          opacity: 0.6;
        }

        .dqa-sl-video-name {
          font-size: 18px;
          color: #ABABAD;
          flex: 1;
        }

        .dqa-sl-video-dur {
          font-size: 18px;
          color: #616061;
        }

        .dqa-sl-reactions {
          display: flex;
          gap: 5px;
          margin-top: 8px;
        }

        .dqa-sl-reaction {
          font-size: 18px;
          background: rgba(255,255,255,0.04);
          border: 1px solid #35373B;
          border-radius: 12px;
          padding: 2px 8px;
          color: #D1D2D3;
          opacity: 0;
          animation: dqa-fade-in 200ms ease-out 9000ms forwards;
        }

        .dqa-sl-thread {
          margin-top: 6px;
          display: flex;
          align-items: center;
          gap: 6px;
          opacity: 0;
          animation: dqa-fade-in 200ms ease-out 9200ms forwards;
        }

        .dqa-sl-thread-avatars {
          display: flex;
        }

        .dqa-sl-thread-av {
          width: 16px;
          height: 16px;
          border-radius: 4px;
          border: 1.5px solid #1C1C1C;
          margin-left: -4px;
        }

        .dqa-sl-thread-av:first-child { margin-left: 0; }

        /* --- Tagline --- */
        .dqa-tagline {
          position: absolute;
          bottom: 16px;
          left: 50%;
          transform: translateX(-50%);
          font-size: 24px;
          color: #9BA4A6;
          opacity: 0;
          animation: dqa-fade-in 500ms ease-out 9800ms forwards;
          white-space: nowrap;
        }

        .dqa-tagline-em { color: #C3FFFD; font-weight: 700; }

        /* --- Keyframes --- */
        @keyframes dqa-fade-in {
          from { opacity: 0; }
          to { opacity: 1; }
        }

        @keyframes dqa-fade-out {
          from { opacity: 1; }
          to { opacity: 0; }
        }

        @media (prefers-reduced-motion: reduce) {
          .dqa-terminal, .dqa-desktop, .dqa-slack, .dqa-tagline,
          .dqa-line, .dqa-check,
          .dqa-sl-msg, .dqa-sl-attach-title,
          .dqa-sl-line,
          .dqa-sl-reaction, .dqa-sl-thread,
          .dqa-mini-page-home, .dqa-mini-page-todos, .dqa-mini-tab-click,
          .dqa-mini-input, .dqa-mini-item-expected, .dqa-mini-typed {
            animation: none !important;
            opacity: 1 !important;
          }
          .dqa-cmd {
            animation: none !important;
            width: auto !important;
          }
          .dqa-ptr, .dqa-flash, .dqa-click-ring {
            animation: none !important;
            opacity: 0 !important;
          }
          .dqa-left {
            animation: none !important;
            width: 0 !important;
            opacity: 0 !important;
          }
          .dqa-slack {
            animation: none !important;
            opacity: 1 !important;
            width: 60% !important;
          }
        }
      `}</style>

      <div className="dqa-heading">
        <span className="dqa-heading-em">Autonomous</span> bug detection and reporting
      </div>

      <div className="dqa-main">
        {/* Desktop panel */}
        <div className="dqa-left">
          <div className="dqa-desktop">
            <div className="dqa-desk-bar" />
            <div className="dqa-desk-body">
              <div className="dqa-mini-app">
                <div className="dqa-mini-app-bar">
                  <div className="dqa-mini-dot" style={{ background: "#FF3B4D" }} />
                  <div className="dqa-mini-dot" style={{ background: "#E3B341" }} />
                  <div className="dqa-mini-dot" style={{ background: "#00C781" }} />
                </div>
                <div className="dqa-mini-app-body">
                  <div className="dqa-mini-nav">
                    <div className="dqa-mini-tab">Home</div>
                    <div className="dqa-mini-tab dqa-mini-tab-click">Todos</div>
                    <div className="dqa-mini-tab">Settings</div>
                  </div>
                  <div className="dqa-mini-page-home">
                    <div className="dqa-mini-placeholder" style={{ width: "60%" }} />
                    <div className="dqa-mini-placeholder" style={{ width: "45%" }} />
                    <div className="dqa-mini-placeholder" style={{ width: "50%" }} />
                  </div>
                  <div className="dqa-mini-page-todos">
                    <div className="dqa-mini-row">
                      <div className="dqa-mini-input">
                        <span className="dqa-mini-typed">Buy groceries</span>
                      </div>
                      <div className="dqa-mini-btn" />
                    </div>
                    <div className="dqa-mini-item dqa-mini-item-existing" />
                    <div className="dqa-mini-item dqa-mini-item-expected" />
                  </div>
                </div>
              </div>
              <svg className="dqa-ptr" viewBox="0 0 24 24" fill="none">
                <path d="M5 3l14 8-6 2-4 6-4-16z" fill="#fff" stroke="#000" strokeWidth="1.5" strokeLinejoin="round" />
              </svg>
              <svg className="dqa-warn-icon" viewBox="0 0 24 24" fill="none">
                <path d="M12 2L1 21h22L12 2z" fill="#E3B341" stroke="#000" strokeWidth="0.5" strokeLinejoin="round" />
                <text x="12" y="18" textAnchor="middle" fontSize="13" fontWeight="900" fill="#000" fontFamily="sans-serif">!</text>
              </svg>
              <div className="dqa-flash" />
              <div className="dqa-click-ring dqa-click-1" />
              <div className="dqa-click-ring dqa-click-2" />
              <div className="dqa-click-ring dqa-click-3" />
              <div className="dqa-click-ring dqa-click-4" />
              <div className="dqa-click-ring dqa-click-5" />
              <div className="dqa-click-ring dqa-click-6" />
            </div>
          </div>
        </div>

        {/* Terminal */}
        <div className="dqa-terminal">
          <div className="dqa-titlebar">
            <div className="dqa-dot" style={{ background: "#FF3B4D" }} />
            <div className="dqa-dot" style={{ background: "#E3B341" }} />
            <div className="dqa-dot" style={{ background: "#00C781" }} />
          </div>
          <div className="dqa-body">
            <span style={{ display: "block" }}>
              <span className="dqa-prompt">$ </span>
              <span className="dqa-cmd">{CMD}</span>
            </span>
            <span className="dqa-line dqa-l-qa">
              <span className="dqa-yellow">  QA mode enabled</span>
              <span className="dqa-dim">{" — watching for issues..."}</span>
            </span>
            <span className="dqa-line dqa-l-gap1">&nbsp;</span>

            <span className="dqa-line dqa-l-s1">
              <span className="dqa-dim">{"  Step 1/5 — Navigate to /todos      "}</span>
              <span className="dqa-check dqa-c1">{"✓"}</span>
            </span>
            <span className="dqa-line dqa-l-s2">
              <span className="dqa-dim">{'  Step 2/5 — Add todo "Buy groceries"'}</span>
              <span className="dqa-check dqa-c2">{" ✓"}</span>
            </span>
            <span className="dqa-line dqa-l-s3">
              <span className="dqa-dim">{'  Step 3/5 — Click "Add Todo" button '}</span>
              <span className="dqa-check dqa-c3">{"✓"}</span>
            </span>
            <span className="dqa-line dqa-l-s4">
              <span className="dqa-dim">{"  Step 4/5 — Verify todo was added"}</span>
            </span>

            <span className="dqa-line dqa-l-warn">
              <span className="dqa-step-warn" style={{ display: "inline-block", padding: "2px 8px" }}>
                <span className="dqa-yellow-bold">{"⚠ "}</span>
                <span className="dqa-yellow">{"Button click had no effect"}</span>
                <span className="dqa-dim">{" — no UI change detected"}</span>
              </span>
            </span>
            <span className="dqa-line dqa-l-bug">
              <span className="dqa-step-warn" style={{ display: "inline-block", padding: "2px 8px" }}>
                <span className="dqa-white">{"🐛 "}</span>
                <span className="dqa-yellow-bold">{"BUG filed"}</span>
                <span className="dqa-dim">{" → Slack #qa-alerts"}</span>
              </span>
            </span>

          </div>
        </div>

        {/* Slack panel */}
        <div className="dqa-slack">
          {/* macOS title bar */}
          <div className="dqa-sl-titlebar">
            <div className="dqa-sl-dots">
              <div className="dqa-sl-dot" />
              <div className="dqa-sl-dot" />
              <div className="dqa-sl-dot" />
            </div>
            <span className="dqa-sl-titlebar-text">Slack</span>
          </div>

          {/* Channel header */}
          <div className="dqa-sl-channel-header">
            <div className="dqa-sl-channel-info">
              <span className="dqa-sl-channel-name">#qa-alerts</span>
              <span className="dqa-sl-channel-members">3 members</span>
            </div>
            <div className="dqa-sl-member-avatars">
              <div className="dqa-sl-member-av dqa-sl-av1" />
              <div className="dqa-sl-member-av dqa-sl-av2" />
              <div className="dqa-sl-member-av dqa-sl-av3" />
            </div>
          </div>

          {/* Messages */}
          <div className="dqa-sl-messages">
            <div className="dqa-sl-msg">
              <div className="dqa-sl-avatar">
                <span className="dqa-sl-avatar-text">{">_"}</span>
              </div>
              <div className="dqa-sl-msg-body">
                <div className="dqa-sl-msg-header">
                  <span className="dqa-sl-sender">Desktest</span>
                  <span className="dqa-sl-badge">APP</span>
                  <span className="dqa-sl-time">5m</span>
                </div>
                <div className="dqa-sl-attach">
                  <div className="dqa-sl-attach-title">{"🐛 Bug Detected"}</div>
                  <span className="dqa-sl-line dqa-sl-1">
                    <span className="dqa-sl-key">Title: </span>
                    <span className="dqa-sl-val">"Add Todo" button fails silently</span>
                  </span>
                  <span className="dqa-sl-line dqa-sl-2">
                    <span className="dqa-sl-key">Severity: </span>
                    <span className="dqa-sl-sev-high">High</span>
                  </span>
                  <span className="dqa-sl-line dqa-sl-3">
                    <span className="dqa-sl-key">Category: </span>
                    <span className="dqa-sl-val">Functionality</span>
                  </span>
                  <span className="dqa-sl-line dqa-sl-4">&nbsp;</span>
                  <span className="dqa-sl-line dqa-sl-5">
                    <span className="dqa-sl-key">Observed: </span>
                    <span className="dqa-sl-val">Click did not create todo item</span>
                  </span>
                  <span className="dqa-sl-line dqa-sl-6">
                    <span className="dqa-sl-key">Expected: </span>
                    <span className="dqa-sl-val">New todo appears in list</span>
                  </span>
                  <div className="dqa-sl-video">
                    <div className="dqa-sl-video-thumb">
                      <div className="dqa-sl-video-scene">
                        <div className="dqa-sl-video-scene-bar" />
                        <div className="dqa-sl-video-scene-body">
                          <div className="dqa-sl-video-scene-line" />
                          <div className="dqa-sl-video-scene-line" style={{ width: "40%" }} />
                        </div>
                      </div>
                      <div className="dqa-sl-vid-btn" />
                      <svg className="dqa-sl-vid-cursor" viewBox="0 0 24 24" fill="none">
                        <path d="M5 3l14 8-6 2-4 6-4-16z" fill="#fff" stroke="#000" strokeWidth="1.5" strokeLinejoin="round" />
                      </svg>
                      <div className="dqa-sl-vid-click dqa-sl-vid-click-1" />
                      <div className="dqa-sl-vid-click dqa-sl-vid-click-2" />
                      <div className="dqa-sl-vid-click dqa-sl-vid-click-3" />
                      <svg className="dqa-sl-vid-warn" viewBox="0 0 24 24" fill="none">
                        <path d="M12 2L1 21h22L12 2z" fill="#E3B341" stroke="#000" strokeWidth="0.5" strokeLinejoin="round" />
                        <text x="12" y="18" textAnchor="middle" fontSize="13" fontWeight="900" fill="#000" fontFamily="sans-serif">!</text>
                      </svg>
                      <div className="dqa-sl-video-play">
                        <div className="dqa-sl-video-play-icon" />
                      </div>
                    </div>
                    <div className="dqa-sl-video-bar">
                      <div className="dqa-sl-video-progress" />
                    </div>
                    <div className="dqa-sl-video-meta">
                      <span className="dqa-sl-video-icon">▶</span>
                      <span className="dqa-sl-video-name">recording_step3-4.mp4</span>
                      <span className="dqa-sl-video-dur">0:12</span>
                    </div>
                  </div>
                </div>
                <div className="dqa-sl-reactions">
                  <span className="dqa-sl-reaction">{"👀 1"}</span>
                </div>
                <div className="dqa-sl-thread">
                  <div className="dqa-sl-thread-avatars">
                    <div className="dqa-sl-thread-av dqa-sl-av2" />
                  </div>
                  <span style={{ fontSize: "18px", color: "#1264A3", fontWeight: 600 }}>1 reply</span>
                  <span style={{ fontSize: "18px", color: "#616061" }}>5m</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="dqa-tagline">
        <span>Test + QA in one pass — </span>
        <span className="dqa-tagline-em">bugs auto-filed to Slack</span>
      </div>
    </div>
  );
}
