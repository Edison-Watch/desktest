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

## macOS Tests (Tart VM)

macOS tests require **Apple Silicon runners**. This is a hard constraint — Apple's Virtualization.framework only supports macOS guests on ARM64 hardware.

### GitHub Actions

Use `macos-14` or later runners, which run on Apple Silicon (M1):

```yaml
jobs:
  macos-e2e-test:
    runs-on: macos-14  # Apple Silicon (M1)
    steps:
      - uses: actions/checkout@v4
      - name: Install dependencies
        run: |
          brew install cirruslabs/cli/tart
          brew install hudochenkov/sshpass/sshpass
      - name: Install desktest
        run: curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh
      - name: Prepare golden image
        run: desktest init-macos
      - name: Run macOS tests
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: desktest suite tests/macos/
```

> **Important**: `macos-13` runners are Intel-based and cannot run macOS VMs via Virtualization.framework. You must use `macos-14` or later.

> **Tip**: Cache the golden image between runs to avoid re-provisioning. The Tart VM images are stored in `~/.tart/vms/`. Alternatively, push the golden image to a container registry with `tart push` and pull it in CI.

### Cirrus CI

Cirrus CI offers Tart support via [Cirrus Runners](https://cirrus-runners.app/) on self-hosted bare-metal Apple Silicon Macs. Note that `macos_tart` mode requires **bare-metal** runners (not `macos_instance` VMs) because Apple's Virtualization.framework does not support nested macOS virtualization on M1/M2 chips (M3+ with macOS 15+ may support it).

For `macos_native` mode (no nested VM), a standard `macos_instance` works. For `macos_tart` mode, use a self-hosted Cirrus Runner:

```yaml
# macos_native mode (runs directly in CI VM, no isolation)
macos_native_task:
  macos_instance:
    image: ghcr.io/yourorg/macos-test:latest
  install_script: curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh
  test_script: desktest suite tests/macos-native/

# macos_tart mode (requires self-hosted bare-metal Apple Silicon runner)
# macos_instance runs inside a Cirrus-managed VM, which does NOT support
# nested macOS virtualization on M1/M2. Use a persistent worker instead.
macos_tart_task:
  persistent_worker:
    labels:
      os: darwin
      arch: arm64
  install_script: |
    brew install cirruslabs/cli/tart
    curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh
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

Run `desktest init-macos` to create a golden image. This automatically installs and configures:
- Python 3 + PyAutoGUI (Quartz backend)
- Swift accessibility helper (`a11y-helper`) with TCC Accessibility grants
- PyAutoGUI action executor (`execute-action`)
- VM agent (LaunchAgent with Homebrew PATH)
- Passwordless SSH keys for localhost (required for a11y tree extraction)
- TCC permissions with proper code signing requirement (`csreq`) blobs
- Homebrew PATH in `/etc/paths.d`

The base image must have SIP disabled (Cirrus Labs base images ship with this). For Electron app testing, add `--with-electron` to install Node.js.

```bash
# Basic golden image
desktest init-macos

# With Electron support
desktest init-macos --with-electron
```

See [macOS Support](macos-support.md) for details on TCC permissions and the SSH localhost workaround for accessibility.

## Windows Tests (QEMU/KVM)

Windows tests require a **Linux host with KVM** (hardware virtualization). Software emulation (TCG) is too slow for interactive desktop testing.

### GitHub Actions

Standard free-tier `ubuntu-latest` runners do **not** expose `/dev/kvm`. Use larger runners (4+ vCPU) or self-hosted runners:

```yaml
jobs:
  windows-e2e-test:
    # IMPORTANT: Standard ubuntu-latest runners do NOT have KVM.
    # Use a larger runner or self-hosted runner with KVM access.
    runs-on: ubuntu-latest-4-cores  # Example: 4-core larger runner
    steps:
      - uses: actions/checkout@v4
      - name: Check KVM availability
        run: |
          if [ ! -e /dev/kvm ]; then
            echo "::error::KVM is not available. Use a larger runner or self-hosted runner."
            exit 1
          fi
          sudo chmod 666 /dev/kvm
      - name: Install dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y qemu-system-x86 qemu-utils ovmf swtpm virtiofsd
      - name: Install desktest
        run: curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh
      - name: Download golden image
        run: |
          # Download from your org's storage (S3, GCS, artifact registry, etc.)
          # The golden image is ~15-25 GB — cache or compress with zstd
          echo "Configure golden image download here"
      - name: Run Windows tests
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: desktest suite tests/windows/
```

### Golden Image Preparation

Run `desktest init-windows` once to create a QCOW2 golden image. This two-stage process installs Windows from ISO (unattended) and provisions all dependencies via SSH:

```bash
# Requires: Windows 11 ISO + VirtIO driver ISO (user provides both)
desktest init-windows \
  --windows-iso Win11.iso \
  --virtio-iso virtio-win.iso \
  --output desktest-windows.qcow2
```

The golden image includes: Python 3, PyAutoGUI, uiautomation (UIA), WinFsp + VirtIO-FS, vm-agent (scheduled task), auto-login, disabled UAC/Defender/Windows Update.

Store the golden image in cloud storage or a shared artifact registry. Each test creates a QCOW2 overlay (copy-on-write), so the base image is never modified.

### Self-Hosted Runner Setup

For best performance, use a dedicated Linux machine or cloud VM with nested virtualization:

```bash
# Install dependencies
sudo apt install qemu-system-x86 qemu-utils ovmf swtpm virtiofsd

# Grant KVM access
sudo usermod -aG kvm github-runner

# Build golden image once
desktest init-windows --windows-iso Win11.iso --virtio-iso virtio-win.iso
```

See [Windows CI Guide](../dev-docs/windows-ci-guide.md) for detailed setup instructions, cloud provider compatibility, caching strategies, and troubleshooting.
