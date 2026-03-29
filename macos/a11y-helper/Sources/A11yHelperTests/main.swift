/// Standalone test runner for a11y-helper.
/// No XCTest/Testing framework required — runs with Command Line Tools only.
///
/// Run: swift run a11y-helper-tests

import A11yHelperLib
import Foundation

var passed = 0
var failed = 0

func assert(_ condition: Bool, _ message: String, file: String = #file, line: Int = #line) {
    if condition {
        passed += 1
    } else {
        failed += 1
        print("  FAIL [\(file):\(line)]: \(message)")
    }
}

func assertEqual<T: Equatable>(_ a: T, _ b: T, _ message: String = "", file: String = #file, line: Int = #line) {
    if a == b {
        passed += 1
    } else {
        failed += 1
        let detail = message.isEmpty ? "\(a) != \(b)" : "\(message): \(a) != \(b)"
        print("  FAIL [\(file):\(line)]: \(detail)")
    }
}

func suite(_ name: String, _ body: () -> Void) {
    print("  \(name)")
    body()
}

// ============================================================
// escapeTSV
// ============================================================
suite("escapeTSV") {
    assertEqual(escapeTSV("hello"), "hello", "plain string")
    assertEqual(escapeTSV(""), "", "empty string")
    assertEqual(escapeTSV("col1\tcol2\tcol3"), "col1 col2 col3", "tabs → spaces")
    assertEqual(escapeTSV("line1\nline2"), "line1\\nline2", "newlines escaped")
    assertEqual(escapeTSV("line1\r\nline2"), "line1\\nline2", "CR+LF handled")
    assertEqual(escapeTSV("a\tb\nc\rd"), "a b\\ncd", "mixed special chars")
}

// ============================================================
// friendlyRoleName — known roles
// ============================================================
suite("friendlyRoleName — known roles") {
    assertEqual(friendlyRoleName("AXButton"), "button")
    assertEqual(friendlyRoleName("AXCheckBox"), "checkbox")
    assertEqual(friendlyRoleName("AXRadioButton"), "radio")
    assertEqual(friendlyRoleName("AXTextField"), "text")
    assertEqual(friendlyRoleName("AXSecureTextField"), "password")
    assertEqual(friendlyRoleName("AXStaticText"), "label")
    assertEqual(friendlyRoleName("AXWindow"), "window")
    assertEqual(friendlyRoleName("AXApplication"), "application")
    assertEqual(friendlyRoleName("AXMenuBar"), "menubar")
    assertEqual(friendlyRoleName("AXMenuItem"), "menuitem")
    assertEqual(friendlyRoleName("AXComboBox"), "combobox")
    assertEqual(friendlyRoleName("AXList"), "list")
    assertEqual(friendlyRoleName("AXRow"), "listitem")
    assertEqual(friendlyRoleName("AXTabGroup"), "tablist")
    assertEqual(friendlyRoleName("AXToolbar"), "toolbar")
    assertEqual(friendlyRoleName("AXOutline"), "tree")
    assertEqual(friendlyRoleName("AXOutlineRow"), "treeitem")
    assertEqual(friendlyRoleName("AXTable"), "table")
    assertEqual(friendlyRoleName("AXCell"), "tablecell")
    assertEqual(friendlyRoleName("AXScrollBar"), "scrollbar")
    assertEqual(friendlyRoleName("AXSlider"), "slider")
    assertEqual(friendlyRoleName("AXIncrementor"), "spinbutton")
    assertEqual(friendlyRoleName("AXLink"), "link")
    assertEqual(friendlyRoleName("AXImage"), "image")
    assertEqual(friendlyRoleName("AXGroup"), "panel")
    assertEqual(friendlyRoleName("AXDialog"), "dialog")
    assertEqual(friendlyRoleName("AXSplitter"), "separator")
    assertEqual(friendlyRoleName("AXScrollArea"), "scrollpane")
    assertEqual(friendlyRoleName("AXWebArea"), "documentframe")
    assertEqual(friendlyRoleName("AXHeading"), "heading")
    assertEqual(friendlyRoleName("AXTextArea"), "text")
    assertEqual(friendlyRoleName("AXPopUpButton"), "combobox")
    assertEqual(friendlyRoleName("AXSheet"), "dialog")
}

// ============================================================
// friendlyRoleName — fallback behavior
// ============================================================
suite("friendlyRoleName — fallback") {
    // Unknown AX roles strip "AX" prefix and lowercase
    assertEqual(friendlyRoleName("AXCustomWidget"), "customwidget")
    assertEqual(friendlyRoleName("AXSomeNewRole"), "somenewrole")
    // Non-AX roles lowercase as-is
    assertEqual(friendlyRoleName("Button"), "button")
    assertEqual(friendlyRoleName("SomeRole"), "somerole")
    assertEqual(friendlyRoleName(""), "")
}

// ============================================================
// friendlyRoleName — duplicate mappings (same Linux name)
// ============================================================
suite("friendlyRoleName — duplicate mappings") {
    assertEqual(friendlyRoleName("AXTextField"), friendlyRoleName("AXTextArea"), "text")
    assertEqual(friendlyRoleName("AXComboBox"), friendlyRoleName("AXPopUpButton"), "combobox")
    assertEqual(friendlyRoleName("AXDialog"), friendlyRoleName("AXSheet"), "dialog")
}

// ============================================================
// Role name coverage: every Linux role has a macOS mapping
// ============================================================
suite("role name coverage — Linux roles") {
    let linuxRoles: Set<String> = [
        "button", "checkbox", "radio", "text", "password", "combobox",
        "list", "listitem", "menu", "menuitem", "menubar", "tablist",
        "toolbar", "tree", "treeitem", "table", "tablecell", "scrollbar",
        "slider", "spinbutton", "label", "link", "image", "panel",
        "window", "dialog", "separator", "scrollpane", "application",
        "documentframe", "heading",
    ]
    let macosValues = Set(roleNames.values)
    for role in linuxRoles.sorted() {
        assert(macosValues.contains(role), "Linux role '\(role)' has no macOS AX mapping")
    }
}

// ============================================================
// buildTSVRow
// ============================================================
suite("buildTSVRow") {
    // Basic row
    let row = buildTSVRow(
        tag: "button", name: "OK", text: "", cls: "push button",
        description: "Confirm", position: "100,200", size: "80,30")
    let cols = row.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
    assertEqual(cols.count, 7, "column count")
    assertEqual(cols[0], "button", "tag")
    assertEqual(cols[1], "OK", "name")
    assertEqual(cols[2], "", "text (empty)")
    assertEqual(cols[3], "push button", "class")
    assertEqual(cols[4], "Confirm", "description")
    assertEqual(cols[5], "100,200", "position")
    assertEqual(cols[6], "80,30", "size")

    // Row with special characters
    let row2 = buildTSVRow(
        tag: "text", name: "Input\tField", text: "line1\nline2", cls: "text field",
        description: "", position: "0,0", size: "100,20")
    assert(!row2.contains("\n"), "row should not contain literal newline")
    let cols2 = row2.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
    assertEqual(cols2.count, 7, "special chars column count")
    assertEqual(cols2[1], "Input Field", "tab in name → space")
    assertEqual(cols2[2], "line1\\nline2", "newline in text → escaped")

    // All empty fields
    let row3 = buildTSVRow(
        tag: "", name: "", text: "", cls: "",
        description: "", position: "", size: "")
    let cols3 = row3.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
    assertEqual(cols3.count, 7, "empty fields column count")
    assert(cols3.allSatisfy { $0.isEmpty }, "all fields should be empty")
}

// ============================================================
// TSV header
// ============================================================
suite("TSV header") {
    assertEqual(tsvHeader, "tag\tname\ttext\tclass\tdescription\tposition\tsize", "header format")
    assertEqual(tsvHeader.split(separator: "\t").count, 7, "header column count")

    // Row column count matches header
    let headerCols = tsvHeader.split(separator: "\t").count
    let row = buildTSVRow(
        tag: "button", name: "OK", text: "click", cls: "btn",
        description: "d", position: "1,2", size: "3,4")
    let rowCols = row.split(separator: "\t", omittingEmptySubsequences: false).count
    assertEqual(headerCols, rowCols, "header vs row column count")
}

// ============================================================
// parseArgs
// ============================================================
suite("parseArgs") {
    let defaults = parseArgs(["a11y-helper"])
    assertEqual(defaults.maxNodes, 0, "default maxNodes")
    assert(defaults.appPid == nil, "default appPid is nil")

    let withMaxNodes = parseArgs(["a11y-helper", "--max-nodes", "500"])
    assertEqual(withMaxNodes.maxNodes, 500, "maxNodes 500")
    assert(withMaxNodes.appPid == nil, "appPid nil with maxNodes only")

    let withPid = parseArgs(["a11y-helper", "--app-pid", "12345"])
    assertEqual(withPid.maxNodes, 0, "maxNodes default with pid only")
    assertEqual(withPid.appPid!, 12345, "appPid 12345")

    let withBoth = parseArgs(["a11y-helper", "--max-nodes", "100", "--app-pid", "42"])
    assertEqual(withBoth.maxNodes, 100, "both: maxNodes")
    assertEqual(withBoth.appPid!, 42, "both: appPid")

    let withZero = parseArgs(["a11y-helper", "--max-nodes", "0"])
    assertEqual(withZero.maxNodes, 0, "maxNodes 0")
}

// ============================================================
// Summary
// ============================================================
print("")
if failed > 0 {
    print("FAILED: \(passed) passed, \(failed) failed")
    exit(1)
} else {
    print("OK: \(passed) passed")
}
