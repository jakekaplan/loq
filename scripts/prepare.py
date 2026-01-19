#!/usr/bin/env python3
"""Prepare a release by updating version numbers and creating a PR."""

import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).parent.parent

VERSION_FILES = [
    "Cargo.toml",
    "Cargo.lock",
    "crates/loq_cli/Cargo.toml",
    "crates/loq_fs/Cargo.toml",
    "pyproject.toml",
    "README.md",
]


def run(cmd: list[str], capture: bool = False) -> str:
    result = subprocess.run(cmd, capture_output=capture, text=True, cwd=REPO_ROOT)
    if result.returncode != 0:
        if capture and result.stderr:
            print(result.stderr, file=sys.stderr)
        sys.exit(1)
    return result.stdout.strip() if capture else ""


def parse_pep440(version: str) -> tuple[str, str | None, int | None]:
    """Parse PEP 440 version, return (base, prerelease_type, prerelease_num).

    Requires numeric suffix for prerelease markers (e.g., 1.0.0a1, not 1.0.0a).
    """
    # Try prerelease format first (requires digits after a/b/rc)
    match = re.match(r"^(\d+\.\d+\.\d+)(a|b|rc)(\d+)$", version)
    if match:
        return (match.group(1), match.group(2), int(match.group(3)))
    # Try stable format (no prerelease marker)
    match = re.match(r"^(\d+\.\d+\.\d+)$", version)
    if match:
        return (match.group(1), None, None)
    return ("", None, None)


def pep440_to_semver(version: str) -> str:
    """Convert PEP 440 version to semver."""
    base, pre_type, pre_num = parse_pep440(version)
    if not base:
        return ""
    if pre_type is None:
        return base
    type_map = {"a": "alpha", "b": "beta", "rc": "rc"}
    return f"{base}-{type_map[pre_type]}.{pre_num}"


def find_version_in_section(content: str, section: str) -> tuple[str, int, int] | None:
    """Find version in a TOML section, handling arrays correctly.

    Returns (version, start_pos, end_pos) of the version value (including quotes),
    or None if not found.
    """
    # Find section header
    section_pattern = re.escape(f"[{section}]")
    section_match = re.search(f"^{section_pattern}$", content, re.MULTILINE)
    if not section_match:
        return None

    # Find where this section ends (next section header at start of line)
    section_start = section_match.end()
    next_section = re.search(r"^\[", content[section_start:], re.MULTILINE)
    section_end = section_start + next_section.start() if next_section else len(content)
    section_content = content[section_start:section_end]

    # Find version = "..." within this section
    version_match = re.search(r'^version = "([^"]+)"', section_content, re.MULTILINE)
    if not version_match:
        return None

    version = version_match.group(1)
    # Calculate absolute positions of the quoted version value
    abs_start = section_start + version_match.start(1) - 1  # -1 for opening quote
    abs_end = section_start + version_match.end(1) + 1  # +1 for closing quote
    return (version, abs_start, abs_end)


def check_working_tree():
    print("Checking working tree...", end=" ", flush=True)
    status = run(["git", "status", "--porcelain"], capture=True)
    if status:
        print("dirty")
        print("Error: Working tree is not clean", file=sys.stderr)
        sys.exit(1)
    print("clean")


def validate_version(version: str) -> str:
    print(f"Validating version {version}...", end=" ", flush=True)
    semver = pep440_to_semver(version)
    if not semver:
        print("invalid")
        print(
            "Error: Invalid version format. Expected PEP 440 (e.g., 0.1.0a7, 1.0.0, 2.0.0rc1)",
            file=sys.stderr,
        )
        sys.exit(1)
    print(f"ok (semver: {semver})")
    return semver


def update_cargo_toml(semver: str):
    path = REPO_ROOT / "Cargo.toml"
    content = path.read_text()
    result = find_version_in_section(content, "workspace.package")
    if not result:
        print("Error: Could not find version in [workspace.package]", file=sys.stderr)
        sys.exit(1)
    old_version, start, end = result
    print(f"Updating Cargo.toml: {old_version} → {semver}")
    new_content = content[:start] + f'"{semver}"' + content[end:]
    path.write_text(new_content)


def update_crate_deps(crate_path: str, semver: str):
    path = REPO_ROOT / crate_path
    print(f"Updating {crate_path}...")
    content = path.read_text()
    new_content, count = re.subn(
        r'(loq_(?:core|fs) = \{ path = "[^"]+", version = )"[^"]+"',
        rf'\1"{semver}"',
        content,
    )
    if count == 0:
        print(f"Error: No dependency versions found to update in {crate_path}", file=sys.stderr)
        sys.exit(1)
    path.write_text(new_content)


def update_pyproject(pep440: str):
    path = REPO_ROOT / "pyproject.toml"
    content = path.read_text()
    result = find_version_in_section(content, "project")
    if not result:
        print("Error: Could not find version in [project]", file=sys.stderr)
        sys.exit(1)
    old_version, start, end = result
    print(f"Updating pyproject.toml: {old_version} → {pep440}")
    new_content = content[:start] + f'"{pep440}"' + content[end:]
    path.write_text(new_content)


def update_readme(semver: str):
    """Update README.md pre-commit rev."""
    print("Updating README.md pre-commit rev...")
    path = REPO_ROOT / "README.md"
    content = path.read_text()
    lines = content.split("\n")
    found = False
    for i, line in enumerate(lines):
        if "repo: https://github.com/jakekaplan/loq" in line:
            # Search for rev: within the next few lines (handles comments/blank lines)
            for j in range(i + 1, min(i + 5, len(lines))):
                if "rev:" in lines[j]:
                    found = True
                    lines[j] = re.sub(r"rev: v[^\s]+", f"rev: v{semver}", lines[j])
                    break
                # Stop if we hit another repo: or end of YAML block
                if "repo:" in lines[j] or lines[j].strip() == "```":
                    break
            break
    if not found:
        print("Error: Could not find README.md pre-commit rev to update", file=sys.stderr)
        sys.exit(1)
    path.write_text("\n".join(lines))


def main():
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <version>", file=sys.stderr)
        print("Example: python scripts/prepare.py 0.1.0a7", file=sys.stderr)
        sys.exit(1)

    pep440_version = sys.argv[1]

    check_working_tree()
    semver = validate_version(pep440_version)

    branch = f"prep-{pep440_version}"
    print(f"Creating branch {branch}...")
    run(["git", "checkout", "-b", branch])

    update_cargo_toml(semver)
    update_crate_deps("crates/loq_cli/Cargo.toml", semver)
    update_crate_deps("crates/loq_fs/Cargo.toml", semver)
    update_pyproject(pep440_version)
    update_readme(semver)

    print("Running cargo update -p loq_core -p loq_fs -p loq...")
    run(["cargo", "update", "-p", "loq_core", "-p", "loq_fs", "-p", "loq"])

    print("Committing changes...")
    run(["git", "add"] + [str(REPO_ROOT / f) for f in VERSION_FILES])
    run(["git", "commit", "-m", f"prep-{pep440_version}"])

    print("Pushing branch...")
    run(["git", "push", "-u", "origin", branch])

    print("Creating PR...")
    result = run(
        [
            "gh",
            "pr",
            "create",
            "--title",
            f"prep-{pep440_version}",
            "--body",
            f"Bumps version to {pep440_version}",
        ],
        capture=True,
    )
    print(f"Done: {result}")


if __name__ == "__main__":
    main()
