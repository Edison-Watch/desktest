import React, { useState, useEffect } from "react";

const COMMAND = "desktest suite tests/";
const CHAR_COUNT = COMMAND.length;
const CYCLE_MS = 11000;

export default function DesktestOrchestrationAnimation() {
  const [cycle, setCycle] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setCycle((c) => c + 1), CYCLE_MS);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="do-scene" key={cycle}>
      <style>{`
        .do-scene {
          position: relative;
          width: 100%;
          aspect-ratio: 16 / 9;
          background: #000;
          overflow: hidden;
          font-family: 'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace;
        }

        /* ── Terminal ── */
        .do-terminal {
          position: absolute;
          top: 16%;
          left: 24%;
          width: 52%;
          border-radius: 12px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow:
            0 20px 60px rgba(0, 0, 0, 0.8),
            0 8px 24px rgba(0, 0, 0, 0.5);
          opacity: 0;
          animation: do-fade-in 67ms ease-out forwards;
          z-index: 2;
        }

        .do-titlebar {
          background: #2F3440;
          border-bottom: 1px solid #383838;
          padding: 10px 14px;
          display: flex;
          gap: 7px;
          align-items: center;
        }

        .do-dot {
          width: 11px;
          height: 11px;
          border-radius: 50%;
        }

        .do-body {
          background: #1C1C1C;
          padding: 14px 18px;
        }

        .do-prompt-line {
          display: flex;
          align-items: center;
          font-size: 30px;
          line-height: 1.5;
        }

        .do-prompt {
          color: #C3FFFD;
          font-weight: 700;
        }

        .do-typed {
          color: #F9F9F9;
          overflow: hidden;
          white-space: nowrap;
          display: inline-block;
          width: 0;
          animation: do-type 690ms steps(${CHAR_COUNT}) 70ms forwards;
          font-family: inherit;
        }

        .do-cursor {
          display: inline-block;
          width: 8px;
          height: 17px;
          background: #C3FFFD;
          margin-left: 1px;
          vertical-align: text-bottom;
          opacity: 0;
          animation: do-cursor-show 70ms 70ms forwards, do-fade-out 50ms ease-out 800ms forwards;
        }

        .do-output {
          font-size: 28px;
          line-height: 1.6;
          color: #9BA4A6;
          opacity: 0;
        }

        .do-output-1 {
          animation: do-fade-in-flat 200ms ease-out 1000ms forwards;
        }

        .do-output-2 {
          animation: do-fade-in-flat 200ms ease-out 1300ms forwards;
        }

        .do-output-result {
          color: #00C781;
          font-weight: 700;
          animation: do-fade-in-flat 300ms ease-out 9200ms forwards;
        }

        /* ── VM panels ── */
        .do-vm {
          position: absolute;
          top: 50%;
          width: 28%;
          border-radius: 10px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow:
            0 16px 48px rgba(0, 0, 0, 0.7),
            0 6px 20px rgba(0, 0, 0, 0.4);
          display: flex;
          flex-direction: column;
          opacity: 0;
          z-index: 2;
        }

        .do-vm-1 {
          left: 3.5%;
          animation: do-vm-in 400ms ease-out 1800ms forwards;
        }

        .do-vm-2 {
          left: 36%;
          animation: do-vm-in 400ms ease-out 2100ms forwards;
        }

        .do-vm-3 {
          left: 68.5%;
          animation: do-vm-in 400ms ease-out 2400ms forwards;
        }

        .do-vm-titlebar {
          background: #2F3440;
          border-bottom: 1px solid #383838;
          padding: 8px 10px;
          display: flex;
          gap: 5px;
          align-items: center;
        }

        .do-dot-sm {
          width: 8px;
          height: 8px;
          border-radius: 50%;
        }

        .do-vm-name {
          color: #9BA4A6;
          font-size: 20px;
          margin-left: 6px;
        }

        /* ── OS label at bottom ── */
        .do-vm-label {
          display: flex;
          align-items: center;
          justify-content: center;
          gap: 10px;
          padding: 10px 10px 12px;
          background: #141414;
          border-top: 1px solid #2a2a2a;
        }

        .do-vm-label-icon {
          display: flex;
          align-items: center;
        }

        .do-vm-label-icon svg {
          width: 34px;
          height: 34px;
        }

        .do-vm-label-text {
          display: flex;
          flex-direction: column;
          gap: 1px;
        }

        .do-vm-label-os {
          color: #F9F9F9;
          font-size: 22px;
          font-weight: 700;
        }

        .do-vm-label-tech {
          color: #9BA4A6;
          font-size: 20px;
        }

        .do-vm-body {
          background: #1C1C1C;
          padding: 12px;
          display: flex;
          flex-direction: column;
          gap: 6px;
          position: relative;
        }

        /* ── VM cursors ── */
        .do-vm-ptr {
          position: absolute;
          width: 14px;
          height: 17px;
          z-index: 4;
          pointer-events: none;
          filter: drop-shadow(0 1px 3px rgba(0,0,0,0.9));
          opacity: 0;
        }

        .do-vm-ptr-1 {
          animation:
            do-ptr-1 5s ease-in-out 3200ms infinite,
            do-fade-in-flat 150ms 3200ms forwards,
            do-fade-out 150ms 6300ms forwards;
        }

        .do-vm-ptr-2 {
          animation:
            do-ptr-2 7s ease-in-out 3400ms infinite,
            do-fade-in-flat 150ms 3400ms forwards,
            do-fade-out 150ms 7300ms forwards;
        }

        .do-vm-ptr-3 {
          animation:
            do-ptr-3 9s ease-in-out 3600ms infinite,
            do-fade-in-flat 150ms 3600ms forwards,
            do-fade-out 150ms 8300ms forwards;
        }

        /* ── Mock app content ── */
        .do-mock-bar {
          height: 7px;
          border-radius: 3px;
          background: #2a2a2a;
        }

        .do-mock-bar-highlight {
          background: #2a3a3a;
        }

        .do-mock-bar-header {
          height: 10px;
          background: #333;
          margin-bottom: 4px;
        }

        .do-mock-bar-input {
          height: 16px;
          border: 1px solid #383838;
          background: transparent;
          border-radius: 3px;
        }

        .do-mock-bar-btn {
          height: 14px;
          background: rgba(195, 255, 253, 0.2);
          border-radius: 3px;
        }

        /* ── Screenshot flash overlay ── */
        .do-vm-flash {
          position: absolute;
          inset: 0;
          border: 2px solid #C3FFFD;
          opacity: 0;
          pointer-events: none;
          z-index: 3;
        }

        .do-flash-1 { animation: do-flash 400ms ease-out 5000ms forwards; }
        .do-flash-2 { animation: do-flash 400ms ease-out 6000ms forwards; }
        .do-flash-3 { animation: do-flash 400ms ease-out 7000ms forwards; }

        /* ── Status bar ── */
        .do-vm-status {
          background: #1a1a1a;
          border-top: 1px solid #383838;
          padding: 8px 10px;
          position: relative;
          min-height: 28px;
        }

        .do-status-running {
          display: flex;
          align-items: center;
          gap: 6px;
          font-size: 20px;
          color: #C3FFFD;
          opacity: 0;
          position: absolute;
          inset: 0;
          padding: 0 10px;
        }

        .do-status-running-1 {
          animation:
            do-fade-in-flat 150ms 3000ms forwards,
            do-fade-out 150ms 6400ms forwards;
        }

        .do-status-running-2 {
          animation:
            do-fade-in-flat 150ms 3200ms forwards,
            do-fade-out 150ms 7400ms forwards;
        }

        .do-status-running-3 {
          animation:
            do-fade-in-flat 150ms 3400ms forwards,
            do-fade-out 150ms 8400ms forwards;
        }

        .do-spinner {
          width: 12px;
          height: 12px;
          border-radius: 50%;
          border: 2px solid #383838;
          border-top-color: #C3FFFD;
          animation: do-spin 800ms linear infinite;
          flex-shrink: 0;
        }

        .do-status-pass {
          display: flex;
          align-items: center;
          font-size: 20px;
          color: #00C781;
          font-weight: 700;
          opacity: 0;
          position: absolute;
          inset: 0;
          padding: 0 10px;
        }

        .do-status-pass-1 { animation: do-fade-in-flat 200ms ease-out 6500ms forwards; }
        .do-status-pass-2 { animation: do-fade-in-flat 200ms ease-out 7500ms forwards; }
        .do-status-pass-3 { animation: do-fade-in-flat 200ms ease-out 8500ms forwards; }

        /* ── Connection lines SVG ── */
        .do-lines {
          position: absolute;
          inset: 0;
          width: 100%;
          height: 100%;
          pointer-events: none;
          z-index: 1;
        }

        .do-conn-group {
          opacity: 0;
          animation: do-fade-in-flat 400ms ease-out 2800ms forwards;
        }

        .do-conn-path {
          fill: none;
          stroke: #C3FFFD;
          stroke-width: 1.5;
          opacity: 0.4;
          stroke-dasharray: 8 5;
          animation: do-march 500ms linear 2800ms infinite;
        }

        .do-conn-head {
          fill: #C3FFFD;
          opacity: 0.4;
        }


        /* ── Keyframes ── */
        @keyframes do-fade-in {
          from { opacity: 0; }
          to { opacity: 1; }
        }

        @keyframes do-fade-in-flat {
          from { opacity: 0; }
          to { opacity: 1; }
        }

        @keyframes do-fade-out {
          from { opacity: 1; }
          to { opacity: 0; }
        }

        @keyframes do-type {
          from { width: 0; }
          to { width: ${CHAR_COUNT}ch; }
        }

        @keyframes do-cursor-show {
          from { opacity: 0; }
          to { opacity: 1; }
        }

        @keyframes do-vm-in {
          from { opacity: 0; transform: translateY(20px) scale(0.95); }
          to { opacity: 1; transform: translateY(0) scale(1); }
        }

        @keyframes do-spin {
          to { transform: rotate(360deg); }
        }

        @keyframes do-flash {
          0% { opacity: 0.9; }
          100% { opacity: 0; }
        }

        @keyframes do-march {
          to { stroke-dashoffset: -13; }
        }


        /* Cursor 1: 2 clicks, mostly horizontal, long dwell */
        @keyframes do-ptr-1 {
          0%   { top: 20%; left: 15%; }
          8%   { top: 20%; left: 15%; }
          30%  { top: 25%; left: 70%; }
          32%  { top: 26.5%; left: 70%; }
          34%  { top: 25%; left: 70%; }
          55%  { top: 25%; left: 70%; }
          75%  { top: 60%; left: 40%; }
          77%  { top: 61.5%; left: 40%; }
          79%  { top: 60%; left: 40%; }
          92%  { top: 60%; left: 40%; }
          100% { top: 20%; left: 15%; }
        }

        /* Cursor 2: 4 clicks, diagonal zigzag, quick moves */
        @keyframes do-ptr-2 {
          0%   { top: 70%; left: 65%; }
          12%  { top: 25%; left: 30%; }
          14%  { top: 26.5%; left: 30%; }
          16%  { top: 25%; left: 30%; }
          18%  { top: 25%; left: 30%; }
          32%  { top: 40%; left: 75%; }
          34%  { top: 41.5%; left: 75%; }
          36%  { top: 40%; left: 75%; }
          50%  { top: 40%; left: 75%; }
          62%  { top: 70%; left: 20%; }
          64%  { top: 71.5%; left: 20%; }
          66%  { top: 70%; left: 20%; }
          68%  { top: 70%; left: 20%; }
          82%  { top: 50%; left: 55%; }
          84%  { top: 51.5%; left: 55%; }
          86%  { top: 50%; left: 55%; }
          100% { top: 70%; left: 65%; }
        }

        /* Cursor 3: 3 clicks, vertical sweep, slow with long pauses */
        @keyframes do-ptr-3 {
          0%   { top: 15%; left: 50%; }
          5%   { top: 15%; left: 50%; }
          18%  { top: 45%; left: 35%; }
          20%  { top: 46.5%; left: 35%; }
          22%  { top: 45%; left: 35%; }
          38%  { top: 45%; left: 35%; }
          50%  { top: 75%; left: 60%; }
          52%  { top: 76.5%; left: 60%; }
          54%  { top: 75%; left: 60%; }
          65%  { top: 75%; left: 60%; }
          78%  { top: 30%; left: 72%; }
          80%  { top: 31.5%; left: 72%; }
          82%  { top: 30%; left: 72%; }
          93%  { top: 30%; left: 72%; }
          100% { top: 15%; left: 50%; }
        }

        .do-title {
          position: absolute;
          top: 80px;
          left: 50%;
          transform: translateX(-50%);
          font-size: 64px;
          color: #F9F9F9;
          font-weight: 700;
          white-space: nowrap;
          opacity: 0;
          animation: do-fade-in 500ms ease-out 100ms forwards;
          z-index: 10;
        }

        .do-title-accent { color: #C3FFFD; }

        .do-tagline {
          position: absolute;
          bottom: 16px;
          left: 50%;
          transform: translateX(-50%);
          font-size: 24px;
          color: #9BA4A6;
          opacity: 0;
          animation: do-fade-in 500ms ease-out 9500ms forwards;
          white-space: nowrap;
          z-index: 10;
        }

        .do-tagline-em { color: #C3FFFD; font-weight: 700; }

        @media (prefers-reduced-motion: reduce) {
          .do-terminal { animation: none; opacity: 1; }
          .do-typed { animation: none; width: ${CHAR_COUNT}ch; }
          .do-cursor { animation: none; opacity: 0; }
          .do-vm-1, .do-vm-2, .do-vm-3 {
            animation: none; opacity: 1; transform: none;
          }
          .do-output-1, .do-output-2, .do-output-result {
            animation: none; opacity: 1;
          }
          .do-conn-group { animation: none; opacity: 1; }
          .do-conn-path { animation: none; }
          .do-status-running-1, .do-status-running-2, .do-status-running-3 {
            animation: none !important; opacity: 0 !important;
          }
          .do-status-pass-1, .do-status-pass-2, .do-status-pass-3 {
            animation: none; opacity: 1;
          }
          .do-spinner { animation: none; }
          .do-vm-flash { animation: none; }
          .do-vm-ptr { animation: none !important; opacity: 0 !important; }
          .do-title, .do-tagline { animation: none; opacity: 1; }
        }
      `}</style>

      <div className="do-title">
        <span className="do-title-accent">Cross-platform</span> test orchestration
      </div>

      {/* Terminal */}
      <div className="do-terminal">
        <div className="do-titlebar">
          <div className="do-dot" style={{ background: "#FF3B4D" }} />
          <div className="do-dot" style={{ background: "#E3B341" }} />
          <div className="do-dot" style={{ background: "#00C781" }} />
        </div>
        <div className="do-body">
          <div className="do-prompt-line">
            <span className="do-prompt">$&nbsp;</span>
            <span className="do-typed">{COMMAND}</span>
            <span className="do-cursor" />
          </div>
          <div className="do-output do-output-1">Discovered 3 tasks</div>
          <div className="do-output do-output-2">Launching 3 sessions...</div>
          <div className="do-output do-output-result">
            {"✓ 3/3 passed (0 failed) — 42s"}
          </div>
        </div>
      </div>

      {/* SVG connection lines */}
      <svg
        className="do-lines"
        viewBox="0 0 1920 1080"
        preserveAspectRatio="xMidYMid meet"
      >
        <g className="do-conn-group">
          <path
            className="do-conn-path"
            d="M 960 220 Q 650 370 336 530"
          />
          <polygon
            className="do-conn-head"
            points="328,522 344,522 336,542"
          />

          <line
            className="do-conn-path"
            x1="960" y1="220" x2="960" y2="530"
          />
          <polygon
            className="do-conn-head"
            points="952,522 968,522 960,542"
          />

          <path
            className="do-conn-path"
            d="M 960 220 Q 1270 370 1584 530"
          />
          <polygon
            className="do-conn-head"
            points="1576,522 1592,522 1584,542"
          />
        </g>
      </svg>

      {/* VM 1 — Linux */}
      <div className="do-vm do-vm-1">
        <div className="do-vm-titlebar">
          <div className="do-dot-sm" style={{ background: "#FF3B4D" }} />
          <div className="do-dot-sm" style={{ background: "#E3B341" }} />
          <div className="do-dot-sm" style={{ background: "#00C781" }} />
        </div>
        <div className="do-vm-body">
          <div className="do-mock-bar" style={{ width: "85%" }} />
          <div className="do-mock-bar" style={{ width: "55%" }} />
          <div
            className="do-mock-bar do-mock-bar-highlight"
            style={{ width: "72%" }}
          />
          <div className="do-mock-bar" style={{ width: "40%" }} />
          <div className="do-mock-bar" style={{ width: "90%" }} />
          <div className="do-mock-bar" style={{ width: "30%" }} />
          <div className="do-mock-bar" style={{ width: "65%" }} />
          <svg className="do-vm-ptr do-vm-ptr-1" viewBox="0 0 24 24" fill="none">
            <path d="M5 3l14 8-6 2-4 6-4-16z" fill="#fff" stroke="#000" strokeWidth="1.5" strokeLinejoin="round" />
          </svg>
        </div>
        <div className="do-vm-flash do-flash-1" />
        <div className="do-vm-status">
          <div className="do-status-running do-status-running-1">
            <div className="do-spinner" />
            <span>Running · Step 7/10</span>
          </div>
          <div className="do-status-pass do-status-pass-1">
            {"✓ PASS · 10 steps · 18s"}
          </div>
        </div>
        <div className="do-vm-label">
          <span className="do-vm-label-icon">
            <svg viewBox="0 0 256 295">
              <defs>
                <linearGradient id="do_lt0" x1="48.548%" x2="51.047%" y1="115.276%" y2="41.364%"><stop offset="0%" stopColor="#FFEED7"/><stop offset="100%" stopColor="#BDBFC2"/></linearGradient>
                <linearGradient id="do_lt1" x1="54.407%" x2="46.175%" y1="2.404%" y2="90.542%"><stop offset="0%" stopColor="#FFF" stopOpacity=".8"/><stop offset="100%" stopColor="#FFF" stopOpacity="0"/></linearGradient>
                <linearGradient id="do_lt2" x1="51.86%" x2="47.947%" y1="88.248%" y2="9.748%"><stop offset="0%" stopColor="#FFEED7"/><stop offset="100%" stopColor="#BDBFC2"/></linearGradient>
                <linearGradient id="do_lt3" x1="49.925%" x2="49.924%" y1="85.49%" y2="13.811%"><stop offset="0%" stopColor="#FFEED7"/><stop offset="100%" stopColor="#BDBFC2"/></linearGradient>
                <linearGradient id="do_lt4" x1="53.901%" x2="45.956%" y1="3.102%" y2="93.895%"><stop offset="0%" stopColor="#FFF" stopOpacity=".65"/><stop offset="100%" stopColor="#FFF" stopOpacity="0"/></linearGradient>
                <linearGradient id="do_lt5" x1="45.593%" x2="54.811%" y1="5.475%" y2="93.524%"><stop offset="0%" stopColor="#FFF" stopOpacity=".65"/><stop offset="100%" stopColor="#FFF" stopOpacity="0"/></linearGradient>
                <linearGradient id="do_lt6" x1="49.984%" x2="49.984%" y1="89.845%" y2="40.632%"><stop offset="0%" stopColor="#FFEED7"/><stop offset="100%" stopColor="#BDBFC2"/></linearGradient>
                <linearGradient id="do_lt7" x1="53.505%" x2="42.746%" y1="99.975%" y2="23.545%"><stop offset="0%" stopColor="#FFEED7"/><stop offset="100%" stopColor="#BDBFC2"/></linearGradient>
                <linearGradient id="do_lt8" x1="49.841%" x2="50.241%" y1="13.229%" y2="94.673%"><stop offset="0%" stopColor="#FFF" stopOpacity=".8"/><stop offset="100%" stopColor="#FFF" stopOpacity="0"/></linearGradient>
                <linearGradient id="do_lt9" x1="49.927%" x2="50.727%" y1="37.327%" y2="92.782%"><stop offset="0%" stopColor="#FFF" stopOpacity=".65"/><stop offset="100%" stopColor="#FFF" stopOpacity="0"/></linearGradient>
                <linearGradient id="do_lta" x1="49.876%" x2="49.876%" y1="2.299%" y2="81.204%"><stop offset="0%" stopColor="#FFF" stopOpacity=".65"/><stop offset="100%" stopColor="#FFF" stopOpacity="0"/></linearGradient>
                <linearGradient id="do_ltb" x1="49.833%" x2="49.824%" y1="2.272%" y2="71.799%"><stop offset="0%" stopColor="#FFF" stopOpacity=".65"/><stop offset="100%" stopColor="#FFF" stopOpacity="0"/></linearGradient>
                <linearGradient id="do_ltc" x1="53.467%" x2="38.949%" y1="48.921%" y2="98.1%"><stop offset="0%" stopColor="#FFA63F"/><stop offset="100%" stopColor="#FF0"/></linearGradient>
                <linearGradient id="do_ltd" x1="52.373%" x2="47.579%" y1="143.009%" y2="-64.622%"><stop offset="0%" stopColor="#FFEED7"/><stop offset="100%" stopColor="#BDBFC2"/></linearGradient>
                <linearGradient id="do_lte" x1="30.581%" x2="65.887%" y1="34.024%" y2="89.175%"><stop offset="0%" stopColor="#FFA63F"/><stop offset="100%" stopColor="#FF0"/></linearGradient>
                <linearGradient id="do_ltf" x1="59.572%" x2="48.361%" y1="-17.216%" y2="66.118%"><stop offset="0%" stopColor="#FFF" stopOpacity=".65"/><stop offset="100%" stopColor="#FFF" stopOpacity="0"/></linearGradient>
                <linearGradient id="do_ltg" x1="47.769%" x2="51.373%" y1="1.565%" y2="104.313%"><stop offset="0%" stopColor="#FFF" stopOpacity=".65"/><stop offset="100%" stopColor="#FFF" stopOpacity="0"/></linearGradient>
                <linearGradient id="do_lth" x1="43.55%" x2="57.114%" y1="4.533%" y2="92.827%"><stop offset="0%" stopColor="#FFF" stopOpacity=".65"/><stop offset="100%" stopColor="#FFF" stopOpacity="0"/></linearGradient>
                <linearGradient id="do_lti" x1="49.733%" x2="50.558%" y1="17.609%" y2="99.385%"><stop offset="0%" stopColor="#FFA63F"/><stop offset="100%" stopColor="#FF0"/></linearGradient>
                <linearGradient id="do_ltj" x1="50.17%" x2="49.68%" y1="2.89%" y2="94.17%"><stop offset="0%" stopColor="#FFF" stopOpacity=".65"/><stop offset="100%" stopColor="#FFF" stopOpacity="0"/></linearGradient>
              </defs>
              <g fill="none">
                <path fill="#000" d="M63.213 215.474c-11.387-16.346-13.591-69.606 12.947-102.39C89.292 97.383 92.69 86.455 93.7 71.67c.734-16.805-11.846-66.851 35.537-70.616c48.027-3.857 45.364 43.526 45.088 68.596c-.183 21.12 15.52 33.15 26.355 49.68c19.927 30.303 18.274 82.461-3.765 110.745c-27.916 35.354-51.791 20.018-67.678 21.304c-29.752 1.745-30.762 17.54-66.024-35.905"/>
                <path fill="url(#do_lt0)" d="M169.1 122.451c8.265 7.622 29.661 41.69-4.224 62.995c-11.937 7.438 10.653 35.721 21.488 22.039c19.193-24.61 6.98-63.913-4.591-77.963c-7.714-9.917-19.651-13.774-12.672-7.07" transform="translate(10)"/>
                <path fill="#000" stroke="#000" strokeWidth=".977" d="M176.805 117.86c13.59 11.02 38.292 49.587 2.204 74.748c-11.846 7.806 10.468 32.508 23.049 19.927c43.618-43.894-1.102-94.308-16.53-111.664c-13.774-15.151-25.987 3.49-8.723 16.989z"/>
                <path fill="url(#do_lt1)" d="M147.245 25.02c-.459 12.581-14.325 23.51-30.946 24.52c-16.621 1.01-29.66-8.54-29.202-21.121c.46-12.581 14.326-23.509 30.947-24.519c16.62-.918 29.66 8.54 29.201 21.12" transform="translate(10)"/>
                <path fill="url(#do_lt2)" d="M107.483 54.957c.46 8.173-3.397 15.06-8.723 15.335c-5.326.276-10.01-6.06-10.469-14.233c-.459-8.173 3.398-15.06 8.724-15.335c5.326-.276 10.01 6.06 10.468 14.233" transform="translate(10)"/>
                <path fill="url(#do_lt3)" d="M117.125 55.6c.184 9.458 6.337 16.988 13.683 16.805c7.346-.184 13.131-7.99 12.948-17.54c-.184-9.458-6.336-16.988-13.683-16.804c-7.346.183-13.223 8.08-12.948 17.539" transform="translate(10)"/>
                <path fill="#000" d="M133.186 57.712c-.092 5.234 2.48 9.458 5.877 9.458c3.306 0 6.153-4.224 6.245-9.366c.091-5.234-2.48-9.459-5.878-9.459c-3.397 0-6.152 4.225-6.244 9.367m-21.212.092c.459 4.316-1.194 7.989-3.582 8.356c-2.387.276-4.683-2.938-5.142-7.254c-.46-4.316 1.194-7.99 3.581-8.357c2.388-.275 4.684 2.939 5.143 7.255"/>
                <path fill="url(#do_lt4)" d="M124.564 54.773c-.276 2.939 1.102 5.326 3.03 5.51c1.928.184 3.765-2.112 4.04-4.959c.276-2.938-1.102-5.326-3.03-5.51c-1.928-.183-3.765 2.113-4.04 4.96" transform="translate(10)"/>
                <path fill="url(#do_lt5)" d="M99.953 55.508c.276 2.388-.734 4.5-2.203 4.683c-1.47.184-2.847-1.653-3.123-4.132c-.275-2.388.735-4.5 2.204-4.683c1.47-.184 2.847 1.744 3.122 4.132" transform="translate(10)"/>
                <path fill="url(#do_lt6)" d="M71.027 145.684c6.52-14.785 20.386-40.772 20.662-60.883c0-15.978 47.843-19.835 51.7-3.856c3.856 15.978 13.59 39.853 19.834 51.424c6.245 11.478 24.335 48.118 5.051 80.074c-17.356 28.284-69.973 50.69-98.073-3.856c-9.55-18.917-7.806-42.333.826-62.903" transform="translate(10)"/>
                <path fill="url(#do_lt7)" d="M65.15 134.664c-5.601 10.56-17.172 38.293 11.112 53.445c30.395 16.162 30.303 49.312-6.245 33.517c-33.425-14.233-18.641-71.902-9.274-85.676c6.06-9.642 15.243-21.488 4.407-1.286" transform="translate(10)"/>
                <path fill="#000" stroke="#000" strokeWidth="1.25" d="M79.925 122.727c-8.907 14.509-30.211 48.669-1.652 66.484c38.384 23.6 27.548 47.108-7.53 25.895c-49.404-29.568-5.97-89.257 13.774-112.03c22.59-25.529 4.316 4.683-4.592 19.65z"/>
                <path fill="url(#do_lt8)" d="M156.428 151.285c0 16.162-15.519 37.1-42.15 36.916c-27.456.183-39.118-20.754-39.118-36.916c0-16.161 18.182-29.293 40.588-29.293c22.498.092 40.68 13.132 40.68 29.293" transform="translate(10)"/>
                <path fill="url(#do_lt9)" d="M141.92 100.504c-.276 16.713-11.204 20.662-24.978 20.662c-13.775 0-23.784-2.48-24.978-20.662c0-11.387 11.203-17.998 24.978-17.998c13.774-.092 24.977 6.52 24.977 17.998" transform="translate(10)"/>
                <path fill="url(#do_lta)" d="M58.63 126.216c9-13.682 28.008-34.711 3.582 2.939c-19.835 31.038-7.346 50.965-.918 56.474c18.549 16.53 17.814 27.64 3.214 18.917c-31.314-18.641-24.794-50.047-5.878-78.33" transform="translate(10)"/>
                <path fill="url(#do_ltb)" d="M188.936 131.818c-7.806-16.07-32.6-56.842 1.193-9.459c30.763 42.884 9.183 72.729 5.326 75.667c-3.856 2.939-16.804 8.908-13.04-1.469c3.858-10.377 22.958-30.028 6.52-64.74" transform="translate(10)"/>
                <path fill="url(#do_ltc)" stroke="#E68C3F" strokeWidth="6.25" d="M51.835 258.542c-20.57-10.928-50.414 2.112-39.578-27.457c2.204-6.704-3.214-16.805.275-23.325c4.133-7.989 13.04-6.244 18.366-11.57c5.234-5.51 8.54-15.06 18.366-13.59c9.734 1.468 16.254 13.406 23.049 28.099c5.05 10.468 22.865 25.253 21.672 37.007c-1.47 17.998-21.948 21.396-42.15 10.836z" transform="translate(10)"/>
                <path fill="url(#do_ltd)" d="M201.608 189.119c-3.122 5.877-16.162 15.335-24.886 12.856c-8.815-2.388-12.856-15.795-11.111-25.988c1.653-11.386 11.111-12.03 23.05-6.336c12.855 6.336 16.712 11.662 12.947 19.468" transform="translate(10)"/>
                <path fill="url(#do_lte)" stroke="#E68C3F" strokeWidth="6.251" d="M194.445 253.49c15.06-18.273 48.578-14.508 25.988-39.577c-4.775-5.418-3.306-16.989-9.183-21.947c-6.887-6.061-14.509-1.102-21.488-4.224c-6.979-3.398-14.325-9.918-22.865-5.327c-8.54 4.684-9.459 16.805-10.285 32.783c-.735 11.479-11.203 30.671-5.602 41.231c8.081 16.346 29.11 14.142 43.435-2.938z" transform="translate(10)"/>
                <path fill="url(#do_ltf)" d="M187.925 229.064c23.325-34.435 5.97-34.16.092-36.823c-5.877-2.755-12.03-8.173-18.916-4.408c-6.888 3.857-7.255 13.775-7.439 26.814c-.275 9.367-8.08 25.07-3.397 33.793c5.693 10.193 19.467-4.591 29.66-19.376" transform="translate(10)"/>
                <path fill="url(#do_ltg)" d="M47.06 234.023c-34.895-22.59-18.55-30.303-13.315-33.885c6.336-4.591 6.428-13.407 14.233-12.58c7.806.826 12.397 10.468 17.631 22.406c3.857 8.54 17.264 19.927 16.254 29.753c-1.285 11.57-19.743 3.948-34.803-5.694" transform="translate(10)"/>
                <path fill="#000" d="M209.588 188.843c-2.755 4.776-13.958 12.306-21.396 10.285c-7.622-1.928-11.112-12.672-9.55-20.753c1.377-9.183 9.55-9.642 19.834-5.05c10.928 4.958 14.326 9.182 11.112 15.518"/>
                <path fill="url(#do_lth)" d="M192.058 186.18c-1.745 3.306-9.091 8.54-14.234 7.163c-5.142-1.377-7.713-8.815-6.887-14.417c.735-6.336 6.244-6.704 13.223-3.581c7.53 3.49 9.918 6.428 7.898 10.835" transform="translate(10)"/>
                <path fill="url(#do_lti)" stroke="#E68C3F" strokeWidth="3.75" d="M97.107 66.344c3.673-3.398 12.58-13.774 29.477-2.939c3.122 2.02 5.693 2.204 11.662 4.775c12.03 4.96 6.336 16.897-6.52 20.937c-5.51 1.745-10.468 8.449-20.386 7.806c-8.54-.46-10.744-6.06-15.978-9.091c-9.275-5.234-10.652-12.305-5.602-16.07c5.051-3.765 6.98-5.143 7.347-5.418z" transform="translate(10)"/>
                <path stroke="#E68C3F" strokeWidth="2.5" d="M148.43 75.986c-5.05.275-15.979 11.203-27.457 11.203c-11.479 0-18.366-10.652-20.11-10.652"/>
                <path fill="url(#do_ltj)" d="M102.8 65.426c1.837-1.653 7.622-6.153 15.244-1.562c1.653.919 3.306 1.929 5.693 3.306c4.867 2.847 2.48 6.98-3.398 9.55c-2.663 1.102-7.07 3.49-10.376 3.306c-3.673-.367-6.153-2.755-8.54-4.316c-4.5-2.938-4.224-5.418-2.112-7.346c1.56-1.47 3.305-2.847 3.49-2.938" transform="translate(10)"/>
              </g>
            </svg>
          </span>
          <span className="do-vm-label-text">
            <span className="do-vm-label-os">Linux</span>
            <span className="do-vm-label-tech">Docker</span>
          </span>
        </div>
      </div>

      {/* VM 2 — macOS */}
      <div className="do-vm do-vm-2">
        <div className="do-vm-titlebar">
          <div className="do-dot-sm" style={{ background: "#FF3B4D" }} />
          <div className="do-dot-sm" style={{ background: "#E3B341" }} />
          <div className="do-dot-sm" style={{ background: "#00C781" }} />
        </div>
        <div className="do-vm-body">
          <div
            className="do-mock-bar do-mock-bar-header"
            style={{ width: "45%" }}
          />
          <div
            className="do-mock-bar do-mock-bar-input"
            style={{ width: "100%" }}
          />
          <div
            className="do-mock-bar"
            style={{ width: "55%", marginTop: "4px" }}
          />
          <div className="do-mock-bar" style={{ width: "68%" }} />
          <div className="do-mock-bar" style={{ width: "42%" }} />
          <div
            className="do-mock-bar do-mock-bar-highlight"
            style={{ width: "50%" }}
          />
          <svg className="do-vm-ptr do-vm-ptr-2" viewBox="0 0 24 24" fill="none">
            <path d="M5 3l14 8-6 2-4 6-4-16z" fill="#fff" stroke="#000" strokeWidth="1.5" strokeLinejoin="round" />
          </svg>
        </div>
        <div className="do-vm-flash do-flash-2" />
        <div className="do-vm-status">
          <div className="do-status-running do-status-running-2">
            <div className="do-spinner" />
            <span>Running · Step 5/12</span>
          </div>
          <div className="do-status-pass do-status-pass-2">
            {"✓ PASS · 12 steps · 24s"}
          </div>
        </div>
        <div className="do-vm-label">
          <span className="do-vm-label-icon">
            <svg viewBox="0 0 814 1000" fill="#F9F9F9">
              <path d="M788.1 340.9c-5.8 4.5-108.2 62.2-108.2 190.5 0 148.4 130.3 200.9 134.2 202.2-.6 3.2-20.7 71.9-68.7 141.9-42.8 61.6-87.5 123.1-155.5 123.1s-85.5-39.5-164-39.5c-76.5 0-103.7 40.8-165.9 40.8s-105.6-57-155.5-127C46.7 790.7 0 663 0 541.8c0-194.4 126.4-297.5 250.8-297.5 66.1 0 121.2 43.4 162.7 43.4 39.5 0 101.1-46 176.3-46 28.5 0 130.9 2.6 198.3 99.2zm-234-181.5c31.1-36.9 53.1-88.1 53.1-139.3 0-7.1-.6-14.3-1.9-20.1-50.6 1.9-110.8 33.7-147.1 75.8-28.5 32.4-55.1 83.6-55.1 135.5 0 7.8 1.3 15.6 1.9 18.1 3.2.6 8.4 1.3 13.6 1.3 45.4 0 102.5-30.4 135.5-71.3z"/>
            </svg>
          </span>
          <span className="do-vm-label-text">
            <span className="do-vm-label-os">macOS</span>
            <span className="do-vm-label-tech">Tart VM</span>
          </span>
        </div>
      </div>

      {/* VM 3 — Windows */}
      <div className="do-vm do-vm-3">
        <div className="do-vm-titlebar">
          <div className="do-dot-sm" style={{ background: "#FF3B4D" }} />
          <div className="do-dot-sm" style={{ background: "#E3B341" }} />
          <div className="do-dot-sm" style={{ background: "#00C781" }} />
        </div>
        <div className="do-vm-body">
          <div
            className="do-mock-bar"
            style={{ width: "100%", height: "12px", borderRadius: "6px" }}
          />
          <div
            className="do-mock-bar"
            style={{ width: "35%", marginTop: "6px" }}
          />
          <div
            className="do-mock-bar do-mock-bar-input"
            style={{ width: "75%" }}
          />
          <div
            className="do-mock-bar do-mock-bar-input"
            style={{ width: "75%" }}
          />
          <div
            className="do-mock-bar do-mock-bar-btn"
            style={{ width: "35%", marginTop: "4px" }}
          />
          <svg className="do-vm-ptr do-vm-ptr-3" viewBox="0 0 24 24" fill="none">
            <path d="M5 3l14 8-6 2-4 6-4-16z" fill="#fff" stroke="#000" strokeWidth="1.5" strokeLinejoin="round" />
          </svg>
        </div>
        <div className="do-vm-flash do-flash-3" />
        <div className="do-vm-status">
          <div className="do-status-running do-status-running-3">
            <div className="do-spinner" />
            <span>Running · Step 9/15</span>
          </div>
          <div className="do-status-pass do-status-pass-3">
            {"✓ PASS · 15 steps · 31s"}
          </div>
        </div>
        <div className="do-vm-label">
          <span className="do-vm-label-icon">
            <svg viewBox="0 0 88 88" fill="#00adef">
              <path d="m0 12.402 35.687-4.86.016 34.423-35.67.203zm35.67 33.529.028 34.453L.028 75.48.026 45.7zm4.326-39.025L87.314 0v41.527l-47.318.376zm47.329 39.349-.011 41.34-47.318-6.678-.066-34.739z"/>
            </svg>
          </span>
          <span className="do-vm-label-text">
            <span className="do-vm-label-os">Windows</span>
            <span className="do-vm-label-tech">QEMU / KVM</span>
          </span>
        </div>
      </div>

    </div>
  );
}
