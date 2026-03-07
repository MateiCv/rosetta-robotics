"""
Real-world dataset breadth test.

Downloads diverse HuggingFace datasets and verifies the Rust CLI can convert
them to lerobot-v3 format.

Requirements:
    pip install huggingface_hub

Usage:
    python3 tests/test_real_datasets.py
"""

import subprocess
import sys
import tempfile
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


def build_cli(root: Path) -> Path:
    """Build the forge-cli binary."""
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

    binary = root / "target" / "release" / "forge"
    if not binary.exists():
        binary = root / "target" / "debug" / "forge"
    return binary


# Datasets to test: (repo_id, expected_source_format)
DATASETS = [
    ("lerobot/aloha_sim_transfer_cube_human", "lerobot-v2"),
]


def test_real_dataset(cli_binary: Path, repo_id: str, expected_format: str):
    """Download a dataset and test conversion to lerobot-v3."""
    print(f"\n{'=' * 60}")
    print(f"Testing: {repo_id} (expected: {expected_format})")
    print(f"{'=' * 60}")

    try:
        from huggingface_hub import snapshot_download
    except ImportError:
        print("SKIPPED: huggingface_hub not installed (pip install huggingface_hub)")
        return True

    with tempfile.TemporaryDirectory() as tmpdir:
        download_dir = Path(tmpdir) / "input"
        output_dir = Path(tmpdir) / "output"
        output_dir.mkdir()

        # Download dataset.
        print(f"  Downloading {repo_id}...")
        try:
            local_path = snapshot_download(
                repo_id=repo_id,
                repo_type="dataset",
                local_dir=str(download_dir),
                ignore_patterns=["*.md", "*.txt", "*.gitattributes"],
            )
        except Exception as e:
            print(f"  SKIPPED: download failed: {e}")
            return True  # Don't fail on download issues

        print(f"  Downloaded to: {local_path}")

        # Convert to lerobot-v3.
        print(f"  Converting to lerobot-v3...")
        result = subprocess.run(
            [
                str(cli_binary),
                "convert",
                str(local_path),
                str(output_dir),
                "--target-format",
                "lerobot-v3",
            ],
            capture_output=True,
            text=True,
            timeout=300,  # 5 minute timeout
        )

        if result.returncode != 0:
            print(f"  FAILED: conversion failed (exit code {result.returncode})")
            print(f"  stderr: {result.stderr[:500]}")
            return False

        # Verify output.
        info_json = output_dir / "meta" / "info.json"
        if not info_json.exists():
            print(f"  FAILED: meta/info.json not found in output")
            return False

        import json
        info = json.loads(info_json.read_text())
        print(f"  Output info:")
        print(f"    total_episodes: {info.get('total_episodes')}")
        print(f"    total_frames: {info.get('total_frames')}")
        print(f"    codebase_version: {info.get('codebase_version')}")

        if info.get("total_episodes", 0) == 0:
            print(f"  FAILED: 0 episodes in output")
            return False

        print(f"  PASS")
        return True


def main():
    root = find_workspace_root()
    cli_binary = build_cli(root)

    if not cli_binary.exists():
        print(f"ERROR: CLI binary not found at {cli_binary}")
        sys.exit(1)

    results = []
    for repo_id, expected_format in DATASETS:
        success = test_real_dataset(cli_binary, repo_id, expected_format)
        results.append((repo_id, success))

    print(f"\n{'=' * 60}")
    print("Summary:")
    for repo_id, success in results:
        status = "PASS" if success else "FAIL"
        print(f"  [{status}] {repo_id}")

    failures = [r for r in results if not r[1]]
    if failures:
        print(f"\n{len(failures)} test(s) failed.")
        sys.exit(1)
    else:
        print(f"\nAll {len(results)} test(s) passed.")


if __name__ == "__main__":
    main()
