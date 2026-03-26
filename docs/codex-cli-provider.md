# Codex CLI Provider

Use the `codex-cli` provider to run desktest's agent loop through the locally-installed [OpenAI Codex CLI](https://github.com/openai/codex), using your existing ChatGPT login session or `CODEX_API_KEY` instead of configuring API keys in desktest.

## Setup

1. Install Codex CLI: `npm install -g @openai/codex`
2. Authenticate: run `codex login` and complete the login flow, OR set `CODEX_API_KEY` / `OPENAI_API_KEY`
3. Set the provider in your config or task file:

```json
{
  "provider": "codex-cli"
}
```

Or via CLI flag:

```bash
desktest run task.json --provider codex-cli
```

No `api_key` configuration is needed in desktest. The provider shells out to the `codex` binary, which handles authentication.

## How it works

Each agent loop step:

1. Creates a temp directory and saves trajectory screenshots as numbered files (e.g., `step_001_screenshot.png`)
2. Builds a structured prompt with the system prompt embedded inline (in `<system-instructions>` tags) and accessibility trees embedded as labeled text sections
3. Runs `codex exec - --skip-git-repo-check --sandbox danger-full-access -o <output_file> -i <screenshot1> -i <screenshot2> ...`
4. Codex sees the screenshots directly as image attachments (via `-i` flags) — no extra tool calls needed
5. Reads the text response from the output file
6. Cleans up the temp directory (via RAII guard, even on timeout/cancellation)

## Limitations

### Latency overhead per step

Each step spawns a new `codex` process. Expect similar latency to the `claude-cli` provider (~15-20 seconds per step). For a 10-step test, that's roughly 3-4 minutes of LLM time.

### No conversation state between steps

The CLI provider operates in single-shot mode (`codex exec`). There is no persistent conversation session between agent steps. The full sliding window context is rebuilt each step. This means:

- The model sees flattened text rather than structured multi-turn messages
- Token efficiency may be slightly worse than native API calls with proper message arrays

### No turn limit control

Unlike `claude -p --max-turns`, Codex CLI has no equivalent flag. The `codex exec` process runs to completion. The 5-minute internal timeout and the agent loop's `step_timeout` provide safeguards against runaway processes.

### No tool-use passthrough

The `tools` parameter from the agent loop is ignored. The provider uses `--sandbox danger-full-access` to suppress approval prompts but does not expose Codex's shell execution capabilities to the agent loop. This is fine for the standard PyAutoGUI-based agent loop, which doesn't use LLM tool calling.

### Model selection

The provider uses whatever model your Codex CLI is configured with. The `model` field in the desktest config is ignored for this provider. To change the model, use Codex CLI's configuration.

### Not suitable for CI/CD (with ChatGPT auth)

If using ChatGPT login authentication (`codex login`), the provider requires an interactive login session and is intended for local development. For CI/CD, either set `CODEX_API_KEY` or use an API-key-based provider like `openai`.

## When to use this provider

- **Local development and testing** where you want to iterate using your existing Codex/ChatGPT subscription
- **Exploring desktest** without needing to set up API keys first
- **Personal test runs** on your own machine

For production test suites, scheduled runs, or CI/CD pipelines, use an API-key-based provider.
