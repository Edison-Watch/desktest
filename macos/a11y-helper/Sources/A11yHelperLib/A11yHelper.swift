import AppKit
import ApplicationServices
import Foundation

// MARK: - CLI argument parsing

public struct Options {
    public var maxNodes: Int = 0  // 0 = unlimited
    public var appPid: pid_t? = nil

    public init(maxNodes: Int = 0, appPid: pid_t? = nil) {
        self.maxNodes = maxNodes
        self.appPid = appPid
    }
}

public func parseArgs(_ arguments: [String]) -> Options {
    var opts = Options()
    var args = arguments.dropFirst()  // skip program name
    while let arg = args.popFirst() {
        switch arg {
        case "--max-nodes":
            guard let val = args.popFirst(), let n = Int(val), n >= 0 else {
                fputs("error: --max-nodes requires a non-negative integer\n", stderr)
                exit(1)
            }
            opts.maxNodes = n
        case "--app-pid":
            guard let val = args.popFirst(), let p = Int32(val) else {
                fputs("error: --app-pid requires a valid PID\n", stderr)
                exit(1)
            }
            opts.appPid = p
        case "--help", "-h":
            print("""
            Usage: a11y-helper [OPTIONS]

            Extract the macOS accessibility tree and output as TSV.

            Options:
              --max-nodes <n>   Maximum number of nodes to extract (0 = unlimited)
              --app-pid <pid>   Only extract tree for a specific application PID
              -h, --help        Show this help message
            """)
            exit(0)
        default:
            fputs("error: unknown argument: \(arg)\n", stderr)
            exit(1)
        }
    }
    return opts
}

// MARK: - Role name mapping

/// Maps AXRole strings to the same friendly names used by the Linux get-a11y-tree.py.
public let roleNames: [String: String] = [
    "AXButton": "button",
    "AXCheckBox": "checkbox",
    "AXRadioButton": "radio",
    "AXTextField": "text",
    "AXSecureTextField": "password",
    "AXComboBox": "combobox",
    "AXList": "list",
    "AXRow": "listitem",
    "AXMenu": "menu",
    "AXMenuItem": "menuitem",
    "AXMenuBar": "menubar",
    "AXTabGroup": "tablist",
    "AXToolbar": "toolbar",
    "AXOutline": "tree",
    "AXOutlineRow": "treeitem",
    "AXTable": "table",
    "AXCell": "tablecell",
    "AXScrollBar": "scrollbar",
    "AXSlider": "slider",
    "AXIncrementor": "spinbutton",
    "AXStaticText": "label",
    "AXLink": "link",
    "AXImage": "image",
    "AXGroup": "panel",
    "AXWindow": "window",
    "AXDialog": "dialog",
    "AXSplitter": "separator",
    "AXScrollArea": "scrollpane",
    "AXApplication": "application",
    "AXWebArea": "documentframe",
    "AXHeading": "heading",
    "AXTextArea": "text",
    "AXPopUpButton": "combobox",
    "AXSheet": "dialog",
]

public func friendlyRoleName(_ axRole: String) -> String {
    roleNames[axRole] ?? axRole
        .replacingOccurrences(of: "AX", with: "")
        .lowercased()
}

// MARK: - AXUIElement attribute helpers

public func attribute<T>(_ element: AXUIElement, _ attr: String) -> T? {
    var value: AnyObject?
    guard AXUIElementCopyAttributeValue(element, attr as CFString, &value) == .success else {
        return nil
    }
    return value as? T
}

public func stringAttribute(_ element: AXUIElement, _ attr: String) -> String {
    attribute(element, attr) ?? ""
}

public func getRole(_ element: AXUIElement) -> String {
    stringAttribute(element, kAXRoleAttribute as String)
}

public func getTitle(_ element: AXUIElement) -> String {
    stringAttribute(element, kAXTitleAttribute as String)
}

public func getValue(_ element: AXUIElement) -> String {
    var value: AnyObject?
    guard AXUIElementCopyAttributeValue(element, kAXValueAttribute as CFString, &value) == .success
    else {
        return ""
    }
    if let str = value as? String {
        return str
    }
    if let num = value as? NSNumber {
        return num.stringValue
    }
    return ""
}

public func getDescription(_ element: AXUIElement) -> String {
    stringAttribute(element, kAXDescriptionAttribute as String)
}

public func getRoleDescription(_ element: AXUIElement) -> String {
    stringAttribute(element, kAXRoleDescriptionAttribute as String)
}

public func getPosition(_ element: AXUIElement) -> String {
    var value: AnyObject?
    guard AXUIElementCopyAttributeValue(element, kAXPositionAttribute as CFString, &value) == .success
    else {
        return ""
    }
    var point = CGPoint.zero
    guard AXValueGetValue(value as! AXValue, .cgPoint, &point) else {
        return ""
    }
    return "\(Int(point.x)),\(Int(point.y))"
}

public func getSize(_ element: AXUIElement) -> String {
    var value: AnyObject?
    guard AXUIElementCopyAttributeValue(element, kAXSizeAttribute as CFString, &value) == .success
    else {
        return ""
    }
    var size = CGSize.zero
    guard AXValueGetValue(value as! AXValue, .cgSize, &size) else {
        return ""
    }
    return "\(Int(size.width)),\(Int(size.height))"
}

public func getChildren(_ element: AXUIElement) -> [AXUIElement] {
    var value: AnyObject?
    guard
        AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &value) == .success
    else {
        return []
    }
    return (value as? [AXUIElement]) ?? []
}

// MARK: - TSV output

public func escapeTSV(_ value: String) -> String {
    value
        .replacingOccurrences(of: "\t", with: " ")
        .replacingOccurrences(of: "\n", with: "\\n")
        .replacingOccurrences(of: "\r", with: "")
}

/// The TSV header line, matching the Linux get-a11y-tree.py format.
public let tsvHeader = "tag\tname\ttext\tclass\tdescription\tposition\tsize"

/// Build a TSV row from pre-extracted field values.
public func buildTSVRow(
    tag: String, name: String, text: String, cls: String,
    description: String, position: String, size: String
) -> String {
    [tag, name, text, cls, description, position, size]
        .map { escapeTSV($0) }
        .joined(separator: "\t")
}

// MARK: - Tree walking

public func walkTree(
    _ element: AXUIElement, rows: inout [String], depth: Int = 0,
    maxDepth: Int = 30, maxNodes: Int = 0
) {
    if depth > maxDepth { return }
    if maxNodes > 0 && rows.count >= maxNodes { return }

    let role = getRole(element)
    let tag = friendlyRoleName(role)
    let name = getTitle(element)
    let text = getValue(element)
    let cls = getRoleDescription(element)
    let description = getDescription(element)
    let position = getPosition(element)
    let size = getSize(element)

    let row = buildTSVRow(
        tag: tag, name: name, text: text, cls: cls,
        description: description, position: position, size: size)
    rows.append(row)

    for child in getChildren(element) {
        if maxNodes > 0 && rows.count >= maxNodes { return }
        walkTree(child, rows: &rows, depth: depth + 1, maxDepth: maxDepth, maxNodes: maxNodes)
    }
}
