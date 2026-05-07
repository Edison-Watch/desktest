import React, { useState, useEffect } from "react";

const CYCLE_MS = 18000;
const CMD_0 = "desktest run task.json";
const CMD_1 = "desktest codify trajectory.jsonl";
const CMD_2 = "desktest run task.json --replay";

export default function DesktestCodifyAnimation() {
  const [cycle, setCycle] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setCycle((c) => c + 1), CYCLE_MS);
    return () => clearInterval(id);
  }, []);

  const desktopInner = (
    <>
      <div className="dc-desk-bar" />
      <div className="dc-desk-body">
        <div className="dc-mini-app">
          <div className="dc-mini-app-bar">
            <div className="dc-mini-app-dot" style={{ background: "#FF3B4D" }} />
            <div className="dc-mini-app-dot" style={{ background: "#E3B341" }} />
            <div className="dc-mini-app-dot" style={{ background: "#00C781" }} />
          </div>
          <div className="dc-mini-app-body">
            <div className="dc-mock-header" />
            <div className="dc-mini-row">
              <div className="dc-mini-input">
                <span className="dc-mini-typed">Buy groceries</span>
              </div>
              <div className="dc-mini-btn" />
            </div>
            <div className="dc-mini-item dc-mini-item-existing" />
            <div className="dc-mini-item dc-mini-item-new" />
          </div>
        </div>
        <svg className="dc-ptr" viewBox="0 0 24 24" fill="none">
          <path d="M5 3l14 8-6 2-4 6-4-16z" fill="#fff" stroke="#000" strokeWidth="1.5" strokeLinejoin="round" />
        </svg>
        <div className="dc-flash" />
        <div className="dc-click-ring dc-click-1" />
        <div className="dc-click-ring dc-click-2" />
      </div>
    </>
  );

  return (
    <div className="dc-scene" key={cycle}>
      <style>{`
        .dc-scene {
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

        /* --- Title --- */
        .dc-title {
          font-size: 64px;
          color: #F9F9F9;
          font-weight: 700;
          margin-bottom: 6px;
          opacity: 0;
          animation: dc-fade-in 500ms ease-out 100ms forwards;
          flex-shrink: 0;
        }

        .dc-title-accent { color: #C3FFFD; }

        /* --- Phase indicator --- */
        .dc-phase {
          display: flex;
          align-items: center;
          gap: 20px;
          font-size: 26px;
          margin-bottom: 16px;
          flex-shrink: 0;
        }

        .dc-phase-step {
          display: flex;
          align-items: center;
          gap: 8px;
          color: #9BA4A6;
          opacity: 0.3;
        }

        .dc-phase-num {
          width: 30px;
          height: 30px;
          border-radius: 50%;
          border: 1.5px solid currentColor;
          display: flex;
          align-items: center;
          justify-content: center;
          font-size: 24px;
          font-weight: 700;
        }

        .dc-phase-arrow { color: #383838; font-size: 28px; }

        .dc-phase-1 { animation: dc-activate 200ms ease-out 200ms forwards, dc-done 200ms ease-out 3400ms forwards; }
        .dc-phase-2 { animation: dc-activate 200ms ease-out 3900ms forwards, dc-done 200ms ease-out 7400ms forwards; }
        .dc-phase-3 { animation: dc-activate 200ms ease-out 7700ms forwards, dc-done 200ms ease-out 11800ms forwards; }
        .dc-phase-4 { animation: dc-activate 200ms ease-out 12000ms forwards, dc-done 200ms ease-out 15600ms forwards; }

        @keyframes dc-activate { to { opacity: 1; color: #C3FFFD; } }
        @keyframes dc-done { to { opacity: 1; color: #00C781; } }

        /* --- Main layout --- */
        .dc-main {
          display: flex;
          flex: 1;
          width: 98%;
          max-width: 1800px;
          min-height: 0;
          animation: dc-fade-out 400ms ease-out 12000ms forwards;
        }

        /* --- Left panel --- */
        .dc-left {
          width: 28%;
          margin-right: 14px;
          position: relative;
          flex-shrink: 0;
          overflow: hidden;
          animation: dc-collapse-left 400ms ease-out 7500ms forwards;
        }

        @keyframes dc-collapse-left {
          to { width: 0; margin-right: 0; }
        }

        /* --- Task.json card (Phase 1 input, top of left panel) --- */
        .dc-task-card {
          position: absolute;
          left: 0;
          right: 0;
          top: 0;
          height: 40%;
          border-radius: 10px;
          border: 1px solid #D0D0D0;
          overflow: hidden;
          box-shadow: 0 8px 24px rgba(0,0,0,0.4);
          display: flex;
          flex-direction: column;
          opacity: 0;
          animation:
            dc-fade-in 400ms ease-out 400ms forwards,
            dc-fade-out 400ms ease-out 3200ms forwards;
        }

        /* --- Arrow from task.json → desktop explore --- */
        .dc-task-arrow {
          position: absolute;
          top: 40%;
          left: 50%;
          transform: translateX(-50%);
          z-index: 10;
          pointer-events: none;
          display: flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          height: 14%;
          opacity: 0;
          animation:
            dc-fade-in 150ms ease-out 800ms forwards,
            dc-fade-out 300ms ease-out 3200ms forwards;
        }

        .dc-task-arrow-line {
          fill: none;
          stroke: #C3FFFD;
          stroke-width: 2;
          stroke-dasharray: 30;
          stroke-dashoffset: 30;
          animation: dc-convert-draw 300ms ease-out 800ms forwards;
        }

        .dc-task-arrow-head {
          fill: #C3FFFD;
          opacity: 0;
          animation: dc-fade-in 100ms ease-out 1050ms forwards;
        }

        .dc-task-titlebar {
          background: #E8E8E8;
          border-bottom: 1px solid #D0D0D0;
          padding: 6px 10px;
          display: flex;
          align-items: center;
          gap: 5px;
          flex-shrink: 0;
        }

        .dc-task-filename { color: #555; font-size: 26px; margin-left: 4px; }

        .dc-task-body {
          background: #F5F5F5;
          padding: 12px 16px;
          font-size: 26px;
          line-height: 1.8;
          flex: 1;
          overflow: hidden;
        }

        .dc-task-brace { color: #333; }
        .dc-task-key { color: #0969da; font-weight: 700; }
        .dc-task-value { color: #1a1a1a; word-break: break-word; white-space: normal; }
        .dc-task-dim { color: #999; opacity: 0.5; }
        .dc-task-indent { display: inline-block; width: 1.5ch; }
        .dc-task-line { display: block; }

        /* --- Trajectory card (Phase 2 input) --- */
        .dc-traj-card {
          position: absolute;
          left: 0;
          right: 0;
          bottom: 0;
          height: 43%;
          border-radius: 10px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow: 0 12px 40px rgba(0,0,0,0.5);
          opacity: 0;
          display: flex;
          flex-direction: column;
          transform-origin: top center;
          animation: dc-pop-down 400ms ease-out 3900ms forwards, dc-fade-out 400ms ease-out 7500ms forwards;
        }

        .dc-traj-titlebar {
          background: #2F3440;
          border-bottom: 1px solid #383838;
          padding: 7px 10px;
          display: flex;
          align-items: center;
          gap: 5px;
          flex-shrink: 0;
        }

        .dc-traj-filename { color: #9BA4A6; font-size: 26px; margin-left: 4px; }

        .dc-traj-body {
          background: #1C1C1C;
          padding: 14px 16px;
          font-size: 26px;
          line-height: 1.9;
          flex: 1;
          overflow: hidden;
        }

        .dc-traj-line { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; opacity: 0; }
        .dc-tl-1 { animation: dc-fade-in 80ms ease-out 4400ms forwards; }
        .dc-tl-2 { animation: dc-fade-in 80ms ease-out 4600ms forwards; }
        .dc-tl-3 { animation: dc-fade-in 80ms ease-out 4800ms forwards; }
        .dc-tl-4 { animation: dc-fade-in 80ms ease-out 5000ms forwards; }
        .dc-tl-5 { animation: dc-fade-in 80ms ease-out 5200ms forwards; }
        .dc-tl-6 { animation: dc-fade-in 80ms ease-out 5400ms forwards; }
        .dc-tl-7 { animation: dc-fade-in 80ms ease-out 5600ms forwards; }

        .dc-traj-key { color: #C3FFFD; }
        .dc-traj-str { color: #E3B341; }
        .dc-traj-num { color: #00C781; }
        .dc-traj-brace { color: #555; }
        .dc-traj-dim { color: #666; }

        /* --- Mini desktop (shared base) --- */
        .dc-desktop {
          position: absolute;
          top: 0;
          left: 0;
          right: 0;
          bottom: 0;
          border-radius: 10px;
          border: 1px solid #383838;
          overflow: hidden;
          opacity: 0;
          display: flex;
          flex-direction: column;
        }

        .dc-desk-bar {
          background: #2F3440;
          height: 14px;
          border-bottom: 1px solid #383838;
          flex-shrink: 0;
        }

        .dc-desk-body {
          background: #1a1a2e;
          flex: 1;
          position: relative;
          display: flex;
          align-items: center;
          justify-content: center;
        }

        .dc-mini-app {
          width: 75%;
          height: 65%;
          background: #F5F5F5;
          border-radius: 3px;
          overflow: hidden;
          display: flex;
          flex-direction: column;
        }

        .dc-mini-app-bar {
          background: #E0E0E0;
          height: 10px;
          display: flex;
          align-items: center;
          padding: 0 4px;
          gap: 2px;
          flex-shrink: 0;
        }

        .dc-mini-app-dot { width: 3px; height: 3px; border-radius: 50%; }

        .dc-mini-app-body {
          padding: 6px;
          flex: 1;
          display: flex;
          flex-direction: column;
          gap: 3px;
        }

        .dc-mock-header {
          height: 8px;
          width: 50%;
          background: #e0e0e0;
          border-radius: 2px;
        }

        .dc-mini-row { display: flex; gap: 3px; align-items: center; }

        .dc-mini-input {
          flex: 1;
          height: 10px;
          background: #fff;
          border: 1px solid #ddd;
          border-radius: 2px;
          display: flex;
          align-items: center;
          padding: 0 3px;
          overflow: hidden;
          transition: border-color 150ms, box-shadow 150ms;
        }

        .dc-mini-typed {
          font-size: 5px;
          font-family: inherit;
          color: #333;
          white-space: nowrap;
          opacity: 0;
        }

        .dc-mini-btn {
          width: 16px;
          height: 10px;
          background: #ccc;
          border-radius: 2px;
        }

        @keyframes dc-btn-click {
          0% { background: #ccc; transform: scale(1); }
          30% { background: #0969da; transform: scale(0.85); }
          60% { background: #0969da; transform: scale(1.05); }
          100% { background: #0969da; transform: scale(1); }
        }

        @keyframes dc-input-focus {
          to { border-color: #0969da; box-shadow: 0 0 0 1px rgba(9,105,218,0.3); }
        }

        .dc-mini-item { height: 8px; border-radius: 2px; }
        .dc-mini-item-existing { width: 55%; background: #e0e0e0; opacity: 0; }
        .dc-mini-item-new { width: 62%; background: #d4edda; opacity: 0; }

        .dc-ptr {
          position: absolute;
          width: 14px;
          height: 17px;
          z-index: 4;
          pointer-events: none;
          filter: drop-shadow(0 1px 3px rgba(0,0,0,0.9));
          top: 25%;
          left: 20%;
          opacity: 0;
        }

        .dc-flash {
          position: absolute;
          inset: 0;
          border: 2px solid #C3FFFD;
          border-radius: 10px;
          opacity: 0;
          pointer-events: none;
          z-index: 3;
        }

        @keyframes dc-flash-pulse {
          0% { opacity: 0.8; }
          100% { opacity: 0; }
        }

        .dc-click-ring {
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

        .dc-click-1 { top: 42%; left: 38%; }
        .dc-click-2 { top: 42%; left: 78%; }

        @keyframes dc-click-pulse {
          0% { opacity: 1; transform: translate(-50%, -50%) scale(0.3); }
          100% { opacity: 0; transform: translate(-50%, -50%) scale(2.5); }
        }

        /* --- Phase 1: Explore desktop (starts at bottom, slides to top) --- */
        .dc-desktop-explore {
          top: 56%;
          bottom: 0;
          transform-origin: right center;
          animation:
            dc-pop-out-left 700ms cubic-bezier(0.16, 1, 0.3, 1) 1300ms forwards,
            dc-slide-to-top 600ms ease-in-out 3400ms forwards,
            dc-fade-out 400ms ease-out 7200ms forwards;
        }
        .dc-desktop-explore .dc-mini-item-existing { animation: dc-fade-in 200ms ease-out 1500ms forwards; }
        .dc-desktop-explore .dc-ptr { animation: dc-fade-in 200ms ease-out 1600ms forwards, dc-ptr-explore 1800ms ease-in-out 1700ms forwards; }
        .dc-desktop-explore .dc-click-1 { animation: dc-click-pulse 600ms ease-out 2000ms forwards; }
        .dc-desktop-explore .dc-mini-input { animation: dc-input-focus 150ms ease-out 2000ms forwards; }
        .dc-desktop-explore .dc-mini-typed { animation: dc-fade-in 200ms ease-out 2400ms forwards; }
        .dc-desktop-explore .dc-click-2 { animation: dc-click-pulse 600ms ease-out 3000ms forwards; }
        .dc-desktop-explore .dc-mini-btn { animation: dc-btn-click 300ms ease-out 3000ms forwards; }
        .dc-desktop-explore .dc-mini-item-new { animation: dc-fade-in 200ms ease-out 3200ms forwards; }
        .dc-desktop-explore .dc-flash { animation: dc-flash-pulse 400ms ease-out 3300ms forwards; }

        @keyframes dc-ptr-explore {
          0%   { top: 25%; left: 20%; }
          15%  { top: 42%; left: 38%; }
          20%  { top: 43.5%; left: 38%; }
          25%  { top: 42%; left: 38%; }
          50%  { top: 42%; left: 38%; }
          65%  { top: 42%; left: 78%; }
          72%  { top: 43.5%; left: 78%; }
          78%  { top: 42%; left: 78%; }
          100% { top: 58%; left: 45%; }
        }

        /* --- Replay desktop wrapper (right of code card) --- */
        .dc-replay-wrap {
          width: 0;
          margin-left: 0;
          flex-shrink: 0;
          position: relative;
          overflow: hidden;
          animation: dc-expand-replay 400ms ease-out 9000ms forwards;
        }

        @keyframes dc-expand-replay {
          to { width: 28%; margin-left: 14px; }
        }

        /* --- Phase 3: Replay desktop (spawned by CMD_2, associated with replay.py) --- */
        .dc-desktop-replay {
          transform-origin: left center;
          animation: dc-pop-out-right 700ms cubic-bezier(0.16, 1, 0.3, 1) 9200ms forwards;
        }
        .dc-desktop-replay .dc-mini-item-existing { animation: dc-fade-in 200ms ease-out 9300ms forwards; }
        .dc-desktop-replay .dc-ptr { animation: dc-fade-in 200ms ease-out 9400ms forwards, dc-ptr-replay 2000ms ease-in-out 9500ms forwards; }
        .dc-desktop-replay .dc-click-1 { animation: dc-click-pulse 600ms ease-out 9700ms forwards; }
        .dc-desktop-replay .dc-mini-input { animation: dc-input-focus 150ms ease-out 9700ms forwards; }
        .dc-desktop-replay .dc-mini-typed { animation: dc-fade-in 200ms ease-out 10000ms forwards; }
        .dc-desktop-replay .dc-click-2 { animation: dc-click-pulse 600ms ease-out 10800ms forwards; }
        .dc-desktop-replay .dc-mini-btn { animation: dc-btn-click 300ms ease-out 10800ms forwards; }
        .dc-desktop-replay .dc-mini-item-new { animation: dc-fade-in 200ms ease-out 11000ms forwards; }
        .dc-desktop-replay .dc-flash { animation: dc-flash-pulse 400ms ease-out 11100ms forwards; }

        @keyframes dc-ptr-replay {
          0%   { top: 25%; left: 20%; }
          12%  { top: 42%; left: 38%; }
          16%  { top: 43.5%; left: 38%; }
          20%  { top: 42%; left: 38%; }
          50%  { top: 42%; left: 38%; }
          64%  { top: 42%; left: 78%; }
          70%  { top: 43.5%; left: 78%; }
          75%  { top: 42%; left: 78%; }
          100% { top: 58%; left: 45%; }
        }

        /* --- Slide desktop from bottom to top half --- */
        @keyframes dc-slide-to-top {
          from { top: 56%; bottom: 0; }
          to   { top: 0; bottom: 57%; }
        }

        /* --- Conversion arrow (desktop ↓ trajectory.jsonl) --- */
        .dc-convert {
          position: absolute;
          top: 43%;
          left: 50%;
          transform: translateX(-50%);
          z-index: 10;
          pointer-events: none;
          display: flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          height: 14%;
          opacity: 0;
          animation: dc-fade-in 150ms ease-out 3500ms forwards, dc-fade-out 400ms ease-out 7200ms forwards;
        }

        .dc-convert-line {
          fill: none;
          stroke: #C3FFFD;
          stroke-width: 2;
          stroke-dasharray: 30;
          stroke-dashoffset: 30;
          animation: dc-convert-draw 300ms ease-out 3500ms forwards;
        }

        .dc-convert-head {
          fill: #C3FFFD;
          opacity: 0;
          animation: dc-fade-in 100ms ease-out 3750ms forwards;
        }

        @keyframes dc-convert-draw {
          to { stroke-dashoffset: 0; }
        }

        /* --- Pop-down (trajectory enters from arrow direction) --- */
        @keyframes dc-pop-down {
          from { opacity: 0; transform: translateY(-10px) scale(0.9); }
          to { opacity: 1; transform: translateY(0) scale(1); }
        }

        /* --- Pop-out animation (scale up from terminal edge + cyan glow) --- */
        @keyframes dc-pop-out-left {
          0% {
            opacity: 0;
            transform: scale(0.05) translateX(20px);
            box-shadow: 0 0 40px rgba(195, 255, 253, 0.8);
          }
          20% {
            opacity: 1;
            transform: scale(0.2) translateX(10px);
            box-shadow: 0 0 24px rgba(195, 255, 253, 0.5);
          }
          100% {
            opacity: 1;
            transform: scale(1) translateX(0);
            box-shadow: 0 12px 40px rgba(0,0,0,0.5);
          }
        }

        @keyframes dc-pop-out-right {
          0% {
            opacity: 0;
            transform: scale(0.05) translateX(-20px);
            box-shadow: 0 0 40px rgba(195, 255, 253, 0.8);
          }
          20% {
            opacity: 1;
            transform: scale(0.2) translateX(-10px);
            box-shadow: 0 0 24px rgba(195, 255, 253, 0.5);
          }
          100% {
            opacity: 1;
            transform: scale(1) translateX(0);
            box-shadow: 0 12px 40px rgba(0,0,0,0.4);
          }
        }

        /* --- Terminal --- */
        .dc-terminal {
          flex: 1;
          min-width: 0;
          border-radius: 12px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow: 0 20px 60px rgba(0,0,0,0.8), 0 8px 24px rgba(0,0,0,0.5);
          opacity: 0;
          animation: dc-fade-in 300ms ease-out 200ms forwards;
          display: flex;
          flex-direction: column;
        }

        .dc-titlebar {
          background: #2F3440;
          border-bottom: 1px solid #383838;
          padding: 10px 14px;
          display: flex;
          gap: 7px;
          align-items: center;
          flex-shrink: 0;
        }

        .dc-dot { width: 10px; height: 10px; border-radius: 50%; }

        .dc-body {
          background: #1C1C1C;
          padding: 18px 22px;
          font-size: 30px;
          line-height: 1.9;
          flex: 1;
          overflow: hidden;
        }

        .dc-prompt { color: #C3FFFD; font-weight: 700; }

        .dc-cmd-0 {
          color: #F9F9F9;
          overflow: hidden;
          white-space: nowrap;
          display: inline-block;
          vertical-align: bottom;
          width: 0;
          animation: dc-type-0 ${CMD_0.length * 25}ms steps(${CMD_0.length}) 400ms forwards;
        }

        .dc-cmd-1 {
          color: #F9F9F9;
          overflow: hidden;
          white-space: nowrap;
          display: inline-block;
          vertical-align: bottom;
          width: 0;
          animation: dc-type-1 ${CMD_1.length * 25}ms steps(${CMD_1.length}) 4200ms forwards;
        }

        .dc-cmd-2 {
          color: #F9F9F9;
          overflow: hidden;
          white-space: nowrap;
          display: inline-block;
          vertical-align: bottom;
          width: 0;
          animation: dc-type-2 ${CMD_2.length * 25}ms steps(${CMD_2.length}) 8000ms forwards;
        }

        .dc-out { display: block; opacity: 0; }

        /* Launch lines — cyan with brief glow */
        @keyframes dc-launch-flash {
          0% { opacity: 0; }
          30% { opacity: 1; text-shadow: 0 0 10px rgba(195, 255, 253, 0.6); }
          100% { opacity: 1; text-shadow: none; }
        }

        .dc-launch { display: block; opacity: 0; }
        .dc-launch-1 { animation: dc-launch-flash 800ms ease-out 1000ms forwards; }
        .dc-launch-2 { animation: dc-launch-flash 800ms ease-out 6400ms forwards; }
        .dc-launch-3 { animation: dc-launch-flash 800ms ease-out 8900ms forwards; }

        /* Phase 1 outputs */
        .dc-out-e1 { animation: dc-fade-in 150ms ease-out 1800ms forwards; }
        .dc-out-e2 { animation: dc-fade-in 150ms ease-out 3200ms forwards; }

        /* Phase 2 section */
        .dc-gap { display: block; opacity: 0; }
        .dc-gap-1 { animation: dc-fade-in 100ms ease-out 3600ms forwards; }
        .dc-section-2 { display: block; opacity: 0; animation: dc-fade-in 200ms ease-out 4000ms forwards; }

        .dc-out-c1 { animation: dc-fade-in 150ms ease-out 5200ms forwards; }
        .dc-out-c2 { animation: dc-fade-in 150ms ease-out 5600ms forwards; }
        .dc-out-c3 { animation: dc-fade-in 150ms ease-out 7000ms forwards; }
        .dc-out-c4 { animation: dc-fade-in 150ms ease-out 7200ms forwards; }

        /* Phase 3 section */
        .dc-gap-2 { animation: dc-fade-in 100ms ease-out 7600ms forwards; }
        .dc-section-3 { display: block; opacity: 0; animation: dc-fade-in 200ms ease-out 7800ms forwards; }

        .dc-out-r1 { animation: dc-fade-in 150ms ease-out 11200ms forwards; }
        .dc-out-r2 { animation: dc-fade-in 150ms ease-out 11600ms forwards; }

        .dc-dim { color: #9BA4A6; opacity: 0.6; }
        .dc-white { color: #F9F9F9; }
        .dc-cyan { color: #C3FFFD; }
        .dc-green { color: #00C781; }
        .dc-green-bold { color: #00C781; font-weight: 700; }

        /* --- Right panel: replay.py (spawned by CMD_1) --- */
        .dc-code-card {
          width: 0;
          margin-left: 0;
          flex-shrink: 0;
          border-radius: 10px;
          border: 1px solid #D0D0D0;
          overflow: hidden;
          opacity: 0;
          transform-origin: left center;
          animation: dc-expand-code 400ms ease-out 6600ms forwards, dc-pop-out-right 700ms cubic-bezier(0.16, 1, 0.3, 1) 6700ms forwards;
          display: flex;
          flex-direction: column;
        }

        @keyframes dc-expand-code {
          to { width: 32%; margin-left: 14px; }
        }

        .dc-code-titlebar {
          background: #E8E8E8;
          border-bottom: 1px solid #D0D0D0;
          padding: 7px 10px;
          display: flex;
          align-items: center;
          gap: 5px;
          flex-shrink: 0;
        }

        .dc-code-dot { width: 7px; height: 7px; border-radius: 50%; }
        .dc-code-filename { color: #555; font-size: 26px; margin-left: 4px; }

        .dc-code-body {
          background: #F5F5F5;
          padding: 14px 16px;
          font-size: 26px;
          line-height: 1.8;
          color: #666;
          flex: 1;
          overflow: hidden;
        }

        .dc-code-comment { color: #999; }
        .dc-code-fn { color: #0969da; }
        .dc-code-str { color: #b35900; }
        .dc-code-num { color: #0a7d50; }

        .dc-cl {
          display: block;
          opacity: 0;
          padding: 1px 4px;
          border-left: 2px solid transparent;
          margin-left: -6px;
        }

        .dc-cl-1 { animation: dc-fade-in 80ms ease-out 6900ms forwards; }
        .dc-cl-2 { animation: dc-fade-in 80ms ease-out 7050ms forwards, dc-exec 500ms ease-in-out 9700ms forwards; }
        .dc-cl-3 { animation: dc-fade-in 80ms ease-out 7200ms forwards, dc-exec 500ms ease-in-out 9900ms forwards; }
        .dc-cl-4 { animation: dc-fade-in 80ms ease-out 7350ms forwards, dc-exec 500ms ease-in-out 10000ms forwards; }
        .dc-cl-5 { animation: dc-fade-in 80ms ease-out 7500ms forwards, dc-exec 500ms ease-in-out 10400ms forwards; }
        .dc-cl-6 { animation: dc-fade-in 80ms ease-out 7650ms forwards, dc-exec 500ms ease-in-out 10800ms forwards; }
        .dc-cl-7 { animation: dc-fade-in 80ms ease-out 7800ms forwards; }

        @keyframes dc-exec {
          0% { border-left-color: transparent; background: transparent; }
          15% { border-left-color: #0969da; background: rgba(9,105,218,0.08); }
          75% { border-left-color: #0969da; background: rgba(9,105,218,0.08); }
          100% { border-left-color: transparent; background: transparent; }
        }

        /* --- Phase 4: CI flow overlay --- */
        .dc-ci-flow {
          position: absolute;
          top: 56px;
          left: 0;
          right: 0;
          bottom: 48px;
          display: flex;
          align-items: center;
          justify-content: center;
          gap: 10px;
          opacity: 0;
          pointer-events: none;
          animation: dc-fade-in 400ms ease-out 12200ms forwards;
        }

        .dc-ci-preview {
          width: 360px;
          height: 260px;
          border-radius: 8px;
          border: 1px solid #D0D0D0;
          overflow: hidden;
          box-shadow: 0 8px 24px rgba(0,0,0,0.3);
          flex-shrink: 0;
          opacity: 0;
          animation: dc-slide-right 400ms ease-out 13800ms forwards;
          display: flex;
          flex-direction: column;
        }

        .dc-ci-desktop {
          width: 360px;
          height: 260px;
          border-radius: 8px;
          border: 1px solid #383838;
          overflow: hidden;
          display: flex;
          flex-direction: column;
          flex-shrink: 0;
          opacity: 0;
          transform-origin: left center;
          animation: dc-pop-out-right 600ms cubic-bezier(0.16, 1, 0.3, 1) 14000ms forwards;
          margin-left: 10px;
        }

        .dc-ci-desktop .dc-desk-bar { height: 12px; }
        .dc-ci-desktop .dc-mini-app { width: 70%; height: 60%; }
        .dc-ci-desktop .dc-mini-app-bar { height: 8px; }
        .dc-ci-desktop .dc-mini-app-dot { width: 2.5px; height: 2.5px; }
        .dc-ci-desktop .dc-mini-app-body { padding: 4px; gap: 2px; }
        .dc-ci-desktop .dc-mock-header { height: 6px; }
        .dc-ci-desktop .dc-mini-row { gap: 2px; }
        .dc-ci-desktop .dc-mini-input { height: 8px; }
        .dc-ci-desktop .dc-mini-typed { font-size: 4px; opacity: 0; }
        .dc-ci-desktop .dc-mini-btn { width: 12px; height: 8px; }
        .dc-ci-desktop .dc-mini-item { height: 6px; }
        .dc-ci-desktop .dc-mini-item-existing { opacity: 0; }
        .dc-ci-desktop .dc-mini-item-new { opacity: 0; }
        .dc-ci-desktop .dc-ptr { width: 10px; height: 13px; opacity: 0; }
        .dc-ci-desktop .dc-flash { opacity: 0; }
        .dc-ci-desktop .dc-click-ring { width: 14px; height: 14px; opacity: 0; }

        .dc-ci-desktop .dc-mini-item-existing { animation: dc-fade-in 150ms ease-out 14100ms forwards; }
        .dc-ci-desktop .dc-ptr { animation: dc-fade-in 150ms ease-out 14200ms forwards, dc-ptr-ci 1200ms ease-in-out 14200ms forwards; }
        .dc-ci-desktop .dc-click-1 { animation: dc-click-pulse 500ms ease-out 14400ms forwards; }
        .dc-ci-desktop .dc-mini-input { animation: dc-input-focus 150ms ease-out 14400ms forwards; }
        .dc-ci-desktop .dc-mini-typed { animation: dc-fade-in 150ms ease-out 14600ms forwards; }
        .dc-ci-desktop .dc-click-2 { animation: dc-click-pulse 500ms ease-out 14900ms forwards; }
        .dc-ci-desktop .dc-mini-btn { animation: dc-btn-click 250ms ease-out 14900ms forwards; }
        .dc-ci-desktop .dc-mini-item-new { animation: dc-fade-in 150ms ease-out 15000ms forwards; }
        .dc-ci-desktop .dc-flash { animation: dc-flash-pulse 300ms ease-out 15100ms forwards; }

        @keyframes dc-ptr-ci {
          0%   { top: 25%; left: 20%; }
          18%  { top: 42%; left: 38%; }
          23%  { top: 43.5%; left: 38%; }
          28%  { top: 42%; left: 38%; }
          55%  { top: 42%; left: 38%; }
          70%  { top: 42%; left: 78%; }
          76%  { top: 43.5%; left: 78%; }
          82%  { top: 42%; left: 78%; }
          100% { top: 50%; left: 50%; }
        }

        .dc-ci-preview-bar {
          background: #E8E8E8;
          border-bottom: 1px solid #D0D0D0;
          padding: 6px 8px;
          display: flex;
          align-items: center;
          gap: 4px;
        }

        .dc-ci-preview-dot { width: 9px; height: 9px; border-radius: 50%; }
        .dc-ci-preview-name { color: #555; font-size: 26px; margin-left: 4px; font-weight: 700; }

        .dc-ci-preview-body {
          background: #F5F5F5;
          padding: 12px 16px;
          font-size: 18px;
          line-height: 1.7;
          color: #666;
        }

        .dc-ci-preview-line { display: block; white-space: nowrap; }
        .dc-ci-preview-fn { color: #0969da; }
        .dc-ci-preview-str { color: #b35900; }
        .dc-ci-preview-num { color: #0a7d50; }
        .dc-ci-preview-comment { color: #999; }

        .dc-ci-arrow-wrap {
          display: flex;
          flex-direction: column;
          align-items: center;
          gap: 6px;
          padding: 0 16px;
          opacity: 0;
          animation: dc-fade-in 300ms ease-out 13200ms forwards;
        }

        .dc-ci-arrow-label {
          font-size: 26px;
          color: #C3FFFD;
          font-weight: 700;
          white-space: nowrap;
        }

        .dc-ci-arrow-path {
          fill: none;
          stroke: #C3FFFD;
          stroke-width: 2;
          stroke-dasharray: 80;
          stroke-dashoffset: 80;
          animation: dc-arrow-draw 600ms ease-out 13400ms forwards;
        }

        .dc-ci-arrow-head {
          fill: #C3FFFD;
          opacity: 0;
          animation: dc-fade-in 150ms ease-out 13900ms forwards;
        }

        @keyframes dc-arrow-draw {
          to { stroke-dashoffset: 0; }
        }

        .dc-ci-card {
          width: 680px;
          border-radius: 8px;
          border: 1px solid #d0d7de;
          overflow: hidden;
          box-shadow: 0 8px 24px rgba(0,0,0,0.3);
          flex-shrink: 0;
          opacity: 0;
          animation: dc-slide-left 400ms cubic-bezier(0.16,1,0.3,1) 12400ms forwards;
          display: flex;
          flex-direction: column;
          background: #ffffff;
        }

        .dc-ci-header {
          background: #f6f8fa;
          border-bottom: 1px solid #d0d7de;
          padding: 8px 12px;
          display: flex;
          align-items: center;
          gap: 6px;
          flex-shrink: 0;
        }

        .dc-ci-icon { width: 24px; height: 24px; flex-shrink: 0; }
        .dc-ci-icons { display: flex; align-items: center; gap: 5px; flex-shrink: 0; }
        .dc-ci-icons-sep { width: 1px; height: 10px; background: #d0d7de; }
        .dc-ci-title { color: #1f2328; font-size: 26px; font-weight: 700; }
        .dc-ci-run { color: #656d76; font-size: 22px; margin-left: auto; }

        .dc-ci-body {
          padding: 12px 14px;
          display: flex;
          flex-direction: column;
          gap: 0;
          font-size: 24px;
        }

        .dc-ci-step {
          display: flex;
          align-items: center;
          gap: 6px;
          padding: 5px 6px;
          border-radius: 4px;
          opacity: 0;
        }

        .dc-ci-s1 { animation: dc-fade-in 150ms ease-out 12600ms forwards; }
        .dc-ci-s2 { animation: dc-fade-in 150ms ease-out 12800ms forwards; }
        .dc-ci-s3 { animation: dc-fade-in 150ms ease-out 13000ms forwards; }
        .dc-ci-s4 { animation: dc-fade-in 150ms ease-out 13200ms forwards; }

        .dc-ci-step-hl {
          background: rgba(9, 105, 218, 0.06);
          border: 1px solid rgba(9, 105, 218, 0.15);
        }

        .dc-ci-check { width: 14px; height: 14px; flex-shrink: 0; }

        .dc-ci-name {
          color: #1f2328;
          flex: 1;
          white-space: nowrap;
          overflow: hidden;
          text-overflow: ellipsis;
        }

        .dc-ci-name-bold { font-weight: 700; }
        .dc-ci-time { color: #656d76; font-size: 22px; flex-shrink: 0; }

        .dc-ci-sep {
          height: 1px;
          background: #d0d7de;
          margin: 6px 0;
          opacity: 0;
          animation: dc-fade-in 150ms ease-out 15200ms forwards;
        }

        .dc-ci-result {
          display: flex;
          align-items: center;
          gap: 6px;
          padding: 4px 6px;
          opacity: 0;
          animation: dc-fade-in 200ms ease-out 15400ms forwards;
        }

        .dc-ci-result-text { color: #1a7f37; font-size: 24px; font-weight: 700; }
        .dc-ci-result-detail { color: #656d76; font-size: 22px; }

        /* --- Tagline --- */
        .dc-tagline {
          position: absolute;
          bottom: 16px;
          left: 50%;
          transform: translateX(-50%);
          font-size: 24px;
          color: #9BA4A6;
          opacity: 0;
          animation: dc-fade-in 500ms ease-out 16000ms forwards;
          white-space: nowrap;
        }

        .dc-tagline-em { color: #C3FFFD; font-weight: 700; }
        .dc-tagline-green { color: #00C781; font-weight: 700; }

        /* --- Keyframes --- */
        @keyframes dc-fade-in {
          from { opacity: 0; }
          to { opacity: 1; }
        }

        @keyframes dc-fade-out {
          from { opacity: 1; }
          to { opacity: 0; }
        }

        @keyframes dc-type-0 {
          from { width: 0; }
          to { width: ${CMD_0.length}ch; }
        }

        @keyframes dc-type-1 {
          from { width: 0; }
          to { width: ${CMD_1.length}ch; }
        }

        @keyframes dc-type-2 {
          from { width: 0; }
          to { width: ${CMD_2.length}ch; }
        }

        @keyframes dc-slide-left {
          from { opacity: 0; transform: translateX(-20px); }
          to { opacity: 1; transform: translateX(0); }
        }

        @keyframes dc-slide-right {
          from { opacity: 0; transform: translateX(20px); }
          to { opacity: 1; transform: translateX(0); }
        }

        @media (prefers-reduced-motion: reduce) {
          .dc-terminal, .dc-desktop, .dc-code-card,
          .dc-tagline, .dc-phase-1, .dc-phase-2, .dc-phase-3, .dc-phase-4,
          .dc-out, .dc-gap, .dc-launch,
          .dc-section-2, .dc-section-3,
          .dc-traj-line, .dc-cl,
          .dc-ci-flow, .dc-ci-preview, .dc-ci-desktop, .dc-ci-arrow-wrap,
          .dc-ci-card, .dc-ci-step, .dc-ci-sep, .dc-ci-result {
            animation: none !important;
            opacity: 1 !important;
          }
          .dc-ci-arrow-path {
            animation: none !important;
            stroke-dashoffset: 0 !important;
          }
          .dc-ci-arrow-head {
            animation: none !important;
            opacity: 0.8 !important;
          }
          .dc-cmd-0, .dc-cmd-1, .dc-cmd-2 {
            animation: none !important;
            width: auto !important;
          }
          .dc-traj-card, .dc-main {
            animation: none !important;
            opacity: 0 !important;
          }
          .dc-convert {
            animation: none !important;
            opacity: 0 !important;
          }
          .dc-convert-line {
            animation: none !important;
            stroke-dashoffset: 0 !important;
          }
          .dc-convert-head {
            animation: none !important;
            opacity: 0 !important;
          }
          .dc-left {
            animation: none !important;
            width: 28% !important;
            margin-right: 14px !important;
          }
          .dc-code-card {
            animation: none !important;
            opacity: 1 !important;
            width: 32% !important;
            margin-left: 14px !important;
          }
          .dc-replay-wrap {
            animation: none !important;
            width: 28% !important;
            margin-left: 14px !important;
          }
          .dc-ptr, .dc-flash, .dc-click-ring,
          .dc-mini-typed, .dc-mini-item-existing, .dc-mini-item-new {
            animation: none !important;
            opacity: 0 !important;
          }
        }
      `}</style>

      <div className="dc-title">
        <span className="dc-title-accent">Deterministic</span> E2E testing in CI
      </div>

      {/* Phase indicator */}
      <div className="dc-phase">
        <div className="dc-phase-step dc-phase-1">
          <div className="dc-phase-num">1</div>
          <span>Explore</span>
        </div>
        <span className="dc-phase-arrow">{"→"}</span>
        <div className="dc-phase-step dc-phase-2">
          <div className="dc-phase-num">2</div>
          <span>Codify</span>
        </div>
        <span className="dc-phase-arrow">{"→"}</span>
        <div className="dc-phase-step dc-phase-3">
          <div className="dc-phase-num">3</div>
          <span>Replay</span>
        </div>
        <span className="dc-phase-arrow">{"→"}</span>
        <div className="dc-phase-step dc-phase-4">
          <div className="dc-phase-num">4</div>
          <span>CI</span>
        </div>
      </div>

      {/* Main content */}
      <div className="dc-main">
        {/* Left: desktop (explore) → trajectory */}
        <div className="dc-left">
          <div className="dc-desktop dc-desktop-explore">
            {desktopInner}
          </div>

          {/* task.json card — visible during Phase 1 exploration */}
          <div className="dc-task-card">
            <div className="dc-task-titlebar">
              <div className="dc-dot" style={{ width: 7, height: 7, background: "#FF3B4D" }} />
              <div className="dc-dot" style={{ width: 7, height: 7, background: "#E3B341" }} />
              <div className="dc-dot" style={{ width: 7, height: 7, background: "#00C781" }} />
              <span className="dc-task-filename">task.json</span>
            </div>
            <div className="dc-task-body">
              <span className="dc-task-line">
                <span className="dc-task-brace">{"{"}</span>
              </span>
              <span className="dc-task-line">
                <span className="dc-task-indent" />
                <span className="dc-task-key">{'"instruction"'}</span>
                <span className="dc-task-brace">: </span>
                <span className="dc-task-value">{'"Add \'Buy groceries\'"'}</span>
              </span>
              <span className="dc-task-line">
                <span className="dc-task-indent" />
                <span className="dc-task-dim">...</span>
              </span>
              <span className="dc-task-line">
                <span className="dc-task-brace">{"}"}</span>
              </span>
            </div>
          </div>

          {/* Arrow: task.json → desktop explore */}
          <div className="dc-task-arrow">
            <svg width="16" height="40" viewBox="0 0 16 40">
              <path className="dc-task-arrow-line" d="M8 2 L8 28" />
              <polygon className="dc-task-arrow-head" points="4,27 8,37 12,27" />
            </svg>
          </div>

          {/* Conversion arrow: desktop ↓ trajectory.jsonl */}
          <div className="dc-convert">
            <svg width="16" height="40" viewBox="0 0 16 40">
              <path className="dc-convert-line" d="M8 2 L8 28" />
              <polygon className="dc-convert-head" points="4,27 8,37 12,27" />
            </svg>
          </div>

          <div className="dc-traj-card">
            <div className="dc-traj-titlebar">
              <div className="dc-dot" style={{ width: 7, height: 7, background: "#FF3B4D" }} />
              <div className="dc-dot" style={{ width: 7, height: 7, background: "#E3B341" }} />
              <div className="dc-dot" style={{ width: 7, height: 7, background: "#00C781" }} />
              <span className="dc-traj-filename">trajectory.jsonl</span>
            </div>
            <div className="dc-traj-body">
              <span className="dc-traj-line dc-tl-1">
                <span className="dc-traj-brace">{"{"}</span>
                <span className="dc-traj-key">{'"step"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-num">1</span>
                <span className="dc-traj-brace">, </span>
                <span className="dc-traj-key">{'"type"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-str">{'"screenshot"'}</span>
                <span className="dc-traj-brace">{"}"}</span>
              </span>
              <span className="dc-traj-line dc-tl-2">
                <span className="dc-traj-brace">{"{"}</span>
                <span className="dc-traj-key">{'"step"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-num">2</span>
                <span className="dc-traj-brace">, </span>
                <span className="dc-traj-key">{'"type"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-str">{'"click"'}</span>
                <span className="dc-traj-brace">{"}"}</span>
              </span>
              <span className="dc-traj-line dc-tl-3">
                <span className="dc-traj-brace">{"{"}</span>
                <span className="dc-traj-key">{'"step"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-num">3</span>
                <span className="dc-traj-brace">, </span>
                <span className="dc-traj-key">{'"type"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-str">{'"screenshot"'}</span>
                <span className="dc-traj-brace">{"}"}</span>
              </span>
              <span className="dc-traj-line dc-tl-4">
                <span className="dc-traj-brace">{"{"}</span>
                <span className="dc-traj-key">{'"step"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-num">4</span>
                <span className="dc-traj-brace">, </span>
                <span className="dc-traj-key">{'"type"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-str">{'"type"'}</span>
                <span className="dc-traj-brace">{"}"}</span>
              </span>
              <span className="dc-traj-line dc-tl-5">
                <span className="dc-traj-brace">{"{"}</span>
                <span className="dc-traj-key">{'"step"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-num">5</span>
                <span className="dc-traj-brace">, </span>
                <span className="dc-traj-key">{'"type"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-str">{'"screenshot"'}</span>
                <span className="dc-traj-brace">{"}"}</span>
              </span>
              <span className="dc-traj-line dc-tl-6">
                <span className="dc-traj-brace">{"{"}</span>
                <span className="dc-traj-key">{'"step"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-num">6</span>
                <span className="dc-traj-brace">, </span>
                <span className="dc-traj-key">{'"type"'}</span>
                <span className="dc-traj-brace">:</span>
                <span className="dc-traj-str">{'"click"'}</span>
                <span className="dc-traj-brace">{"}"}</span>
              </span>
              <span className="dc-traj-line dc-tl-7">
                <span className="dc-traj-dim">{"  ... 9 more steps"}</span>
              </span>
            </div>
          </div>

        </div>

        {/* Center: Terminal */}
        <div className="dc-terminal">
          <div className="dc-titlebar">
            <div className="dc-dot" style={{ background: "#FF3B4D" }} />
            <div className="dc-dot" style={{ background: "#E3B341" }} />
            <div className="dc-dot" style={{ background: "#00C781" }} />
          </div>
          <div className="dc-body">
            {/* Phase 1: Explore */}
            <span style={{ display: "block" }}>
              <span className="dc-prompt">$ </span>
              <span className="dc-cmd-0">{CMD_0}</span>
            </span>
            <span className="dc-launch dc-launch-1">
              <span className="dc-cyan">{"  ↳ Launching desktop..."}</span>
            </span>
            <span className="dc-out dc-out-e1">
              <span className="dc-dim">  Step 3/15: click (180, 245)</span>
            </span>
            <span className="dc-out dc-out-e2">
              <span className="dc-green">{"  ✓ "}</span>
              <span className="dc-white">{"Done — "}</span>
              <span className="dc-cyan">trajectory.jsonl</span>
              <span className="dc-dim"> (15 steps)</span>
            </span>

            <span className="dc-gap dc-gap-1">&nbsp;</span>

            {/* Phase 2: Codify */}
            <span className="dc-section-2">
              <span className="dc-prompt">$ </span>
              <span className="dc-cmd-1">{CMD_1}</span>
            </span>
            <span className="dc-out dc-out-c1">
              <span className="dc-dim">  Reading trajectory (15 steps)...</span>
            </span>
            <span className="dc-out dc-out-c2">
              <span className="dc-dim">  Extracting deterministic actions...</span>
            </span>
            <span className="dc-launch dc-launch-2">
              <span className="dc-cyan">{"  ↳ Writing replay script..."}</span>
            </span>
            <span className="dc-out dc-out-c3">
              <span className="dc-green">{"  ✓ "}</span>
              <span className="dc-white">Generated </span>
              <span className="dc-cyan">replay.py</span>
            </span>
            <span className="dc-out dc-out-c4">
              <span className="dc-dim">    23 actions, 0 LLM calls</span>
            </span>

            <span className="dc-gap dc-gap-2">&nbsp;</span>

            {/* Phase 3: Replay */}
            <span className="dc-section-3">
              <span className="dc-prompt">$ </span>
              <span className="dc-cmd-2">{CMD_2}</span>
            </span>
            <span className="dc-launch dc-launch-3">
              <span className="dc-cyan">{"  ↳ Replaying 23 actions..."}</span>
            </span>
            <span className="dc-out dc-out-r1">
              <span className="dc-dim">{"  ████████████████ "}</span>
              <span className="dc-white">23/23</span>
            </span>
            <span className="dc-out dc-out-r2">
              <span className="dc-green-bold">  PASSED</span>
              <span className="dc-dim"> in 18s (deterministic, $0 cost)</span>
            </span>
          </div>
        </div>

        {/* Right: replay.py */}
        <div className="dc-code-card">
          <div className="dc-code-titlebar">
            <div className="dc-code-dot" style={{ background: "#FF3B4D" }} />
            <div className="dc-code-dot" style={{ background: "#E3B341" }} />
            <div className="dc-code-dot" style={{ background: "#00C781" }} />
            <span className="dc-code-filename">replay.py</span>
          </div>
          <div className="dc-code-body">
            <span className="dc-cl dc-cl-1">
              <span className="dc-code-comment"># Auto-generated</span>
            </span>
            <span className="dc-cl dc-cl-2">
              <span className="dc-code-fn">pyautogui.click</span>
              (<span className="dc-code-num">180</span>, <span className="dc-code-num">245</span>)
            </span>
            <span className="dc-cl dc-cl-3">
              <span className="dc-code-fn">time.sleep</span>
              (<span className="dc-code-num">0.5</span>)
            </span>
            <span className="dc-cl dc-cl-4">
              <span className="dc-code-fn">pyautogui.typewrite</span>
              (<span className="dc-code-str">{"'Buy groceries'"}</span>)
            </span>
            <span className="dc-cl dc-cl-5">
              <span className="dc-code-fn">time.sleep</span>
              (<span className="dc-code-num">0.3</span>)
            </span>
            <span className="dc-cl dc-cl-6">
              <span className="dc-code-fn">pyautogui.click</span>
              (<span className="dc-code-num">380</span>, <span className="dc-code-num">245</span>)
            </span>
            <span className="dc-cl dc-cl-7">
              <span className="dc-code-comment"># ... 17 more actions</span>
            </span>
          </div>
        </div>

        {/* Right-most: replay desktop (spawned by --replay, associated with replay.py) */}
        <div className="dc-replay-wrap">
          <div className="dc-desktop dc-desktop-replay">
            {desktopInner}
          </div>
        </div>
      </div>

      {/* Phase 4: CI flow — CI pipeline executes replay.py */}
      <div className="dc-ci-flow">
        <div className="dc-ci-card">
          <div className="dc-ci-header">
            <div className="dc-ci-icons">
              <svg className="dc-ci-icon" viewBox="0 0 1024 1024" fill="none">
                <path fillRule="evenodd" clipRule="evenodd" d="M8 0C3.58 0 0 3.58 0 8C0 11.54 2.29 14.53 5.47 15.59C5.87 15.66 6.02 15.42 6.02 15.21C6.02 15.02 6.01 14.39 6.01 13.72C4 14.09 3.48 13.23 3.32 12.78C3.23 12.55 2.84 11.84 2.5 11.65C2.22 11.5 1.82 11.13 2.49 11.12C3.12 11.11 3.57 11.7 3.72 11.94C4.44 13.15 5.59 12.81 6.05 12.6C6.12 12.08 6.33 11.73 6.56 11.53C4.78 11.33 2.92 10.64 2.92 7.58C2.92 6.71 3.23 5.99 3.74 5.43C3.66 5.23 3.38 4.41 3.82 3.31C3.82 3.31 4.49 3.1 6.02 4.13C6.66 3.95 7.34 3.86 8.02 3.86C8.7 3.86 9.38 3.95 10.02 4.13C11.55 3.09 12.22 3.31 12.22 3.31C12.66 4.41 12.38 5.23 12.3 5.43C12.81 5.99 13.12 6.7 13.12 7.58C13.12 10.65 11.25 11.33 9.47 11.53C9.76 11.78 10.01 12.26 10.01 13.01C10.01 14.08 10 14.94 10 15.21C10 15.42 10.15 15.67 10.55 15.59C13.71 14.53 16 11.53 16 8C16 3.58 12.42 0 8 0Z" transform="scale(64)" fill="#1f2328" />
              </svg>
              <div className="dc-ci-icons-sep" />
              <svg className="dc-ci-icon" viewBox="0 0 32 32" fill="none">
                <path d="m31.46 12.78-.04-.12-4.35-11.35A1.14 1.14 0 0 0 25.94.6c-.24 0-.47.1-.66.24-.19.15-.33.36-.39.6l-2.94 9h-11.9l-2.94-9A1.14 1.14 0 0 0 6.07.58a1.15 1.15 0 0 0-1.14.72L.58 12.68l-.05.11a8.1 8.1 0 0 0 2.68 9.34l.02.01.04.03 6.63 4.97 3.28 2.48 2 1.52a1.35 1.35 0 0 0 1.62 0l2-1.52 3.28-2.48 6.67-5h.02a8.09 8.09 0 0 0 2.7-9.36Z" fill="#E24329" />
                <path d="m31.46 12.78-.04-.12a14.75 14.75 0 0 0-5.86 2.64l-9.55 7.24 6.09 4.6 6.67-5h.02a8.09 8.09 0 0 0 2.67-9.36Z" fill="#FC6D26" />
                <path d="m9.9 27.14 3.28 2.48 2 1.52a1.35 1.35 0 0 0 1.62 0l2-1.52 3.28-2.48-6.1-4.6-6.07 4.6Z" fill="#FCA326" />
                <path d="M6.44 15.3a14.71 14.71 0 0 0-5.86-2.63l-.05.12a8.1 8.1 0 0 0 2.68 9.34l.02.01.04.03 6.63 4.97 6.1-4.6-9.56-7.24Z" fill="#FC6D26" />
              </svg>
            </div>
            <span className="dc-ci-title">E2E Tests</span>
            <span className="dc-ci-run">#142</span>
          </div>
          <div className="dc-ci-body">
            <div className="dc-ci-step dc-ci-s1">
              <svg className="dc-ci-check" viewBox="0 0 16 16">
                <circle cx="8" cy="8" r="7" fill="#1a7f37" />
                <path d="M5.5 8L7 9.5L10.5 6" stroke="#fff" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              <span className="dc-ci-name">Checkout code</span>
              <span className="dc-ci-time">2s</span>
            </div>
            <div className="dc-ci-step dc-ci-s2">
              <svg className="dc-ci-check" viewBox="0 0 16 16">
                <circle cx="8" cy="8" r="7" fill="#1a7f37" />
                <path d="M5.5 8L7 9.5L10.5 6" stroke="#fff" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              <span className="dc-ci-name">Build container</span>
              <span className="dc-ci-time">34s</span>
            </div>
            <div className="dc-ci-step dc-ci-s3 dc-ci-step-hl">
              <svg className="dc-ci-check" viewBox="0 0 16 16">
                <circle cx="8" cy="8" r="7" fill="#1a7f37" />
                <path d="M5.5 8L7 9.5L10.5 6" stroke="#fff" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              <span className="dc-ci-name dc-ci-name-bold">desktest --replay</span>
              <span className="dc-ci-time">18s</span>
            </div>
            <div className="dc-ci-step dc-ci-s4">
              <svg className="dc-ci-check" viewBox="0 0 16 16">
                <circle cx="8" cy="8" r="7" fill="#1a7f37" />
                <path d="M5.5 8L7 9.5L10.5 6" stroke="#fff" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              <span className="dc-ci-name">Upload artifacts</span>
              <span className="dc-ci-time">1s</span>
            </div>
            <div className="dc-ci-sep" />
            <div className="dc-ci-result">
              <svg className="dc-ci-check" viewBox="0 0 16 16">
                <circle cx="8" cy="8" r="7" fill="#1a7f37" />
                <path d="M5.5 8L7 9.5L10.5 6" stroke="#fff" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              <span className="dc-ci-result-text">All checks passed</span>
              <span className="dc-ci-result-detail">55s · $0</span>
            </div>
          </div>
        </div>

        <div className="dc-ci-arrow-wrap">
          <span className="dc-ci-arrow-label">executes</span>
          <svg width="90" height="20" viewBox="0 0 90 20">
            <path className="dc-ci-arrow-path" d="M0 10 L72 10" />
            <polygon className="dc-ci-arrow-head" points="72,5 82,10 72,15" />
          </svg>
        </div>

        <div className="dc-ci-preview">
          <div className="dc-ci-preview-bar">
            <div className="dc-ci-preview-dot" style={{ background: "#FF3B4D" }} />
            <div className="dc-ci-preview-dot" style={{ background: "#E3B341" }} />
            <div className="dc-ci-preview-dot" style={{ background: "#00C781" }} />
            <span className="dc-ci-preview-name">replay.py</span>
          </div>
          <div className="dc-ci-preview-body">
            <span className="dc-ci-preview-line"><span className="dc-ci-preview-comment"># Auto-generated — 23 actions</span></span>
            <span className="dc-ci-preview-line"><span className="dc-ci-preview-fn">pyautogui.click</span>(<span className="dc-ci-preview-num">180</span>, <span className="dc-ci-preview-num">245</span>)</span>
            <span className="dc-ci-preview-line"><span className="dc-ci-preview-fn">time.sleep</span>(<span className="dc-ci-preview-num">0.5</span>)</span>
            <span className="dc-ci-preview-line"><span className="dc-ci-preview-fn">pyautogui.typewrite</span>(<span className="dc-ci-preview-str">{"'Buy groceries'"}</span>)</span>
            <span className="dc-ci-preview-line"><span className="dc-ci-preview-fn">pyautogui.click</span>(<span className="dc-ci-preview-num">380</span>, <span className="dc-ci-preview-num">245</span>)</span>
            <span className="dc-ci-preview-line"><span className="dc-ci-preview-comment"># ... 18 more actions</span></span>
          </div>
        </div>

        <div className="dc-ci-desktop">
          <div className="dc-desk-bar" />
          <div className="dc-desk-body">
            <div className="dc-mini-app">
              <div className="dc-mini-app-bar">
                <div className="dc-mini-app-dot" style={{ background: "#FF3B4D" }} />
                <div className="dc-mini-app-dot" style={{ background: "#E3B341" }} />
                <div className="dc-mini-app-dot" style={{ background: "#00C781" }} />
              </div>
              <div className="dc-mini-app-body">
                <div className="dc-mock-header" />
                <div className="dc-mini-row">
                  <div className="dc-mini-input">
                    <span className="dc-mini-typed">Buy groceries</span>
                  </div>
                  <div className="dc-mini-btn" />
                </div>
                <div className="dc-mini-item dc-mini-item-existing" />
                <div className="dc-mini-item dc-mini-item-new" />
              </div>
            </div>
            <svg className="dc-ptr" viewBox="0 0 24 24" fill="none">
              <path d="M5 3l14 8-6 2-4 6-4-16z" fill="#fff" stroke="#000" strokeWidth="1.5" strokeLinejoin="round" />
            </svg>
            <div className="dc-flash" />
            <div className="dc-click-ring dc-click-1" />
            <div className="dc-click-ring dc-click-2" />
          </div>
        </div>
      </div>

      {/* Tagline */}
      <div className="dc-tagline">
        <span className="dc-tagline-em">Explore once</span>
        <span>, replay forever — </span>
        <span className="dc-tagline-green">deterministic</span>
        <span> CI at </span>
        <span className="dc-tagline-em">$0</span>
      </div>
    </div>
  );
}
