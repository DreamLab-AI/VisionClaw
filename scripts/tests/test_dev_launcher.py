"""Exercise the real launcher without building the GPU backend."""
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]


class DevelopmentLauncher(unittest.TestCase):
    def test_build_restart_and_failed_build(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "bin").mkdir()
            (root / "src").mkdir()
            (root / "src/main.rs").write_text("fn main() {}")
            os.utime(root / "src/main.rs", (1, 1))
            (root / "errors.log").write_text("previous fatal diagnostic\n")
            for name, content in {
                "nvidia-smi": "#!/bin/sh\necho 8.9\n",
                "cargo": '''#!/bin/bash
echo "$*" >> "$APP_ROOT/cargo.calls"
[[ "$*" == "clean" ]] && exit 0
[[ "${FAIL_BUILD:-0}" == 1 ]] && exit 1
[[ "$*" == "build --profile dev-runtime --features gpu,ontology,dev-auth" ]] || exit 5
[[ "$CARGO_PROFILE_DEV_RUNTIME_DEBUG_ASSERTIONS" == true ]] || exit 6
mkdir -p "$APP_ROOT/target/dev-runtime"
printf '#!/bin/sh\\necho backend-started\\n' > "$APP_ROOT/target/dev-runtime/visionclaw-server"
chmod +x "$APP_ROOT/target/dev-runtime/visionclaw-server"
''',
            }.items():
                file = root / "bin" / name
                file.write_text(content)
                file.chmod(0o755)
            env = dict(os.environ, APP_ROOT=str(root),
                       PATH=f"{root / 'bin'}:{os.environ['PATH']}",
                       RUST_ERROR_LOG=str(root / "errors.log"),
                       SKIP_RUST_REBUILD="false", BUILD_FEATURES="gpu,ontology,dev-auth")
            for key in ("RUST_BINARY", "BUILD_STAMP", "FAIL_BUILD"):
                env.pop(key, None)
            def run():
                return subprocess.run(["bash", str(ROOT / "scripts/rust-backend-wrapper.sh")],
                                      env=env, capture_output=True, text=True)
            first = run()
            self.assertEqual(first.returncode, 0, first.stdout + first.stderr)
            self.assertIn("backend-started", first.stdout)
            second = run()
            self.assertEqual(second.returncode, 0, second.stdout + second.stderr)
            self.assertIn("Skipping cargo", second.stdout)
            self.assertEqual(len((root / "cargo.calls").read_text().splitlines()), 1)
            self.assertEqual((root / "errors.log").read_text(), "previous fatal diagnostic\n")
            (root / "target/.visionclaw-dev-runtime-build-stamp").write_text("old release identity")
            env["FAIL_BUILD"] = "1"
            failed = run()
            self.assertNotEqual(failed.returncode, 0)
            self.assertNotIn("backend-started", failed.stdout)

    def test_development_identity_and_shared_entrypoint(self):
        manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
        profile = manifest["profile"]["dev-runtime"]
        self.assertEqual(profile["inherits"], "release")
        self.assertIs(profile["debug-assertions"], True)
        entrypoint = (ROOT / "scripts/dev-entrypoint.sh").read_text()
        self.assertIn("/app/scripts/rust-backend-wrapper.sh >>", entrypoint)
        self.assertNotIn("cargo build --release", entrypoint)


if __name__ == "__main__":
    unittest.main()
