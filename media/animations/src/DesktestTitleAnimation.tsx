import React, { useState, useEffect } from "react";

const CYCLE_MS = 7000;

const ASCII_ART = [
  " ██████╗ ███████╗███████╗██╗  ██╗████████╗███████╗███████╗████████╗",
  " ██╔══██╗██╔════╝██╔════╝██║ ██╔╝╚══██╔══╝██╔════╝██╔════╝╚══██╔══╝",
  " ██║  ██║█████╗  ███████╗█████╔╝    ██║   █████╗  ███████╗   ██║",
  " ██║  ██║██╔══╝  ╚════██║██╔═██╗    ██║   ██╔══╝  ╚════██║   ██║",
  " ██████╔╝███████╗███████║██║  ██╗   ██║   ███████╗███████║   ██║",
  " ╚═════╝ ╚══════╝╚══════╝╚═╝  ╚═╝   ╚═╝   ╚══════╝╚══════╝   ╚═╝",
].join("\n");

export default function DesktestTitleAnimation() {
  const [cycle, setCycle] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setCycle((c) => c + 1), CYCLE_MS);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="dt-scene" key={cycle}>
      <style>{`
        .dt-scene {
          position: relative;
          width: 100%;
          aspect-ratio: 16 / 9;
          background: #000;
          display: flex;
          align-items: center;
          justify-content: center;
          overflow: hidden;
          font-family: 'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace;
        }

        .dt-perspective {
          perspective: 1200px;
          width: 90%;
          max-width: 1600px;
          transform: scale(1.1);
        }

        .dt-sway {
          transform-style: preserve-3d;
          animation: dt-sway 4s ease-in-out infinite;
          animation-delay: 1.6s;
        }

        .dt-terminal {
          border-radius: 16px;
          border: 1px solid #383838;
          overflow: hidden;
          box-shadow:
            0 25px 80px rgba(0, 0, 0, 0.9),
            0 10px 30px rgba(0, 0, 0, 0.6);
          opacity: 0;
          animation: dt-fade-in 67ms ease-out forwards;
        }

        .dt-titlebar {
          background: #2F3440;
          border-bottom: 1px solid #383838;
          padding: 14px 16px;
          display: flex;
          gap: 8px;
          align-items: center;
        }

        .dt-dot {
          width: 14px;
          height: 14px;
          border-radius: 50%;
        }

        .dt-body {
          background: #1C1C1C;
          padding: 24px 30px 28px;
        }

        .dt-prompt-line {
          display: flex;
          align-items: center;
          font-size: 28px;
          line-height: 1.5;
        }

        .dt-prompt {
          color: #C3FFFD;
          font-weight: 700;
        }

        .dt-typed {
          color: #F9F9F9;
          overflow: hidden;
          white-space: nowrap;
          display: inline-block;
          width: 0;
          animation: dt-type 267ms steps(8) 70ms forwards;
          font-family: inherit;
        }

        .dt-cursor {
          display: inline-block;
          width: 10px;
          height: 22px;
          background: #C3FFFD;
          margin-left: 1px;
          vertical-align: text-bottom;
          opacity: 0;
          animation: dt-blink 534ms linear 70ms 1 forwards;
        }

        .dt-banner-wrap {
          display: grid;
          grid-template-rows: 0fr;
          animation: dt-expand 800ms cubic-bezier(0.16, 1, 0.3, 1) 600ms forwards;
        }

        .dt-banner-inner {
          overflow: hidden;
          min-height: 0;
          perspective: 1200px;
        }

        .dt-banner {
          opacity: 0;
          transform: translateY(400px) rotateX(22deg);
          transform-origin: center top;
          animation: dt-spring-in 1000ms linear 600ms both;
          padding-top: 16px;
        }

        .dt-bordered-box {
          border: 3px solid #C3FFFD;
          border-radius: 8px;
          padding: 20px 24px;
          margin-bottom: 16px;
        }

        .dt-ascii {
          margin: 0;
          font-size: 16px;
          line-height: 1.25;
          color: #F9F9F9;
          font-weight: 700;
          font-family: inherit;
          white-space: pre;
        }

        .dt-version {
          margin-top: 12px;
          font-size: 15px;
        }

        .dt-version-bold {
          color: #F9F9F9;
          font-weight: 700;
        }

        .dt-version-grey {
          color: #9BA4A6;
        }

        .dt-separator {
          color: #C3FFFD;
          opacity: 0.5;
          margin: 12px 0;
          font-size: 14px;
          overflow: hidden;
          white-space: nowrap;
        }

        .dt-description {
          color: #9BA4A6;
          font-size: 15px;
          line-height: 1.5;
          margin-bottom: 16px;
        }

        .dt-usage {
          font-size: 15px;
          color: #9BA4A6;
          margin-bottom: 16px;
        }

        .dt-usage-label {
          color: #C3FFFD;
          font-weight: 700;
          text-decoration: underline;
          text-underline-offset: 3px;
        }

        .dt-workflows-label {
          color: #C3FFFD;
          font-weight: 700;
          text-decoration: underline;
          text-underline-offset: 3px;
          font-size: 15px;
          margin-bottom: 6px;
        }

        .dt-workflow-line {
          font-size: 15px;
          line-height: 1.8;
          white-space: nowrap;
        }

        .dt-wf-cmd {
          color: #C3FFFD;
        }

        .dt-wf-args {
          color: #F9F9F9;
        }

        .dt-wf-comment {
          color: #9BA4A6;
          opacity: 0.5;
        }


        @keyframes dt-fade-in {
          from { opacity: 0; }
          to { opacity: 1; }
        }

        @keyframes dt-type {
          from { width: 0; }
          to { width: 8ch; }
        }

        @keyframes dt-blink {
          0%, 49.9% { opacity: 1; }
          50%, 100% { opacity: 0; }
        }

        @keyframes dt-expand {
          to { grid-template-rows: 1fr; }
        }

        @keyframes dt-spring-in {
          0%   { transform: translateY(400px) rotateX(22deg); opacity: 0; }
          5%   { opacity: 1; }
          10%  { transform: translateY(288px) rotateX(15.8deg); opacity: 1; }
          20%  { transform: translateY(144px) rotateX(7.9deg); opacity: 1; }
          30%  { transform: translateY(56px)  rotateX(3.1deg); opacity: 1; }
          40%  { transform: translateY(16px)  rotateX(0.9deg); opacity: 1; }
          50%  { transform: translateY(2px)   rotateX(0.1deg); opacity: 1; }
          55%  { transform: translateY(0px)   rotateX(0deg); opacity: 1; }
          60%  { transform: translateY(-1px)  rotateX(-0.07deg); opacity: 1; }
          65%  { transform: translateY(-1px)  rotateX(-0.07deg); opacity: 1; }
          100% { transform: translateY(0px)   rotateX(0deg); opacity: 1; }
        }

        @keyframes dt-sway {
          0%, 100% { transform: rotateY(0deg); }
          25%      { transform: rotateY(1.2deg); }
          75%      { transform: rotateY(-1.2deg); }
        }


        @media (prefers-reduced-motion: reduce) {
          .dt-terminal     { animation: none; opacity: 1; }
          .dt-typed        { animation: none; width: 8ch; }
          .dt-cursor       { animation: none; opacity: 0; }
          .dt-banner-wrap  { animation: none; grid-template-rows: 1fr; }
          .dt-banner       { animation: none; opacity: 1; transform: none; }
          .dt-sway         { animation: none; }
        }
      `}</style>

      <div className="dt-perspective">
        <div className="dt-sway">
          <div className="dt-terminal">
            <div className="dt-titlebar">
              <div className="dt-dot" style={{ background: "#FF3B4D" }} />
              <div className="dt-dot" style={{ background: "#E3B341" }} />
              <div className="dt-dot" style={{ background: "#00C781" }} />
            </div>
            <div className="dt-body">
              <div className="dt-prompt-line">
                <span className="dt-prompt">$&nbsp;</span>
                <span className="dt-typed">desktest</span>
                <span className="dt-cursor" />
              </div>
              <div className="dt-banner-wrap">
                <div className="dt-banner-inner">
                  <div className="dt-banner">
                    <div className="dt-bordered-box">
                      <pre className="dt-ascii">{ASCII_ART}</pre>
                      <div className="dt-version">
                        <span className="dt-version-bold">
                          Desktest CLI v0.21.0 (9b71709)
                        </span>
                        <span className="dt-version-grey">
                          {" — Playwright for full-computer tests"}
                        </span>
                      </div>
                    </div>
                    <div className="dt-separator">{"─ ".repeat(50)}</div>
                    <div className="dt-description">
                      Automated end-to-end testing for Linux, macOS, and
                      Windows desktop apps using LLM-powered agents
                    </div>
                    <div className="dt-usage">
                      <span className="dt-usage-label">Usage:</span>
                      {" desktest [OPTIONS] [COMMAND]"}
                    </div>
                    <div className="dt-workflows-label">WORKFLOWS:</div>
                    <div className="dt-workflow-line">
                      <span className="dt-wf-cmd">{"▸ desktest"}</span>
                      <span className="dt-wf-args">
                        {" run task.json --monitor"}
                      </span>
                      <span className="dt-wf-comment">
                        {"    # Watch the agent explore"}
                      </span>
                    </div>
                    <div className="dt-workflow-line">
                      <span className="dt-wf-cmd">{"▸ desktest"}</span>
                      <span className="dt-wf-args">
                        {" codify trajectory.jsonl"}
                      </span>
                      <span className="dt-wf-comment">
                        {"    # Convert to deterministic script"}
                      </span>
                    </div>
                    <div className="dt-workflow-line">
                      <span className="dt-wf-cmd">{"▸ desktest"}</span>
                      <span className="dt-wf-args">
                        {" run task.json --replay"}
                      </span>
                      <span className="dt-wf-comment">
                        {"     # Replay in CI (no LLM)"}
                      </span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

    </div>
  );
}
