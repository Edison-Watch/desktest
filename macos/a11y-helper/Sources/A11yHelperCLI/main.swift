import A11yHelperLib
import AppKit
import ApplicationServices

let opts = parseArgs(CommandLine.arguments)

// Check accessibility permissions.
// AXIsProcessTrustedWithOptions queries tccd, which may return false for
// LaunchAgent-spawned processes even when the actual AXUIElement API calls
// succeed (due to "responsible process" attribution). We log a warning but
// continue — the AX calls themselves will return empty trees if truly denied.
let trusted = AXIsProcessTrustedWithOptions(
    [kAXTrustedCheckOptionPrompt.takeRetainedValue(): false] as CFDictionary)
if !trusted {
    fputs(
        "warning: AXIsProcessTrustedWithOptions returned false — accessibility tree may be empty. "
        + "Grant permission in System Settings > Privacy & Security > Accessibility.\n",
        stderr)
}

// Print header (matches Linux get-a11y-tree.py format)
print(tsvHeader)

var rows: [String] = []

if let pid = opts.appPid {
    // Single-app mode: walk only the specified application
    let appElement = AXUIElementCreateApplication(pid)
    walkTree(appElement, rows: &rows, maxNodes: opts.maxNodes)
} else {
    // All-apps mode: walk every application (matches Linux behavior)
    let apps = NSWorkspace.shared.runningApplications
    for app in apps {
        if opts.maxNodes > 0 && rows.count >= opts.maxNodes { break }
        // Skip background-only apps (no UI)
        guard app.activationPolicy == .regular || app.activationPolicy == .accessory else {
            continue
        }
        let appElement = AXUIElementCreateApplication(app.processIdentifier)
        walkTree(appElement, rows: &rows, maxNodes: opts.maxNodes)
    }
}

for row in rows {
    print(row)
}
