import React, { useState, useEffect } from "react";

const CYCLE_MS = 8000;

const ASCII_ART = [
  " ██████╗ ███████╗███████╗██╗  ██╗████████╗███████╗███████╗████████╗",
  " ██╔══██╗██╔════╝██╔════╝██║ ██╔╝╚══██╔══╝██╔════╝██╔════╝╚══██╔══╝",
  " ██║  ██║█████╗  ███████╗█████╔╝    ██║   █████╗  ███████╗   ██║",
  " ██║  ██║██╔══╝  ╚════██║██╔═██╗    ██║   ██╔══╝  ╚════██║   ██║",
  " ██████╔╝███████╗███████║██║  ██╗   ██║   ███████╗███████║   ██║",
  " ╚═════╝ ╚══════╝╚══════╝╚═╝  ╚═╝   ╚═╝   ╚══════╝╚══════╝   ╚═╝",
].join("\n");

export default function DesktestClosingAnimation() {
  const [cycle, setCycle] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setCycle((c) => c + 1), CYCLE_MS);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="dcl-scene" key={cycle}>
      <style>{`
        .dcl-scene {
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
        }

        .dcl-glow {
          position: absolute;
          width: 400px;
          height: 400px;
          border-radius: 50%;
          background: radial-gradient(circle, rgba(195, 255, 253, 0.06) 0%, transparent 70%);
          top: 50%;
          left: 50%;
          transform: translate(-50%, -50%);
          animation: dcl-pulse 3s ease-in-out infinite;
          pointer-events: none;
        }

        .dcl-ascii-wrap {
          opacity: 0;
          animation: dcl-fade-in 600ms ease-out 200ms forwards;
          width: 95%;
          max-width: 1400px;
          display: flex;
          justify-content: center;
          overflow: hidden;
        }

        .dcl-ascii {
          margin: 0;
          font-size: clamp(10px, 1.7vw, 22px);
          line-height: 1.3;
          color: #C3FFFD;
          font-weight: 700;
          font-family: inherit;
          white-space: pre;
        }

        .dcl-tagline {
          font-size: 36px;
          color: #9BA4A6;
          opacity: 0;
          animation: dcl-fade-in 500ms ease-out 700ms forwards;
        }

        .dcl-tagline-em {
          color: #F9F9F9;
          font-weight: 700;
        }

        .dcl-divider {
          width: 60px;
          height: 1px;
          background: #383838;
          opacity: 0;
          animation: dcl-fade-in 400ms ease-out 1100ms forwards;
        }

        .dcl-gh {
          display: flex;
          align-items: center;
          gap: 10px;
          opacity: 0;
          animation: dcl-fade-in 500ms ease-out 1400ms forwards;
        }

        .dcl-gh-icon {
          width: 36px;
          height: 36px;
          fill: #F9F9F9;
        }

        .dcl-gh-url {
          font-size: 32px;
          color: #C3FFFD;
          font-weight: 700;
          letter-spacing: 0.3px;
        }

        .dcl-star {
          display: flex;
          align-items: center;
          gap: 6px;
          font-size: 28px;
          color: #9BA4A6;
          opacity: 0;
          animation: dcl-fade-in 400ms ease-out 1800ms forwards;
        }

        .dcl-star-icon {
          color: #E3B341;
        }

        .dcl-license {
          font-size: 26px;
          color: #616061;
          opacity: 0;
          animation: dcl-fade-in 400ms ease-out 2200ms forwards;
        }

        .dcl-license-em {
          color: #C3FFFD;
          font-weight: 700;
        }

        @keyframes dcl-fade-in {
          from { opacity: 0; transform: translateY(8px); }
          to   { opacity: 1; transform: translateY(0); }
        }

        @keyframes dcl-pulse {
          0%, 100% { transform: translate(-50%, -50%) scale(1); opacity: 0.5; }
          50%      { transform: translate(-50%, -50%) scale(1.15); opacity: 1; }
        }

        @media (prefers-reduced-motion: reduce) {
          .dcl-ascii-wrap, .dcl-tagline, .dcl-divider, .dcl-gh, .dcl-star, .dcl-license {
            animation: none;
            opacity: 1;
          }
          .dcl-glow { animation: none; }
        }
      `}</style>

      <div className="dcl-glow" />

      <div className="dcl-ascii-wrap">
        <pre className="dcl-ascii">{ASCII_ART}</pre>
      </div>

      <div className="dcl-tagline">
        <span className="dcl-tagline-em">Computer use CLI</span> for scalable E2E desktop testing
      </div>

      <div className="dcl-divider" />

      <div className="dcl-gh">
        <svg className="dcl-gh-icon" viewBox="0 0 24 24">
          <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z" />
        </svg>
        <span className="dcl-gh-url">github.com/Edison-Watch/desktest</span>
      </div>

      <div className="dcl-star">
        <span className="dcl-star-icon">{"★"}</span>
        <span>Star us on GitHub</span>
      </div>

      <div className="dcl-license">
        Open source — <span className="dcl-license-em">MIT License</span>
      </div>
    </div>
  );
}
