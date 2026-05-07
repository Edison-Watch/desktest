import { chromium } from "playwright";
import { execSync } from "child_process";
import { existsSync, mkdirSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT_DIR = resolve(__dirname, "..", "recordings");
const STORYBOOK_URL =
  "http://localhost:6020/iframe.html?id=animations-desktestlaunch--default&viewMode=story";
const RECORD_MS = 79_500;
const TRIM_DURATION = "00:01:18";
const WIDTH = 1920;
const HEIGHT = 1080;

if (!existsSync(OUT_DIR)) mkdirSync(OUT_DIR, { recursive: true });

console.log("Launching browser…");
const browser = await chromium.launch();
const context = await browser.newContext({
  viewport: { width: WIDTH, height: HEIGHT },
  recordVideo: {
    dir: OUT_DIR,
    size: { width: WIDTH, height: HEIGHT },
  },
});

const page = await context.newPage();
console.log("Navigating to Storybook…");
await page.goto(STORYBOOK_URL, { waitUntil: "networkidle" });
await page.waitForTimeout(500);

console.log(`Recording for ${RECORD_MS / 1000}s…`);
await page.waitForTimeout(RECORD_MS);

console.log("Stopping recording…");
await page.close();
const videoPath = await page.video()?.path();
await context.close();
await browser.close();

if (!videoPath) {
  console.error("No video file produced.");
  process.exit(1);
}

const mp4Path = resolve(OUT_DIR, "desktest-launch.mp4");
console.log(`Converting ${videoPath} → ${mp4Path}`);
execSync(
  `ffmpeg -y -i "${videoPath}" -t ${TRIM_DURATION} -c:v libx264 -preset slow -crf 22 -pix_fmt yuv420p -movflags +faststart "${mp4Path}"`,
  { stdio: "inherit" }
);

console.log(`\nDone! Video saved to: ${mp4Path}`);
