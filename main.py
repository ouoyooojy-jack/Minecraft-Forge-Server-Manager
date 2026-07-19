"""Entry point. Composes the Flet page using the new app shell."""
import logging
import flet as ft
from ui.app import build_app

def main(page: ft.Page) -> None:
    page.title = "Mc Server Manager"
    page.window.width = 720
    page.window.height = 480
    # Intentionally NOT setting page.horizontal_alignment: doing so
    # would centre the Row inside the page and stop the sidebar from
    # hugging the left edge. The Row uses MainAxisAlignment.START on
    # its own; we let it.
    page.add(build_app(page))


if __name__ == "__main__":
    import sys
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        stream=sys.stderr,                  # logs stay out of stdout
    )

    # Optional debug dump of the layout's flush-edge properties.
    #   python main.py --debug-flush
    # prints key layout attrs to stderr so we can see, without opening
    # any inspector, why the sidebar isn't hugging the window edge.
    if "--debug-flush" in sys.argv:
        from ui.app import build_app
        from ui.sidebar import SIDEBAR_BG

        class _FP:
            def __init__(self):
                self.bgcolor = None
                self.window_bgcolor = None
                self.controls = []
                self.theme_mode = None
                self.theme = None
                self.appbar = None
                self.snack_bar = None
                self.padding = None
                self.spacing = None
                self.horizontal_alignment = None
                self.vertical_alignment = None
                self.window = None

            def add(self, c):
                self.controls.append(c)

            def update(self):
                pass

            def run_task(self, *a, **kw):
                pass

        page = _FP()
        row = build_app(page)
        sw = row.controls[0]
        outer = sw  # what build_app puts as first Row child
        print("[flush-debug] page.padding              =", page.padding)
        print("[flush-debug] page.bgcolor               =", page.bgcolor)
        print("[flush-debug] page.spacing               =", page.spacing)
        print("[flush-debug] row.tight                  =", row.tight)
        print("[flush-debug] row.spacing                =", row.spacing)
        print("[flush-debug] row.alignment              =", row.alignment)
        print("[flush-debug] outer.bgcolor              =", outer.bgcolor)
        print("[flush-debug] outer.padding              =", outer.padding)
        print("[flush-debug] outer.margin               =", getattr(outer, "margin", None))
        print("[flush-debug] outer.border               =", getattr(outer, "border", None))
        print("[flush-debug] outer.border_radius        =", outer.border_radius)
        print("[flush-debug] outer.left (margin-left)  =", getattr(outer, "left", None))
        print("[flush-debug] expected SIDEBAR_BG         =", SIDEBAR_BG)
        sys.exit(0)

    ft.run(main)
