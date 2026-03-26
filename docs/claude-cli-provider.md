# Claude CLI Provider

Use the `claude-cli` provider to run desktest's agent loop through the locally-installed [Claude Code](https://claude.ai/code) CLI, using your existing CLI authentication instead of a separate API key.

## Setup

1. Install Claude Code: https://claude.ai/code
2. Authenticate: run `claude` once and complete the login flow
3. Set the provider in your config or task file:

```json
{
  "provider": "claude-cli"
}
```

Or via CLI flag:

```bash
desktest run task.json --provider claude-cli
```

No `api_key`, `ANTHROPIC_API_KEY`, or any other key is needed. The provider shells out to the `claude` binary, which handles authentication.

## How it works

Each agent loop step:

1. Creates a temp directory and saves all trajectory screenshots and accessibility trees as numbered files (e.g., `step_001_screenshot.png`, `step_001_a11y.txt`)
2. Builds a structured prompt with the system prompt embedded inline (in `<system-instructions>` tags) and an explicit file manifest listing each observation file by exact path
3. Runs `claude -p --output-format text --allowedTools Read` with `--max-turns` scaled to the number of files
4. Claude reads the observation files in order via the Read tool, gaining full visual context of the trajectory
5. Returns the text response to the agent loop
6. Cleans up the temp directory (via RAII guard, even on timeout/cancellation)

## Limitations

### Latency overhead per step

Each step spawns a new `claude` process and Claude reads observation files via tool calls. Expect ~15-20 seconds per step. For a 10-step test, that's roughly 3-4 minutes of LLM time.

### No conversation state between steps

The CLI provider operates in single-shot mode (`claude -p`). There is no persistent conversation session between agent steps. The full sliding window context is rebuilt each step. This means:

- The model sees flattened text rather than structured multi-turn messages
- Token efficiency may be slightly worse than native API calls with proper message arrays

### No tool-use passthrough

The `tools` parameter from the agent loop is ignored. The provider only uses Claude Code's built-in `Read` tool for viewing observation files. This is fine for the standard PyAutoGUI-based agent loop, which doesn't use LLM tool calling.

### Model selection

The provider uses whatever model your Claude Code CLI is configured with. The `model` field in the desktest config is ignored for this provider. To change the model, configure it in Claude Code's settings.

### Not suitable for CI/CD

The `claude-cli` provider requires an interactive Claude Code login session. It is intended for local development and testing, not headless CI/CD environments. For CI/CD, use the `anthropic`, `openai`, or other API-key-based providers.

## When to use this provider

- **Local development and testing** where you want to iterate quickly without API costs
- **Exploring desktest** without needing to set up API keys first
- **Personal test runs** on your own machine

For production test suites, scheduled runs, or CI/CD pipelines, use an API-key-based provider.
