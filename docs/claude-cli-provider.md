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

1. Flattens the message array (system prompt, trajectory, current observation) into a single text prompt
2. Saves any base64 screenshots to temporary PNG files in `/tmp/`
3. Runs `claude -p --output-format json` with the prompt piped via stdin
4. When screenshots are present, passes `--allowedTools Read --max-turns 2` so Claude can view the image file
5. Parses the JSON response and returns it to the agent loop
6. Cleans up temporary files

## Limitations

### Latency overhead per step

Each step spawns a new `claude` process. This adds roughly 2-5 seconds of overhead per step compared to direct API calls. For a 15-step test, expect an additional 30-75 seconds of wall-clock time.

### No conversation state between steps

The CLI provider operates in single-shot mode (`claude -p`). There is no persistent conversation session between agent steps. The full sliding window context is rebuilt and passed as a single prompt each step. This means:

- The model sees flattened text rather than structured multi-turn messages
- Previous screenshots from the trajectory are described in text, not re-sent as images — only the **current** screenshot is passed as an actual image file
- Token efficiency may be slightly worse than native API calls with proper message arrays

### Image handling requires an extra turn

When a screenshot is present, the provider uses `--max-turns 2` and `--allowedTools Read` so Claude can read the temp image file. This adds one extra tool-use round-trip per step compared to the API providers, which embed images directly in the request.

### No tool-use passthrough

The `tools` parameter from the agent loop is ignored. The provider only uses Claude Code's built-in `Read` tool for viewing screenshots. This is fine for the standard PyAutoGUI-based agent loop, which doesn't use LLM tool calling.

### Model selection

The provider uses whatever model your Claude Code CLI is configured with. The `model` field in the desktest config is ignored for this provider. To change the model, configure it in Claude Code's settings.

### Not suitable for CI/CD

The `claude-cli` provider requires an interactive Claude Code login session. It is intended for local development and testing, not headless CI/CD environments. For CI/CD, use the `anthropic`, `openai`, or other API-key-based providers.

## When to use this provider

- **Local development and testing** where you want to iterate quickly without API costs
- **Exploring desktest** without needing to set up API keys first
- **Personal test runs** on your own machine

For production test suites, scheduled runs, or CI/CD pipelines, use an API-key-based provider.
