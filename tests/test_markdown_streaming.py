"""Warm/cold equivalence through the shipped renderer's NDJSON boundary."""
from pathlib import Path
import unittest

from wire import Process


PACKAGE = Path(__file__).resolve().parents[1] / "packages/markdown-render"


class MarkdownStreamingTests(unittest.TestCase):
    def setUp(self):
        self.renderer = Process(PACKAGE, "", preview=True)
        self.addCleanup(self.renderer.close)

    def render(self, view, source, revision):
        self.renderer.send({
            "type": "render", "view": view, "surface": "content",
            "content": {"language": "markdown", "source": source,
                        "context": {"languages": ["mermaid", "latex", "md"]},
                        "source_revision": revision},
        })
        response = self.renderer.wait(lambda message:
            message.get("view") == view and message.get("source_revision") == revision
            and message.get("type") in ("view", "content_error"))
        self.assertEqual(response["type"], "view", response)
        return response["root"]

    def compare(self, before, after):
        self.render("warm", before, 1)
        warm = self.render("warm", after, 2)
        cold = self.render("cold", after, 1)
        self.assertEqual(warm, cold)
        for view in ("warm", "cold"):
            self.renderer.send({"type": "close", "view": view})

    def test_closing_line_edit_revalidates_retained_code_and_math(self):
        for opening, closing in (("```mermaid", "```"), ("~~~rust", "~~~"),
                                 ("```md", "```"), ("$$", "$$")):
            for ending in ("\n", "\r\n"):
                with self.subTest(opening=opening, ending=repr(ending)):
                    prefix = f"{opening}{ending}A-->{ending}{closing}"
                    self.compare(prefix + ending + "Tail",
                                 prefix + " 🦀" + ending + "Tail")

    def test_removed_terminator_then_restored_append_matches_a_fresh_view(self):
        for opening, closing in (("```mermaid", "```"), ("$$", "$$")):
            with self.subTest(opening=opening):
                prefix = f"{opening}\nA\n{closing}"
                self.render("warm", prefix + "\n", 1)
                self.assertEqual(self.render("warm", prefix, 2),
                                 self.render("cold", prefix, 1))
                self.render("warm", prefix + "\n", 3)
                self.assertEqual(self.render("warm", prefix + "\nTail", 4),
                                 self.render("cold", prefix + "\nTail", 2))
                for view in ("warm", "cold"):
                    self.renderer.send({"type": "close", "view": view})


if __name__ == "__main__":
    unittest.main()
