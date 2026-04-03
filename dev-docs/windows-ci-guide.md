# Windows CI/CD Integration Guide

This guide covers running desktest Windows VM tests in CI/CD pipelines, with a focus on GitHub Actions.

## Prerequisites

A Linux host with:

| Dependency | Install (Debian/Ubuntu) | Purpose |
|-----------|------------------------|---------|
| QEMU | `sudo apt install qemu-system-x86 qemu-utils` | VM hypervisor |
| KVM | Kernel module (`kvm_intel` or `kvm_amd`) | Hardware acceleration (required) |
| OVMF | `sudo apt install ovmf` | UEFI firmware for Windows 11 |
| swtpm | `sudo apt install swtpm` | Software TPM 2.0 (Windows 11 requirement) |
| virtiofsd | `sudo apt install virtiofsd` | VirtIO-FS shared directory daemon |
| genisoimage | `sudo apt install genisoimage` | ISO creation for `init-windows` |
| sshpass | `sudo apt install sshpass` | SSH password auth for provisioning |

## Golden Image Preparation

The golden image must be built **once** (locally or in a dedicated CI job), then cached for test runs.

```bash
# Download prerequisites (user provides their own Windows ISO)
# VirtIO drivers: https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/stable-virtio/

# Build the golden image (~30-60 minutes)
desktest init-windows \
  --windows-iso /path/to/Win11.iso \
  --virtio-iso /path/to/virtio-win.iso \
  --output desktest-windows.qcow2 \
  --disk-size 64G \
  --ram 4G \
  --cpus 4
```

The resulting `desktest-windows.qcow2` is ~15-25 GB. Store it in:
- Cloud storage (S3, GCS, Azure Blob) — recommended for CI
- GitHub Actions cache (limited to 10 GB per cache entry — may need compression)
- Artifact registry or shared NFS mount

## Running Tests

```bash
# Single test
desktest run examples/windows-calculator.json

# Test suite (filters Windows-only tasks)
desktest suite ./tests --filter windows
```

Each test creates a QCOW2 overlay (copy-on-write) from the golden image, so tests don't modify the base image and can run in parallel on different machines.

## GitHub Actions

### Requirements

**KVM access is mandatory.** QEMU without KVM is too slow for interactive desktop testing (~100x slower). This limits your runner options:

| Runner Type | KVM Support | Notes |
|------------|-------------|-------|
| `ubuntu-latest` (free tier, 2 vCPU) | No | Standard runners do not expose `/dev/kvm` |
| `ubuntu-latest` (4+ vCPU, larger runners) | Yes | Larger runners support nested virtualization |
| Self-hosted runners (bare metal) | Yes | Full KVM support, best performance |
| Self-hosted runners (cloud VM) | Maybe | Requires nested virtualization enabled on the cloud VM |

### Example Workflow

```yaml
# .github/workflows/windows-e2e.yml
#
# LIMITATIONS:
# - Requires a larger runner (4+ vCPU) or self-hosted runner with KVM access.
#   Standard free-tier GitHub-hosted runners do NOT provide /dev/kvm.
# - The golden image (~15-25 GB) must be pre-built and cached externally.
#   Building it in CI is possible but adds 30-60 minutes to each fresh run.
# - Windows ISO is not included — your org must provide and store it.
# - Each test boots a full Windows VM (~30-60s startup), so these tests are
#   slower than Docker-based Linux tests.
# - Nested virtualization performance varies by cloud provider. Bare-metal
#   self-hosted runners give the best and most consistent performance.

name: Windows E2E Tests
on:
  push:
    branches: [main]
  pull_request:

jobs:
  windows-e2e:
    # IMPORTANT: Standard ubuntu-latest runners do NOT have KVM.
    # Use a larger runner or self-hosted runner with KVM access.
    runs-on: ubuntu-latest-4-cores  # Example: 4-core larger runner
    timeout-minutes: 30

    steps:
      - uses: actions/checkout@v4

      - name: Check KVM availability
        run: |
          if [ ! -e /dev/kvm ]; then
            echo "::error::KVM is not available on this runner. Windows VM tests require KVM."
            echo "Use a larger runner (4+ vCPU) or self-hosted runner with KVM access."
            exit 1
          fi
          sudo chmod 666 /dev/kvm

      - name: Install dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            qemu-system-x86 qemu-utils ovmf swtpm virtiofsd

      - name: Install desktest
        run: |
          # Install from GitHub releases (adjust version as needed)
          curl -fsSL https://github.com/Edison-Watch/desktest/releases/latest/download/desktest-linux-amd64 \
            -o /usr/local/bin/desktest
          chmod +x /usr/local/bin/desktest

      - name: Download golden image
        run: |
          # IMPORTANT: Replace with your actual golden image storage location.
          # Options:
          #   - S3/GCS bucket: aws s3 cp s3://your-bucket/desktest-windows.qcow2 .
          #   - GitHub release asset: gh release download ...
          #   - Shared NFS: cp /mnt/shared/desktest-windows.qcow2 .
          #
          # The golden image is ~15-25 GB. Consider using QCOW2 compression
          # or zstd to reduce transfer time:
          #   qemu-img convert -c -O qcow2 input.qcow2 compressed.qcow2
          echo "::error::Golden image download not configured. See comments above."
          exit 1

      - name: Run Windows Calculator test
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: |
          desktest run examples/windows-calculator.json \
            --artifacts-dir ./artifacts

      - name: Upload test artifacts
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: windows-e2e-artifacts
          path: ./artifacts/
          retention-days: 7
```

### Self-Hosted Runner Setup

For the best experience, use a dedicated Linux machine or cloud VM:

```bash
# 1. Verify KVM is available
ls -la /dev/kvm
# If missing: check BIOS for VT-x/AMD-V, then load the module:
# sudo modprobe kvm_intel  (or kvm_amd)

# 2. Install dependencies
sudo apt install qemu-system-x86 qemu-utils ovmf swtpm virtiofsd

# 3. Grant KVM access to the runner user
sudo usermod -aG kvm github-runner

# 4. Build golden image once
desktest init-windows \
  --windows-iso Win11.iso \
  --virtio-iso virtio-win.iso \
  --output /opt/desktest/desktest-windows.qcow2

# 5. Install GitHub Actions runner
# https://docs.github.com/en/actions/hosting-your-own-runners
```

### Cloud VM Nested Virtualization

If using cloud VMs as self-hosted runners:

| Cloud | Instance Type | Nested Virt |
|-------|--------------|-------------|
| GCP | N2 or C2 family | `--enable-nested-virtualization` (must be enabled at VM creation) |
| AWS | `.metal` instances | Native KVM (bare metal, not nested) |
| Azure | Dv3/Ev3 family | Supported by default on most sizes |

## Caching the Golden Image

The golden image is large (~15-25 GB) but changes infrequently. Cache strategies:

1. **Cloud storage** (recommended): Upload to S3/GCS once, download in CI. Use lifecycle rules for cleanup.
2. **QCOW2 compression**: `qemu-img convert -c -O qcow2 input.qcow2 compressed.qcow2` reduces size by ~30-50%.
3. **zstd compression**: `zstd -T0 desktest-windows.qcow2` for fast decompression in CI.
4. **Rebuild on schedule**: Use a weekly CI job to rebuild the golden image, keeping dependencies fresh.

## Troubleshooting

| Issue | Solution |
|-------|----------|
| `/dev/kvm` not found | Enable VT-x/AMD-V in BIOS; `sudo modprobe kvm_intel` |
| Permission denied on `/dev/kvm` | `sudo chmod 666 /dev/kvm` or `sudo usermod -aG kvm $USER` |
| QEMU boot hangs | Check OVMF is installed: `ls /usr/share/OVMF/OVMF_CODE.fd` |
| SSH timeout during provisioning | Windows may still be configuring after first boot. Increase timeout or check VirtIO network driver is installed. |
| VirtIO-FS not mounting as Z:\ | Verify WinFsp is installed in the golden image and `WinFsp.Launcher` service is running |
| Tests very slow | Confirm KVM is active: `grep -c vmx /proc/cpuinfo` (Intel) or `grep -c svm /proc/cpuinfo` (AMD). Without KVM, QEMU falls back to TCG (software emulation). |
