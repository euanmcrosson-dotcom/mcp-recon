"""Regression tests for sandbox_shim.py (run: python sandbox_shim_test.py).

The shim is embedded into the Rust crate via include_str! and runs inside
the PyPI sandbox container, so it is exercised directly here rather than
through the Rust test suite.
"""

import os
import subprocess
import sys
import unittest

SHIM = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sandbox_shim.py")


class HandshakeFailureReporting(unittest.TestCase):
    def test_server_exiting_before_tools_list_reports_no_result(self):
        # `pip` is always installed and is not an MCP server: it prints usage
        # and exits, so the handshake never returns tools/list. The shim must
        # report that (exit 7 + CAPFRAME_NO_RESULT), not crash while writing
        # its own diagnostics -- a crash there hid the real cause of every
        # PyPI sandbox failure on the leaderboard.
        proc = subprocess.run(
            [sys.executable, SHIM, "pip"],
            capture_output=True,
            timeout=60,
        )
        stderr = proc.stderr.decode("utf-8", "replace")
        self.assertNotIn("TypeError", stderr)
        self.assertIn("CAPFRAME_NO_RESULT", stderr)
        self.assertEqual(proc.returncode, 7, stderr)


if __name__ == "__main__":
    unittest.main()
