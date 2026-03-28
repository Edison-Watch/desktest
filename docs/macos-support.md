# macOS Support (Planned)

> **Status:** This feature is planned but not yet implemented. This document describes the design, constraints, and known limitations.

Desktest's macOS support enables E2E testing of native macOS desktop applications using the same LLM-powered agent loop used for Linux. macOS support requires Apple Silicon hardware and uses [Tart](https://github.com/cirruslabs/tart) VMs for clean, isolated test environments.

## Architecture

On Linux, desktest runs tests inside Docker containers with a virtual X11 desktop. On macOS, Docker cannot host macOS guests (both technically and legally), so desktest uses **Tart VMs** — lightweight macOS virtual machines powered by Apple's Virtualization.framework.

```
Linux (current)                    macOS (planned)
─────────────────                  ─────────────────
Docker container                   Tart macOS VM
  Xvfb + XFCE desktop               Native macOS desktop
  PyAutoGUI (X11/python-xlib)        PyAutoGUI (Quartz/CoreGraphics)
  pyatspi (AT-SPI2)                  Swift a11y helper (AXUIElement)
  scrot (screenshot)                 screencapture (screenshot)
  ffmpeg x11grab (recording)         screencapture -V (recording)
```

### Two Operating Modes

| Mode | Environment | Isolation | Use Case |
|------|-------------|-----------|----------|
| `macos_tart` | Tart VM (cloned from golden image) | Full VM isolation, destroyed after test | CI, reproducible tests |
| `macos_native` | Host macOS desktop (no VM) | None — runs on your real desktop | Quick local iteration, debugging |

## Requirements

- **Apple Silicon Mac** (M1 or later) — Virtualization.framework is ARM-only for macOS guests
- **macOS 13 Ventura or later** — required for Virtualization.framework macOS guest support
- **Tart installed** — `brew install cirruslabs/cli/tart` (for `macos_tart` mode)
- **Python 3 + PyAutoGUI** — installed in the VM golden image (or on host for native mode)
- **Xcode Command Line Tools** — `xcode-select --install`

### Permissions (TCC)

macOS requires explicit grants for Accessibility and Screen Recording. These must be pre-configured in the Tart golden image:

- **Accessibility** (`kTCCServiceAccessibility`) — required for PyAutoGUI mouse/keyboard control
- **Screen Recording** (`kTCCServiceScreenCapture`) — required for `screencapture` and PyAutoGUI screenshot functions

For the Tart golden image, the recommended setup is:
1. Boot the VM, open System Settings > Privacy & Security
2. Grant Accessibility and Screen Recording to Terminal.app and Python
3. Alternatively, disable SIP in Recovery and insert grants into the TCC database directly (see [TCC Database Setup](#tcc-database-setup) below)

## Apple Terms of Service

Desktest's macOS testing approach is fully compliant with Apple's macOS Software License Agreement. Here is a summary of the relevant terms and how they apply.

### What is permitted

- **UI automation via accessibility APIs**: Apple explicitly provides AXUIElement, NSAccessibility, and related APIs as public frameworks. Apple's own XCUITest is built on the same accessibility infrastructure. Using these APIs for automated testing is an intended use case.

- **PyAutoGUI / programmatic input**: PyAutoGUI on macOS uses CoreGraphics (`CGEvent`) for mouse/keyboard and CoreGraphics screen capture APIs for screenshots. These are public Apple APIs with no usage restrictions beyond the runtime permission gates (Accessibility, Screen Recording).

- **Screenshot capture**: `screencapture` is a built-in macOS utility. Programmatic capture via CoreGraphics is also unrestricted. The macOS permission system exists to protect user privacy, not to restrict legitimate testing.

- **Running macOS in VMs on Apple hardware**: The macOS SLA permits running up to 2 additional macOS instances in virtual machines, provided the VMs run on the same Apple-branded hardware as the host.

- **LLM-driven automation**: Apple's terms do not restrict what software makes API calls. The fact that an LLM agent drives the automation is irrelevant to licensing.

### Restrictions and limitations

- **Apple hardware required**: macOS may only run on Apple-branded hardware, whether as host or in a VM. Running macOS on non-Apple hardware (e.g., Hackintosh, generic x86 cloud VMs) violates the SLA. This means macOS desktest tests can only run on physical Macs (local, EC2 Mac, MacStadium, etc.).

- **2-VM limit per host**: The macOS SLA permits a maximum of 2 macOS VM instances running simultaneously per physical Mac. This is also enforced technically by Virtualization.framework (a third VM fails with `VZErrorDomain Code 6`). This limits parallel test execution — a single Mac can run at most 2 tests concurrently.

- **No macOS in Docker**: macOS cannot run inside Docker/OCI containers. There are no macOS container images, and distributing macOS as a container image would violate Apple's redistribution terms. This is why desktest uses Tart VMs instead of extending the existing Docker-based approach.

- **No macOS on non-Apple cloud**: Standard cloud VMs (AWS EC2 x86, GCP, Azure) cannot run macOS. Only dedicated Apple hardware offerings (AWS EC2 Mac instances, MacStadium, etc.) are compliant.

### Precedent

Many widely-used open source projects perform macOS UI automation without any known Apple enforcement issues:

- **Appium** (with Mac2 driver) — cross-platform UI automation
- **Hammerspoon** — Lua-scriptable macOS automation via accessibility APIs
- **cliclick** — CLI mouse/keyboard simulation
- **AppleScript + System Events** — Apple's own scripting bridge for UI automation

### Commercial CI services operating within Apple's terms

- **AWS EC2 Mac** — dedicated Mac mini hardware in AWS data centers, direct licensing agreement with Apple
- **MacStadium / Orka** — data centers of physical Mac hardware with Apple licensing agreements
- **Cirrus CI / Tart** — ephemeral macOS VMs on Apple Silicon, open source

## Limitations Compared to Linux

| Aspect | Linux (Docker) | macOS (Tart VM) |
|--------|---------------|-----------------|
| Parallel tests per host | Unlimited (Docker containers) | Max 2 (Apple VM limit) |
| Environment startup | ~5s (container start) | ~5-7s (VM clone + resume) |
| State isolation | Complete (container destroyed) | Complete (VM clone destroyed) |
| Accessibility tree | pyatspi / AT-SPI2 (mature) | AXUIElement via Swift helper (limited) |
| Action execution | PyAutoGUI via X11 | PyAutoGUI via Quartz/CoreGraphics |
| Screenshots | scrot (X11) | screencapture (built-in) |
| Video recording | ffmpeg x11grab | screencapture -V (built-in) |
| App deployment | Copy into container | Pre-installed in VM image or `open -a` |
| Security boundary | Docker container (non-root user) | VM boundary (stronger) |
| Headless operation | Native (Xvfb) | Requires VM (no headless macOS desktop) |
| CI infrastructure | Any Linux host with Docker | Apple Silicon hardware only |

## Clean State Management

Tart VMs provide clean state through an ephemeral clone workflow:

1. **Prepare golden image**: Create a Tart VM with macOS, install test tools (Python, PyAutoGUI, Swift a11y helper), configure TCC permissions, optionally install the app under test
2. **Before each test**: `tart clone golden-image test-run-N` (near-instant, APFS copy-on-write)
3. **Run test**: Boot the clone, run desktest agent loop against it
4. **After test**: `tart delete test-run-N` (clone destroyed, golden image unchanged)

This mirrors Docker's ephemeral container model. Each test gets a pristine environment.

### Alternative: Native mode (no isolation)

For local development, `macos_native` mode skips the VM entirely and runs on the host desktop. This is faster but provides no state isolation. Optional cleanup can reset `~/Library/Preferences` and `~/Library/Application Support` for the tested app between runs.

## TCC Database Setup

For automated CI environments, you can pre-grant permissions by modifying the TCC database in the golden image:

1. Boot the Tart VM into Recovery Mode
2. Disable SIP: `csrutil disable`
3. Reboot normally
4. Insert permission grants:

```bash
# Grant Accessibility and Screen Recording to Terminal
sudo sqlite3 "/Library/Application Support/com.apple.TCC/TCC.db" \
  "INSERT OR REPLACE INTO access (service, client, client_type, auth_value, auth_reason, auth_version) \
   VALUES ('kTCCServiceAccessibility', 'com.apple.Terminal', 0, 2, 0, 1);"

sudo sqlite3 "/Library/Application Support/com.apple.TCC/TCC.db" \
  "INSERT OR REPLACE INTO access (service, client, client_type, auth_value, auth_reason, auth_version) \
   VALUES ('kTCCServiceScreenCapture', 'com.apple.Terminal', 0, 2, 0, 1);"

# Grant Accessibility and Screen Recording to Python (required for PyAutoGUI)
# Adjust the path to match your Python installation in the golden image
PYTHON_PATH="/usr/local/bin/python3"

sudo sqlite3 "/Library/Application Support/com.apple.TCC/TCC.db" \
  "INSERT OR REPLACE INTO access (service, client, client_type, auth_value, auth_reason, auth_version) \
   VALUES ('kTCCServiceAccessibility', '${PYTHON_PATH}', 1, 2, 0, 1);"

sudo sqlite3 "/Library/Application Support/com.apple.TCC/TCC.db" \
  "INSERT OR REPLACE INTO access (service, client, client_type, auth_value, auth_reason, auth_version) \
   VALUES ('kTCCServiceScreenCapture', '${PYTHON_PATH}', 1, 2, 0, 1);"
```

> **Note**: `client_type` is `0` for bundle IDs (e.g., `com.apple.Terminal`) and `1` for absolute paths (e.g., `/usr/local/bin/python3`). If Python is installed via Homebrew, the path may be `/opt/homebrew/bin/python3`.

5. Optionally re-enable SIP: `csrutil enable` (from Recovery)
6. Save the VM as the golden image: `tart push golden-image ghcr.io/yourorg/macos-test:latest`

> **Note**: The exact TCC database schema varies between macOS versions. Verify column names against your target macOS version before inserting.
