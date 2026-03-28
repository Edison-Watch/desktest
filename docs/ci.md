# Running Desktest in CI

## Linux Tests (Docker)

Linux tests work in any CI environment with Docker available. No special configuration is needed beyond what you'd use locally.

### GitHub Actions

```yaml
jobs:
  e2e-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install desktest
        run: curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh
      - name: Run tests
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: desktest suite tests/
```

Docker is pre-installed on GitHub Actions `ubuntu-latest` runners. For `--replay` mode (no LLM), you don't need an API key.

### Other CI Providers

Any CI with Docker support works: GitLab CI, CircleCI, Buildkite, Jenkins, etc. The only requirements are Docker and (optionally) an LLM API key.

## macOS Tests (Tart VM) — Planned

macOS tests require **Apple Silicon runners**. This is a hard constraint — Apple's Virtualization.framework only supports macOS guests on ARM64 hardware.

### GitHub Actions

Use `macos-14` or later runners, which run on Apple Silicon (M1):

```yaml
jobs:
  macos-e2e-test:
    runs-on: macos-14  # Apple Silicon (M1)
    steps:
      - uses: actions/checkout@v4
      - name: Install Tart
        run: brew install cirruslabs/cli/tart
      - name: Pull golden image
        run: tart pull ghcr.io/yourorg/macos-test:latest
      - name: Install desktest
        run: curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh
      - name: Run macOS tests
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: desktest suite tests/macos/
```

> **Important**: `macos-13` runners are Intel-based and cannot run macOS VMs via Virtualization.framework. You must use `macos-14` or later.

### Cirrus CI

Cirrus CI offers first-class Tart support via [Cirrus Runners](https://cirrus-runners.app/). Each job gets an ephemeral macOS VM:

```yaml
macos_test_task:
  macos_instance:
    image: ghcr.io/yourorg/macos-test:latest
  install_script: curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh
  test_script: desktest suite tests/macos/
```

### AWS EC2 Mac

EC2 Mac instances (`mac2.metal` for M1, `mac2-m2.metal` for M2) are dedicated Apple hardware. They have a 24-hour minimum allocation period, making them better suited for persistent CI workers than ephemeral jobs.

### MacStadium / Orka

MacStadium provides managed Apple Silicon infrastructure with Kubernetes-style orchestration via Orka. Ephemeral macOS VMs can be provisioned on demand.

### Parallelism Constraints

Apple's macOS SLA limits each physical Mac to **2 concurrent macOS VMs**. This means:

| CI Setup | Max Parallel macOS Tests |
|----------|------------------------|
| 1 GitHub Actions runner | 2 |
| 1 EC2 Mac instance | 2 |
| N Mac minis (self-hosted) | 2N |

For Linux tests, there is no such limit — Docker containers scale freely.

### Golden Image Preparation

Your CI pipeline needs a pre-built Tart golden image with:
- macOS with TCC permissions configured (Accessibility + Screen Recording)
- Python 3 + PyAutoGUI installed
- The desktest Swift accessibility helper installed
- SSH configured for desktest connectivity
- Optionally, the app(s) under test pre-installed

See [macOS Support](macos-support.md) for golden image setup instructions.

## Windows Tests — Planned

Windows VM support is planned. Expected to work with any CI environment that can run Windows VMs (QEMU/libvirt, Hyper-V). Details TBD.
