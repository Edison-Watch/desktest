#!/usr/bin/env python3
"""Extract the accessibility tree via AT-SPI and write linearized TSV to stdout.

Columns: tag, name, text, class, description, position, size
"""

import argparse
import sys

try:
    import pyatspi
except ImportError:
    print("ERROR: pyatspi not available", file=sys.stderr)
    sys.exit(1)


def _build_role_names():
    """Build role-name mapping, skipping any constants missing from this pyatspi version."""
    _entries = [
        ("ROLE_PUSH_BUTTON", "button"),
        ("ROLE_CHECK_BOX", "checkbox"),
        ("ROLE_RADIO_BUTTON", "radio"),
        ("ROLE_TEXT", "text"),
        ("ROLE_PASSWORD_TEXT", "password"),
        ("ROLE_COMBO_BOX", "combobox"),
        ("ROLE_LIST", "list"),
        ("ROLE_LIST_ITEM", "listitem"),
        ("ROLE_MENU", "menu"),
        ("ROLE_MENU_ITEM", "menuitem"),
        ("ROLE_MENU_BAR", "menubar"),
        ("ROLE_TAB", "tab"),
        ("ROLE_TAB_LIST", "tablist"),
        ("ROLE_TOOL_BAR", "toolbar"),
        ("ROLE_TREE", "tree"),
        ("ROLE_TREE_ITEM", "treeitem"),
        ("ROLE_TABLE", "table"),
        ("ROLE_TABLE_CELL", "tablecell"),
        ("ROLE_TABLE_ROW", "tablerow"),
        ("ROLE_SCROLL_BAR", "scrollbar"),
        ("ROLE_SLIDER", "slider"),
        ("ROLE_SPIN_BUTTON", "spinbutton"),
        ("ROLE_STATUS_BAR", "statusbar"),
        ("ROLE_LABEL", "label"),
        ("ROLE_LINK", "link"),
        ("ROLE_IMAGE", "image"),
        ("ROLE_PANEL", "panel"),
        ("ROLE_FRAME", "frame"),
        ("ROLE_DIALOG", "dialog"),
        ("ROLE_WINDOW", "window"),
        ("ROLE_FILLER", "filler"),
        ("ROLE_SEPARATOR", "separator"),
        ("ROLE_SCROLL_PANE", "scrollpane"),
        ("ROLE_PAGE_TAB", "pagetab"),
        ("ROLE_PAGE_TAB_LIST", "pagetablist"),
        ("ROLE_ENTRY", "entry"),
        ("ROLE_APPLICATION", "application"),
        ("ROLE_DOCUMENT_FRAME", "documentframe"),
        ("ROLE_TOGGLE_BUTTON", "togglebutton"),
        ("ROLE_ICON", "icon"),
        ("ROLE_HEADING", "heading"),
        ("ROLE_PARAGRAPH", "paragraph"),
        ("ROLE_SECTION", "section"),
    ]
    result = {}
    for attr_name, friendly_name in _entries:
        role_const = getattr(pyatspi, attr_name, None)
        if role_const is not None:
            result[role_const] = friendly_name
    return result


ROLE_NAMES = _build_role_names()


def get_role_name(accessible):
    """Get a human-readable role name."""
    role = accessible.getRole()
    return ROLE_NAMES.get(role, accessible.getRoleName().replace(" ", ""))


def get_text(accessible):
    """Get text content from an accessible object."""
    try:
        text_iface = accessible.queryText()
        return text_iface.getText(0, text_iface.characterCount)
    except (NotImplementedError, AttributeError):
        return ""


def get_position_size(accessible):
    """Get position and size as strings."""
    try:
        component = accessible.queryComponent()
        bbox = component.getExtents(pyatspi.DESKTOP_COORDS)
        return f"{bbox.x},{bbox.y}", f"{bbox.width},{bbox.height}"
    except (NotImplementedError, AttributeError):
        return "", ""


def escape_tsv(value):
    """Escape a value for TSV output (replace tabs and newlines)."""
    if not value:
        return ""
    return value.replace("\t", " ").replace("\n", "\\n").replace("\r", "")


def walk_tree(accessible, rows, depth=0, max_depth=30, max_nodes=0):
    """Recursively walk the accessibility tree and collect rows.

    If max_nodes > 0, stop collecting once len(rows) >= max_nodes.
    """
    if depth > max_depth:
        return
    if max_nodes > 0 and len(rows) >= max_nodes:
        return

    try:
        tag = get_role_name(accessible)
        name = accessible.name or ""
        text = get_text(accessible)
        cls = accessible.getRoleName()
        description = accessible.description or ""
        position, size = get_position_size(accessible)

        row = "\t".join([
            escape_tsv(tag),
            escape_tsv(name),
            escape_tsv(text),
            escape_tsv(cls),
            escape_tsv(description),
            escape_tsv(position),
            escape_tsv(size),
        ])
        rows.append(row)

        for i in range(accessible.childCount):
            if max_nodes > 0 and len(rows) >= max_nodes:
                return
            try:
                child = accessible.getChildAtIndex(i)
                if child is not None:
                    walk_tree(child, rows, depth + 1, max_depth, max_nodes)
            except Exception:
                continue
    except Exception:
        return


def nonnegative_int(value):
    """Argparse type validator: accepts integers >= 0."""
    ivalue = int(value)
    if ivalue < 0:
        raise argparse.ArgumentTypeError(f"--max-nodes must be >= 0, got {value}")
    return ivalue


def main():
    parser = argparse.ArgumentParser(description="Extract accessibility tree via AT-SPI")
    parser.add_argument(
        "--max-nodes",
        type=nonnegative_int,
        default=0,
        help="Maximum number of nodes to extract (0 = unlimited, default: 0)",
    )
    args = parser.parse_args()

    # Print header
    print("tag\tname\ttext\tclass\tdescription\tposition\tsize")

    desktop = pyatspi.Registry.getDesktop(0)
    rows = []

    for i in range(desktop.childCount):
        if args.max_nodes > 0 and len(rows) >= args.max_nodes:
            break
        try:
            app = desktop.getChildAtIndex(i)
            if app is not None:
                walk_tree(app, rows, max_nodes=args.max_nodes)
        except Exception:
            continue

    for row in rows:
        print(row)


if __name__ == "__main__":
    main()
