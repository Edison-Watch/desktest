#!/usr/bin/env python3
"""Extract the accessibility tree via AT-SPI and write linearized TSV to stdout.

Columns: tag, name, text, class, description, position, size
"""

import sys

try:
    import pyatspi
except ImportError:
    print("ERROR: pyatspi not available", file=sys.stderr)
    sys.exit(1)


ROLE_NAMES = {
    pyatspi.ROLE_PUSH_BUTTON: "button",
    pyatspi.ROLE_CHECK_BOX: "checkbox",
    pyatspi.ROLE_RADIO_BUTTON: "radio",
    pyatspi.ROLE_TEXT: "text",
    pyatspi.ROLE_PASSWORD_TEXT: "password",
    pyatspi.ROLE_COMBO_BOX: "combobox",
    pyatspi.ROLE_LIST: "list",
    pyatspi.ROLE_LIST_ITEM: "listitem",
    pyatspi.ROLE_MENU: "menu",
    pyatspi.ROLE_MENU_ITEM: "menuitem",
    pyatspi.ROLE_MENU_BAR: "menubar",
    pyatspi.ROLE_TAB: "tab",
    pyatspi.ROLE_TAB_LIST: "tablist",
    pyatspi.ROLE_TOOL_BAR: "toolbar",
    pyatspi.ROLE_TREE: "tree",
    pyatspi.ROLE_TREE_ITEM: "treeitem",
    pyatspi.ROLE_TABLE: "table",
    pyatspi.ROLE_TABLE_CELL: "tablecell",
    pyatspi.ROLE_TABLE_ROW: "tablerow",
    pyatspi.ROLE_SCROLL_BAR: "scrollbar",
    pyatspi.ROLE_SLIDER: "slider",
    pyatspi.ROLE_SPIN_BUTTON: "spinbutton",
    pyatspi.ROLE_STATUS_BAR: "statusbar",
    pyatspi.ROLE_LABEL: "label",
    pyatspi.ROLE_LINK: "link",
    pyatspi.ROLE_IMAGE: "image",
    pyatspi.ROLE_PANEL: "panel",
    pyatspi.ROLE_FRAME: "frame",
    pyatspi.ROLE_DIALOG: "dialog",
    pyatspi.ROLE_WINDOW: "window",
    pyatspi.ROLE_FILLER: "filler",
    pyatspi.ROLE_SEPARATOR: "separator",
    pyatspi.ROLE_SCROLL_PANE: "scrollpane",
    pyatspi.ROLE_PAGE_TAB: "pagetab",
    pyatspi.ROLE_PAGE_TAB_LIST: "pagetablist",
    pyatspi.ROLE_ENTRY: "entry",
    pyatspi.ROLE_APPLICATION: "application",
    pyatspi.ROLE_DOCUMENT_FRAME: "documentframe",
    pyatspi.ROLE_TOGGLE_BUTTON: "togglebutton",
    pyatspi.ROLE_ICON: "icon",
    pyatspi.ROLE_HEADING: "heading",
    pyatspi.ROLE_PARAGRAPH: "paragraph",
    pyatspi.ROLE_SECTION: "section",
}


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


def walk_tree(accessible, rows, depth=0, max_depth=30):
    """Recursively walk the accessibility tree and collect rows."""
    if depth > max_depth:
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
            try:
                child = accessible.getChildAtIndex(i)
                if child is not None:
                    walk_tree(child, rows, depth + 1, max_depth)
            except Exception:
                continue
    except Exception:
        return


def main():
    # Print header
    print("tag\tname\ttext\tclass\tdescription\tposition\tsize")

    desktop = pyatspi.Registry.getDesktop(0)
    rows = []

    for i in range(desktop.childCount):
        try:
            app = desktop.getChildAtIndex(i)
            if app is not None:
                walk_tree(app, rows)
        except Exception:
            continue

    for row in rows:
        print(row)


if __name__ == "__main__":
    main()
