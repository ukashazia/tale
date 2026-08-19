import unittest

from patch_formula import patch_formula


FORMULA = """class Tale < Formula
  def install
    bin.install "tale"

    install_binary_aliases!

    # Homebrew will automatically install these, so we don't need to do that
    doc_files = Dir["README.*", "LICENSE"]
    leftover_contents = Dir["*"] - doc_files
    pkgshare.install(*leftover_contents) unless leftover_contents.empty?
  end
end
"""


class PatchFormulaTests(unittest.TestCase):
    def test_installs_each_completion_without_copying_the_directory_to_pkgshare(self):
        patched = patch_formula(FORMULA)

        self.assertIn(
            'bash_completion.install "completions/tale.bash" => "tale"', patched
        )
        self.assertIn('zsh_completion.install "completions/_tale"', patched)
        self.assertIn(
            'fish_completion.install "completions/tale.fish"', patched
        )
        self.assertIn('package_manager_files = ["completions"]', patched)
        self.assertIn(
            'Dir["*"] - doc_files - package_manager_files', patched
        )

    def test_rejects_formula_drift_and_double_patching(self):
        with self.assertRaisesRegex(ValueError, "binary-alias install anchor"):
            patch_formula(FORMULA.replace("    install_binary_aliases!\n", ""))

        with self.assertRaisesRegex(ValueError, "already installs"):
            patch_formula(patch_formula(FORMULA))


if __name__ == "__main__":
    unittest.main()
