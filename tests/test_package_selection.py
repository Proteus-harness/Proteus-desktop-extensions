"""Independent synthetic-IO regression tests; never starts Cargo or an extension.

The default target is ../package.py; CANDIDATE_PACKAGE can override it.
Every subprocess entrypoint is replaced with a bounded fake and socket network
connections are denied; all writable content is a tiny temporary fixture.
"""
from contextlib import ExitStack, redirect_stderr, redirect_stdout
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile


CANDIDATE = Path(os.environ.get(
    "CANDIDATE_PACKAGE",
    Path(__file__).resolve().parents[1] / "package.py",
))


class SelectedPackageReview(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="proteus-selection-review-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.packages = self.root / "packages"
        self.dist = self.root / "dist"
        self.dist.mkdir()
        for name in ("plan", "files"):
            package = self.packages / name
            (package / "src").mkdir(parents=True)
            (package / "bin").mkdir()
            (package / "extension.json").write_text(json.dumps({
                "protocol": 4, "id": name, "command": [f"bin/{name}"],
            }))
            (package / "Cargo.toml").write_text(
                f'[package]\nname = "extension-{name}"\nversion = "0.1.0"\n'
                f'[[bin]]\nname = "{name}"\npath = "src/main.rs"\n')
            (package / "src/main.rs").write_text("// fixture data; never compiled\n")
            (package / "LICENSE-GPL").write_text("license fixture\n")
            (package / "asset.txt").write_text(f"{name} asset\n")
            (package / "bin" / name).write_bytes(f"old-{name}".encode())
            (self.dist / f"{name}.zip").write_bytes(f"old-{name}-zip".encode())
        self.package = self.packages / "plan"
        self.binary = self.root / "custom-target/aarch64-unknown-linux-gnu/release/plan"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(b"selected binary fixture; NOT EXECUTABLE")
        self.binary.chmod(0o755)
        self.event = {
            "reason": "compiler-artifact",
            "package_id": f"path+file://{self.package}#extension-plan@0.1.0",
            "manifest_path": str(self.package / "Cargo.toml"),
            "target": {"kind": ["bin"], "name": "plan", "crate_types": ["bin"]},
            "profile": {"test": False, "opt_level": "3"},
            "executable": str(self.binary),
            "fresh": False,
        }
        self.finished = {"reason": "build-finished", "success": True}
        self.calls = []
        spec = importlib.util.spec_from_file_location("candidate_package_review", CANDIDATE)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        self.module.ROOT = self.root
        self.module.PACKAGES = self.packages
        self.sibling_before = self.snapshot(self.packages / "files")

    def snapshot(self, root):
        return {str(path.relative_to(root)): path.read_bytes()
                for path in root.rglob("*") if path.is_file()}

    def invoke(self, *args, events=None, status=0, raw=None):
        events = [self.event, self.finished] if events is None else events
        output = raw if raw is not None else "".join(json.dumps(e) + "\n" for e in events)
        owner = self

        def record(command, **kwargs):
            owner.assertIsInstance(command, (list, tuple))
            owner.assertEqual(list(command[:2]), ["cargo", "build"],
                              "Only a fake Cargo build is permitted")
            owner.calls.append((list(command), kwargs))

        def fake_run(command, **kwargs):
            record(command, **kwargs)
            if status and kwargs.get("check"):
                raise subprocess.CalledProcessError(status, command, output=output)
            return subprocess.CompletedProcess(command, status, stdout=output, stderr="")

        class FakePopen:
            def __init__(self, command, **kwargs):
                record(command, **kwargs)
                self.stdout = io.StringIO(output)
                self.stderr = io.StringIO("")
                self.returncode = status

            def wait(self, *args, **kwargs):
                return self.returncode

            def communicate(self, *args, **kwargs):
                return output, ""

            def poll(self):
                return self.returncode

            def kill(self):
                pass

            def terminate(self):
                pass

            def __enter__(self):
                return self

            def __exit__(self, *args):
                self.stdout.close()
                self.stderr.close()

        def forbidden(*args, **kwargs):
            raise AssertionError("Unexpected real-process API attempted")

        self.stdout = io.StringIO()
        self.stderr = io.StringIO()
        with ExitStack() as stack:
            stack.enter_context(patch.object(sys, "argv", [str(CANDIDATE), *args]))
            stack.enter_context(patch.object(subprocess, "run", fake_run))
            stack.enter_context(patch.object(subprocess, "Popen", FakePopen))
            for name in ("call", "check_call", "check_output"):
                stack.enter_context(patch.object(subprocess, name, forbidden))
            stack.enter_context(patch.object(os, "system", forbidden))
            stack.enter_context(patch.object(socket, "create_connection", forbidden))
            stack.enter_context(patch.object(socket.socket, "connect", forbidden))
            stack.enter_context(patch.object(socket.socket, "connect_ex", forbidden))
            stack.enter_context(redirect_stdout(self.stdout))
            stack.enter_context(redirect_stderr(self.stderr))
            return self.module.main()

    def assert_untouched(self, *, selected_archive=True):
        self.assertEqual(self.snapshot(self.packages / "files"), self.sibling_before)
        self.assertEqual((self.dist / "files.zip").read_bytes(), b"old-files-zip")
        if selected_archive:
            self.assertEqual((self.dist / "plan.zip").read_bytes(), b"old-plan-zip")

    def assert_selection_failure(self, *, events=None, status=0, raw=None, dev=False):
        args = ["--package", "plan"]
        args += ["--dev"] if dev else ["--output", str(self.dist)]
        with self.assertRaises((SystemExit, RuntimeError, ValueError, subprocess.CalledProcessError)):
            self.invoke(*args, events=events, status=status, raw=raw)
        self.assert_untouched()
        self.assertEqual((self.package / "bin/plan").read_bytes(), b"old-plan")

    def test_selected_zip_uses_one_exact_build_and_actual_artifact(self):
        self.invoke("--package", "plan", "--output", str(self.dist))
        self.assertEqual(len(self.calls), 1)
        command, options = self.calls[0]
        self.assertEqual(command, ["cargo", "build", "--release", "--locked", "--quiet",
                                   "-p", "extension-plan", "--bin", "plan",
                                   "--message-format=json-render-diagnostics"])
        self.assertEqual(options["cwd"], self.root)
        self.assertFalse(options.get("shell", False))
        self.assertNotIn("env", options, "Do not inject optimization/profile overrides")
        with zipfile.ZipFile(self.dist / "plan.zip") as archive:
            self.assertEqual(set(archive.namelist()),
                             {"extension.json", "LICENSE-GPL", "asset.txt", "bin/plan"})
            self.assertEqual(archive.read("bin/plan"), self.binary.read_bytes())
        self.assert_untouched(selected_archive=False)
        self.assertEqual((self.package / "bin/plan").read_bytes(), b"old-plan")

    def test_fresh_true_is_a_valid_selected_artifact(self):
        self.event["fresh"] = True
        self.invoke("--package", "plan", "--dev")
        self.assertEqual((self.package / "bin/plan").read_bytes(), self.binary.read_bytes())
        self.assert_untouched()

    def test_unrelated_artifacts_do_not_override_selected(self):
        other = copy.deepcopy(self.event)
        other["manifest_path"] = str(self.packages / "files/Cargo.toml")
        other["package_id"] = "path+file:///different#extension-files@0.1.0"
        other["executable"] = str(self.packages / "files/bin/files")
        self.invoke("--package", "plan", "--dev", events=[other, self.event, other, self.finished])
        self.assertEqual((self.package / "bin/plan").read_bytes(), self.binary.read_bytes())
        self.assert_untouched()

    def test_wrong_manifest_cannot_impersonate_same_named_bin(self):
        self.event["manifest_path"] = str(self.packages / "files/Cargo.toml")
        self.assert_selection_failure()

    def test_wrong_name_is_rejected(self):
        self.event["target"]["name"] = "files"
        self.assert_selection_failure()

    def test_wrong_target_kind_is_rejected(self):
        for kind in (["lib"], ["example"], ["test"], ["custom-build"], ["bin", "example"]):
            with self.subTest(kind=kind):
                self.event["target"]["kind"] = kind
                self.assert_selection_failure()

    def test_test_profile_is_rejected(self):
        self.event["profile"]["test"] = True
        self.assert_selection_failure()

    def test_missing_profile_test_identity_is_rejected(self):
        del self.event["profile"]["test"]
        self.assert_selection_failure()

    def test_missing_executable_is_rejected(self):
        self.event["executable"] = None
        self.assert_selection_failure()

    def test_nonexistent_executable_is_rejected(self):
        self.event["executable"] = str(self.binary.parent / "missing")
        self.assert_selection_failure()

    def test_executable_directory_is_rejected(self):
        self.event["executable"] = str(self.binary.parent)
        self.assert_selection_failure()

    def test_two_distinct_matching_outputs_are_rejected(self):
        other = copy.deepcopy(self.event)
        other_binary = self.binary.with_name("other-plan")
        other_binary.write_bytes(b"second artifact")
        other["executable"] = str(other_binary)
        self.assert_selection_failure(events=[self.event, other, self.finished])

    def test_successful_json_does_not_override_nonzero_cargo_exit(self):
        self.assert_selection_failure(status=9)

    def test_failed_build_finished_is_rejected(self):
        self.finished["success"] = False
        self.assert_selection_failure()

    def test_numeric_truthy_build_finished_is_rejected(self):
        self.finished["success"] = 1
        self.assert_selection_failure()

    def test_failed_build_forwards_compiler_diagnostics(self):
        diagnostic = {"reason": "compiler-message", "message": {"rendered": "error: fixture compiler failure\n"}}
        self.assert_selection_failure(events=[diagnostic, self.event, self.finished], status=9)
        self.assertIn("error: fixture compiler failure\n", self.stderr.getvalue())

    def test_malformed_json_fails_but_later_diagnostic_is_preserved(self):
        diagnostic = {"reason": "compiler-message", "message": {"rendered": "warning: fixture diagnostic\n"}}
        raw = "{malformed\n" + "\n".join(json.dumps(e) for e in (diagnostic, self.event, self.finished))
        self.assert_selection_failure(raw=raw)
        self.assertIn("warning: fixture diagnostic\n", self.stderr.getvalue())

    def test_non_json_tool_log_does_not_break_valid_build(self):
        raw = "unstructured build-tool log\n" + "\n".join(json.dumps(e) for e in (self.event, self.finished))
        self.invoke("--package", "plan", "--dev", raw=raw)
        self.assertEqual((self.package / "bin/plan").read_bytes(), self.binary.read_bytes())
        self.assert_untouched()

    def test_missing_build_finished_is_rejected(self):
        self.assert_selection_failure(events=[self.event])

    def test_duplicate_build_finished_is_rejected(self):
        self.assert_selection_failure(events=[self.event, self.finished, self.finished])

    def test_missing_artifact_is_rejected(self):
        self.assert_selection_failure(events=[self.finished])

    def test_empty_cargo_stream_is_rejected(self):
        self.assert_selection_failure(events=[])

    def test_unknown_and_traversal_selection_fail_before_subprocess(self):
        for name in ("unknown", "../plan", ".", "", "plan/../files", "plan*", "[pf]*"):
            with self.subTest(name=name):
                self.calls.clear()
                with self.assertRaises((SystemExit, ValueError)):
                    self.invoke("--package", name, "--output", str(self.dist))
                self.assertEqual(self.calls, [])
                self.assert_untouched()

    def test_bundle_plus_selection_fails_before_subprocess(self):
        with self.assertRaises(SystemExit):
            self.invoke("--package", "plan", "--bundle", str(self.root / "bundle"))
        self.assertEqual(self.calls, [])
        self.assertFalse((self.root / "bundle").exists())

    def test_symlink_package_cannot_escape_packages(self):
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "extension.json").write_text('{"id":"escape","command":["bin/escape"]}')
        (self.packages / "escape").symlink_to(outside, target_is_directory=True)
        with self.assertRaises((SystemExit, ValueError)):
            self.invoke("--package", "escape", "--output", str(self.dist))
        self.assertEqual(self.calls, [])
        self.assert_untouched()

    def test_missing_license_preserves_previous_archive(self):
        (self.package / "LICENSE-GPL").unlink()
        self.assert_selection_failure()

    def test_dev_build_failure_preserves_previous_binary(self):
        self.assert_selection_failure(events=[self.event], dev=True)

    def test_invalid_command_cannot_escape_selected_package(self):
        for command in (["bin/../../escape"], ["/absolute/program"], [], ["bin/*"]):
            with self.subTest(command=command):
                self.calls.clear()
                (self.package / "extension.json").write_text(json.dumps({"id":"plan", "command":command}))
                with self.assertRaises((SystemExit, ValueError)):
                    self.invoke("--package", "plan", "--output", str(self.dist))
                self.assertEqual(self.calls, [])
                self.assert_untouched()

    def test_duplicate_selection_fails_before_subprocess(self):
        with self.assertRaises(SystemExit):
            self.invoke("--package", "plan", "--package", "files", "--output", str(self.dist))
        self.assertEqual(self.calls, [])
        self.assert_untouched()

    def test_dev_output_conflict_fails_before_subprocess(self):
        with self.assertRaises(SystemExit):
            self.invoke("--package", "plan", "--dev", "--output", str(self.dist))
        self.assertEqual(self.calls, [])
        self.assert_untouched()

    def test_dev_bin_directory_symlink_fails_before_subprocess(self):
        outside = self.root / "outside"
        (self.package / "bin").rename(outside)
        (self.package / "bin").symlink_to(outside, target_is_directory=True)
        with self.assertRaises(SystemExit):
            self.invoke("--package", "plan", "--dev")
        self.assertEqual(self.calls, [])
        self.assertEqual((outside / "plan").read_bytes(), b"old-plan")
        self.assert_untouched()

    def test_manifest_symlinks_fail_before_subprocess(self):
        for filename in ("Cargo.toml", "extension.json"):
            with self.subTest(filename=filename):
                target = self.root / f"outside-{filename}"
                original = self.package / filename
                original.rename(target)
                original.symlink_to(target)
                try:
                    with self.assertRaises(SystemExit):
                        self.invoke("--package", "plan", "--dev")
                    self.assertEqual(self.calls, [])
                finally:
                    original.unlink()
                    target.rename(original)

    def test_dev_partial_copy_failure_preserves_old_binary(self):
        def partial_copy(source, destination, *args, **kwargs):
            Path(destination).write_bytes(b"partial output")
            raise OSError("injected copy failure")
        with patch.object(self.module.shutil, "copy2", partial_copy):
            with self.assertRaises(OSError):
                self.invoke("--package", "plan", "--dev")
        self.assertEqual((self.package / "bin/plan").read_bytes(), b"old-plan")
        self.assertEqual(sorted(p.name for p in (self.package / "bin").iterdir()), ["plan"])
        self.assert_untouched()

    def test_zip_partial_write_failure_preserves_old_archive(self):
        original = zipfile.ZipFile.writestr
        def partial_write(archive, *args, **kwargs):
            original(archive, *args, **kwargs)
            raise OSError("injected ZIP failure")
        with patch.object(zipfile.ZipFile, "writestr", partial_write):
            with self.assertRaises(OSError):
                self.invoke("--package", "plan", "--output", str(self.dist))
        self.assert_untouched()
        self.assertEqual(sorted(p.name for p in self.dist.iterdir()), ["files.zip", "plan.zip"])

    def test_failed_atomic_replace_preserves_old_outputs(self):
        for dev in (False, True):
            with self.subTest(dev=dev):
                with patch.object(os, "replace", side_effect=OSError("injected replace failure")):
                    with self.assertRaises(OSError):
                        self.invoke("--package", "plan", *(["--dev"] if dev else ["--output", str(self.dist)]))
                self.assert_untouched()
                self.assertEqual((self.package / "bin/plan").read_bytes(), b"old-plan")
                self.assertEqual(sorted(p.name for p in self.dist.iterdir()), ["files.zip", "plan.zip"])
                self.assertEqual(sorted(p.name for p in (self.package / "bin").iterdir()), ["plan"])

    def test_source_archive_retains_binary_executable_mode(self):
        self.invoke("--package", "plan", "--output", str(self.dist))
        with zipfile.ZipFile(self.dist / "plan.zip") as archive:
            mode = archive.getinfo("bin/plan").external_attr >> 16
            self.assertEqual(mode & 0o111, 0o111)

    def test_default_mode_still_builds_all_once(self):
        target = self.root / "target"
        (target / "release").mkdir(parents=True)
        for name in ("plan", "files"):
            (target / "release" / name).write_bytes(b"stale default output")
        events = []
        for name in ("plan", "files"):
            binary = self.binary.parent / name
            binary.write_bytes(f"new default {name}".encode())
            events.append(dict(self.event, manifest_path=str(self.packages / name / "Cargo.toml"),
                               target={"kind": ["bin"], "name": name}, executable=str(binary)))
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": str(target)}):
            self.invoke("--output", str(self.dist), events=[*events, self.finished])
        self.assertEqual(len(self.calls), 1)
        self.assertEqual(self.calls[0][0], ["cargo", "build", "--release", "--locked", "--quiet",
                                          "--message-format=json-render-diagnostics"])
        for name in ("plan", "files"):
            with zipfile.ZipFile(self.dist / f"{name}.zip") as archive:
                self.assertEqual(archive.read(f"bin/{name}"), f"new default {name}".encode())

    def test_no_build_reuses_complete_set_without_cargo(self):
        target = self.root / "relative cache/release"
        target.mkdir(parents=True)
        for name in ("plan", "files"):
            (target / name).write_bytes(f"cached {name}".encode())
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": "relative cache"}):
            self.invoke("--no-build", "--output", str(self.dist))
        self.assertEqual(self.calls, [])
        for name in ("plan", "files"):
            with zipfile.ZipFile(self.dist / f"{name}.zip") as archive:
                self.assertEqual(archive.read(f"bin/{name}"), f"cached {name}".encode())

    def test_selected_no_build_rejected_before_cargo_or_output(self):
        with self.assertRaises(SystemExit):
            self.invoke("--package", "plan", "--no-build", "--output", str(self.dist))
        self.assertEqual(self.calls, [])
        self.assert_untouched()


if __name__ == "__main__":
    unittest.main(verbosity=2)
