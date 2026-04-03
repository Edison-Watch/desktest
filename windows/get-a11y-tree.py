#!/usr/bin/env python3
"""Extract the accessibility tree on Windows via UI Automation (uiautomation package).

Outputs a linearized TSV to stdout with columns:
    tag, name, text, class, description, state, position, size

Requires: pip install uiautomation
"""

import argparse
import sys

try:
    import uiautomation as auto
except ImportError:
    print("ERROR: uiautomation not available (pip install uiautomation)", file=sys.stderr)
    sys.exit(1)


# Map UIA control type IDs to short tag names
CONTROL_TYPE_TAGS = {
    "ButtonControl": "button",
    "CheckBoxControl": "checkbox",
    "RadioButtonControl": "radio",
    "EditControl": "text",
    "ComboBoxControl": "combobox",
    "ListControl": "list",
    "ListItemControl": "listitem",
    "MenuControl": "menu",
    "MenuItemControl": "menuitem",
    "MenuBarControl": "menubar",
    "TabControl": "tab",
    "TabItemControl": "tabitem",
    "TreeControl": "tree",
    "TreeItemControl": "treeitem",
    "TableControl": "table",
    "DataItemControl": "dataitem",
    "TextControl": "statictext",
    "ImageControl": "image",
    "ToolBarControl": "toolbar",
    "StatusBarControl": "statusbar",
    "ScrollBarControl": "scrollbar",
    "SliderControl": "slider",
    "SpinnerControl": "spinner",
    "ProgressBarControl": "progressbar",
    "HyperlinkControl": "link",
    "WindowControl": "window",
    "PaneControl": "panel",
    "GroupControl": "group",
    "DocumentControl": "document",
    "TitleBarControl": "titlebar",
    "HeaderControl": "header",
    "HeaderItemControl": "headeritem",
    "SplitButtonControl": "splitbutton",
    "ToolTipControl": "tooltip",
    "DataGridControl": "datagrid",
    "CustomControl": "custom",
}

MAX_DEPTH = 30


def escape_tsv(value):
    """Escape a value for TSV output (replace tabs and newlines)."""
    if not value:
        return ""
    return value.replace("\t", " ").replace("\n", "\\n").replace("\r", "")


def get_tag(control):
    """Get a short tag name for a UIA control."""
    control_type = control.ControlTypeName
    return CONTROL_TYPE_TAGS.get(control_type, control_type.replace("Control", "").lower())


def get_state(control):
    """Get a compact state string from UIA patterns (enabled, toggle, selection)."""
    parts = []

    # IsEnabled — only note when disabled (enabled is the default)
    try:
        if not control.IsEnabled:
            parts.append("disabled")
    except Exception:
        pass

    # TogglePattern — checkbox/toggle state
    try:
        pattern = control.GetTogglePattern()
        if pattern:
            state = pattern.ToggleState
            # ToggleState: 0=Off, 1=On, 2=Indeterminate
            if state == 1:
                parts.append("checked")
            elif state == 2:
                parts.append("indeterminate")
    except Exception:
        pass

    # SelectionItemPattern — list/tab selection
    try:
        pattern = control.GetSelectionItemPattern()
        if pattern and pattern.IsSelected:
            parts.append("selected")
    except Exception:
        pass

    # ExpandCollapsePattern — menus, comboboxes, tree items
    try:
        pattern = control.GetExpandCollapsePattern()
        if pattern:
            state = pattern.ExpandCollapseState
            # 0=Collapsed, 1=Expanded, 2=PartiallyExpanded, 3=LeafNode
            if state == 1:
                parts.append("expanded")
            elif state == 0:
                parts.append("collapsed")
    except Exception:
        pass

    return ",".join(parts)


def is_offscreen(control):
    """Check if a control is offscreen (not visible)."""
    try:
        rect = control.BoundingRectangle
        if rect is None:
            return True
        if rect.width() <= 0 or rect.height() <= 0:
            return True
        # Check IsOffscreen property
        return bool(control.IsOffscreen)
    except Exception:
        return False


def walk_tree(control, lines, max_nodes, skip_offscreen, depth=0):
    """Recursively walk the UIA tree and collect TSV lines."""
    if depth > MAX_DEPTH:
        return
    if max_nodes > 0 and len(lines) >= max_nodes:
        return

    # Skip offscreen elements if requested
    if skip_offscreen and depth > 0 and is_offscreen(control):
        return

    tag = get_tag(control)
    name = escape_tsv(control.Name or "")
    class_name = escape_tsv(control.ClassName or "")

    # Try to get text value
    text = ""
    try:
        pattern = control.GetValuePattern()
        if pattern:
            text = escape_tsv(pattern.Value or "")
    except Exception:
        pass

    # AutomationId as description (useful for developers)
    description = escape_tsv(control.AutomationId or "")

    # State info
    state = get_state(control)

    # Get bounding rectangle
    rect = control.BoundingRectangle
    if rect and rect.width() > 0 and rect.height() > 0:
        position = f"{rect.left},{rect.top}"
        size = f"{rect.width()},{rect.height()}"
    else:
        position = ""
        size = ""

    # Indent by depth for readability
    indent = "  " * depth
    line = f"{indent}{tag}\t{name}\t{text}\t{class_name}\t{description}\t{state}\t{position}\t{size}"
    lines.append(line)

    # Recurse into children
    for child in control.GetChildren():
        if max_nodes > 0 and len(lines) >= max_nodes:
            break
        walk_tree(child, lines, max_nodes, skip_offscreen, depth + 1)


def main():
    parser = argparse.ArgumentParser(description="Extract Windows UI Automation accessibility tree")
    parser.add_argument("--max-nodes", type=int, default=0,
                        help="Maximum number of nodes to extract (0 = unlimited)")
    parser.add_argument("--include-offscreen", action="store_true",
                        help="Include offscreen/invisible elements (excluded by default)")
    args = parser.parse_args()

    skip_offscreen = not args.include_offscreen

    # Print header
    print("tag\tname\ttext\tclass\tdescription\tstate\tposition\tsize")

    root = auto.GetRootControl()
    lines = []

    # Walk all top-level windows (skip the desktop root itself)
    for window in root.GetChildren():
        walk_tree(window, lines, args.max_nodes, skip_offscreen)
        if args.max_nodes > 0 and len(lines) >= args.max_nodes:
            break

    print("\n".join(lines))


if __name__ == "__main__":
    main()
