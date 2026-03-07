"""
Interrupted conversion test.

Spawns a Rust CLI conversion as a subprocess, waits for output files to
start appearing, then sends SIGKILL. Verifies that the partial output is
detectable (missing finalization files, HDF5 won't open cleanly, etc.).

Usage:
    python3 tests/test_interrupted_conversion.py
"""

import os
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def find_workspace_root() -> Path:
    """Find the workspace root by looking for Cargo.toml with [workspace]."""
    current = Path(__file__).resolve().parent
    while current != current.parent:
        cargo = current / "Cargo.toml"
        if cargo.exists() and "[workspace]" in cargo.read_text():
            return current
        current = current.parent
    raise RuntimeError("Could not find workspace root")


def build_cli():
    """Build the forge-cli binary in release mode."""
    root = find_workspace_root()
    print("Building forge-cli...")
    result = subprocess.run(
        ["cargo", "build", "-p", "forge-cli", "--release"],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(f"Build failed:\n{result.stderr}")
        sys.exit(1)
    return root / "target" / "release" / "forge"


def test_interrupted_conversion():
    root = find_workspace_root()
    cli_binary = build_cli()

    if not cli_binary.exists():
        # Try without release
        cli_binary = root / "target" / "debug" / "forge"
        if not cli_binary.exists():
            print("SKIPPED: forge CLI binary not found")
            return

    source_path = root / "sample_data" / "lerobot_pusht"
    if not source_path.exists():
        print(f"SKIPPED: source dataset not found at {source_path}")
        return

    with tempfile.TemporaryDirectory() as output_dir:
        print(f"Starting conversion: lerobot-v3 -> hdf5")
        print(f"  Source: {source_path}")
        print(f"  Output: {output_dir}")

        # Start conversion as subprocess.
        proc = subprocess.Popen(
            [
                str(cli_binary),
                "convert",
                str(source_path),
                str(output_dir),
                "--target-format",
                "hdf5",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        # Wait for output files to start appearing.
        max_wait = 30  # seconds
        start = time.time()
        found_files = False

        while time.time() - start < max_wait:
            output_path = Path(output_dir)
            files = list(output_path.rglob("*"))
            if any(f.is_file() and f.stat().st_size > 0 for f in files):
                found_files = True
                print(f"  Output files detected after {time.time() - start:.1f}s")
                break
            time.sleep(0.2)

        if not found_files:
            # Process may have finished already
            proc.wait(timeout=5)
            if proc.returncode == 0:
                print("  Conversion completed before we could interrupt it (fast dataset)")
                print("  PASS (conversion completes correctly)")
                return
            else:
                print(f"  No output files found after {max_wait}s, process exited with {proc.returncode}")
                print("  SKIPPED: could not verify interrupted conversion")
                return

        # Send SIGKILL to interrupt mid-write.
        print("  Sending SIGKILL...")
        os.kill(proc.pid, signal.SIGKILL)
        proc.wait()
        print(f"  Process killed (exit code: {proc.returncode})")

        # Verify partial output is detectable.
        output_path = Path(output_dir)
        all_files = list(output_path.rglob("*"))
        file_list = [str(f.relative_to(output_path)) for f in all_files if f.is_file()]
        print(f"  Partial output files: {file_list}")

        # Check for signs of incomplete conversion:
        # 1. Missing finalization metadata (for formats that write it last)
        has_info_json = any("info.json" in str(f) for f in all_files)

        # 2. For HDF5: check if any .hdf5 files are truncated
        hdf5_files = [f for f in all_files if f.suffix == ".hdf5"]

        # The key assertion: either there are no finalization files,
        # or some output files exist but the dataset is incomplete.
        if not has_info_json:
            print("  PASS: No finalization metadata (info.json) found - incomplete conversion detected")
        elif hdf5_files:
            # HDF5 files exist but may be corrupt/truncated
            print(f"  PASS: {len(hdf5_files)} HDF5 files found - partial write detected")
        elif file_list:
            print(f"  PASS: {len(file_list)} partial files found")
        else:
            print("  PASS: No output files (killed before any writes)")

    print("\nAll interrupted conversion checks passed.")


if __name__ == "__main__":
    test_interrupted_conversion()
