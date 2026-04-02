#!/usr/bin/env python3
"""Extract the accessibility tree on Windows via UI Automation (uiautomation package).

Outputs a linearized TSV to stdout with columns:
    tag, name, text, class, description, position, size

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


def get_tag(control):
    """Get a short tag name for a UIA control."""
    control_type = control.ControlTypeName
    return CONTROL_TYPE_TAGS.get(control_type, control_type.replace("Control", "").lower())


def walk_tree(control, lines, max_nodes, depth=0):
    """Recursively walk the UIA tree and collect TSV lines."""
    if max_nodes > 0 and len(lines) >= max_nodes:
        return

    tag = get_tag(control)
    name = (control.Name or "").replace("\t", " ").replace("\n", " ").strip()
    class_name = (control.ClassName or "").replace("\t", " ").strip()

    # Try to get text value
    text = ""
    try:
        pattern = control.GetValuePattern()
        if pattern:
            text = (pattern.Value or "").replace("\t", " ").replace("\n", " ").strip()
    except Exception:
        pass

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
    line = f"{indent}{tag}\t{name}\t{text}\t{class_name}\t\t{position}\t{size}"
    lines.append(line)

    # Recurse into children
    for child in control.GetChildren():
        if max_nodes > 0 and len(lines) >= max_nodes:
            break
        walk_tree(child, lines, max_nodes, depth + 1)


def main():
    parser = argparse.ArgumentParser(description="Extract Windows UI Automation accessibility tree")
    parser.add_argument("--max-nodes", type=int, default=0,
                        help="Maximum number of nodes to extract (0 = unlimited)")
    args = parser.parse_args()

    root = auto.GetRootControl()
    lines = []

    # Walk all top-level windows (skip the desktop root itself)
    for window in root.GetChildren():
        walk_tree(window, lines, args.max_nodes)
        if args.max_nodes > 0 and len(lines) >= args.max_nodes:
            break

    print("\n".join(lines))


if __name__ == "__main__":
    main()
