#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Stamp a version onto a change (portable kit, PM 2026-11-12; derived from Unpopped's tools/stamp-version.py).

    python tools/stamp-version.py <version> [--root DIR] [--dry-run]

A PR branch adds only its own files plus a CHANGELOG fragment (`changelog.d/<topic>.md`, format in `changelog.d/README.md`)
and never edits `CHANGELOG.md`, a `Cargo.toml` version or `Cargo.lock`. When the PM allocates the number the gate runs this
script, which

 1. finds every `Cargo.toml` tracked by git whose `[package]` version is the project's current version (one version per
    project) and sets the new one, including `path`+`version` dependency lines and the matching `Cargo.lock` entries;
 2. sets `[workspace.package] version` when the root uses it;
 3. turns the fragments into one entry above the newest CHANGELOG entry (heading style copied from the file);
 4. deletes the fragments it used.

An optional `tools/stamp-current.txt` (one line: a Cargo.toml path) names the manifest whose version is "current" when the
repo has crates on independent versions; only crates at that version are stamped.

Extra files that pin the version as `=X.Y.Z` (README install lines) are listed one path per line in
`tools/stamp-extra.txt` (optional); a line `path ~ REGEX` stamps a present-tense statement (REGEX has `{old}` and ONE group
kept before the version). Standard library only. Line endings of each file are kept. Files are enumerated with
`git ls-files`, never a disk walk.
"""
import argparse
import datetime
import os
import re
import shutil
import subprocess  # nosec B404 - runs `git ls-files` with fixed arguments in a developer or CI checkout
import sys

EM = "—"
NAME_RX = re.compile(r'(?m)^\s*name\s*=\s*"([^"]+)"')
VERSION_RX = r'(?m)^\s*version\s*=\s*"(\d+\.\d+\.\d+[^"]*)"'


def read(path):
    with open(path, "rb") as f:
        raw = f.read().decode("utf-8", errors="surrogateescape")
    return raw.replace("\r\n", "\n"), "\r\n" in raw


def write(path, text, crlf):
    # Encode BEFORE opening (a failed encode must not truncate the file); non-UTF-8 bytes survive via surrogateescape.
    data = (text.replace("\n", "\r\n") if crlf else text).encode("utf-8", errors="surrogateescape")
    with open(path, "wb") as f:
        f.write(data)


def die(msg):
    print(f"stamp-version: {msg}", file=sys.stderr)
    sys.exit(2)


def tracked(root, pattern):
    git = shutil.which("git") or die("git is not on PATH")
    out = subprocess.run([git, "-C", root, "ls-files", pattern], capture_output=True, text=True, check=False)  # nosec B603 - fixed arguments
    if out.returncode != 0:
        die("git ls-files failed: " + out.stderr.strip())
    return [line for line in out.stdout.splitlines() if line]


def table_match(text, table):
    return re.search(r"(?ms)^\[" + re.escape(table) + r"\]\s*$(.*?)(?=^\[|\Z)", text)


def package_version(text):
    """Version in the [package] table, or None; 'workspace' when `version.workspace = true`."""
    m = table_match(text, "package")
    if not m:
        return None
    v = re.search(VERSION_RX, m.group(1))
    if v:
        return v.group(1)
    return "workspace" if re.search(r"(?m)^\s*version\.workspace\s*=\s*true", m.group(1)) else None


def workspace_version(text):
    m = table_match(text, "workspace.package")
    v = re.search(VERSION_RX, m.group(1)) if m else None
    return v.group(1) if v else None


def literal(version):
    return version if version and version != "workspace" else None


def pinned_version(root):
    pin = os.path.join(root, "tools", "stamp-current.txt")
    if not os.path.exists(pin):
        return None
    rel = read(pin)[0].strip().splitlines()[0].strip()
    text, _ = read(os.path.join(root, rel))
    v = literal(package_version(text)) or workspace_version(text)
    return v or die(f"tools/stamp-current.txt names {rel}, which has no literal version")


def current_version(root):
    v = pinned_version(root)
    if v:
        return v
    top, _ = read(os.path.join(root, "Cargo.toml"))
    v = workspace_version(top) or literal(package_version(top))
    for rel in ([] if v else tracked(root, "*Cargo.toml")):
        v = v or literal(package_version(read(os.path.join(root, rel))[0]))
    return v or die("no crate version found")


def set_table_version(text, table, old, new):
    """Replace `version = "old"` inside one [table]."""
    m = table_match(text, table)
    if not m:
        return text
    body = re.sub(r'(?m)^(\s*version\s*=\s*")' + re.escape(old) + '"', lambda x: x.group(1) + new + '"', m.group(1))
    return text[: m.start(1)] + body + text[m.end(1):]


def stamp_dep_lines(t, q, new):
    """Inline-table dependency lines carrying `path` and `version`, in either order (an `=` pin is kept)."""
    t = re.sub(
        r'(?m)^(\s*[\w.-]+\s*=\s*\{[^}\n]*path\s*=\s*"[^"]+"[^}\n]*version\s*=\s*"=?)' + q + '"',
        lambda m: m.group(1) + new + '"',
        t,
    )
    return re.sub(
        r'(?m)^(\s*[\w.-]+\s*=\s*\{[^}\n]*version\s*=\s*"=?)' + q + r'("[^}\n]*path\s*=)',
        lambda m: m.group(1) + new + m.group(2),
        t,
    )


def stamp_manifest(t, old, new, ws_old):
    """One Cargo.toml: returns (new text, crate name when this crate carries the version else None)."""
    pv = package_version(t)
    nm = NAME_RX.search(t)
    carries = pv == old or (pv == "workspace" and ws_old)
    if pv == old:
        t = set_table_version(t, "package", old, new)
    if workspace_version(t) == old:
        t = set_table_version(t, "workspace.package", old, new)
    return stamp_dep_lines(t, re.escape(old), new), (nm.group(1) if carries and nm else None)


def manifest_changes(root, old, new):
    out, names = [], []
    ws_old = workspace_version(read(os.path.join(root, "Cargo.toml"))[0]) == old
    for rel in tracked(root, "*Cargo.toml"):
        p = os.path.join(root, rel)
        t, crlf = read(p)
        t2, name = stamp_manifest(t, old, new, ws_old)
        names.extend([name] if name else [])
        if t2 != t:
            out.append((p, t2, crlf))
    return out, names


def stamp_lock(t, names, q, new):
    for nm in names:
        t = re.sub(r'(name = "' + re.escape(nm) + r'"\nversion = ")' + q + '"', lambda m: m.group(1) + new + '"', t)
        # dependency refs `"name OLD"` (written when the lock holds two versions of the name)
        t = re.sub(r'(?m)^(\s*")' + re.escape(nm) + " " + q + r'(")', lambda m: m.group(1) + nm + " " + new + m.group(2), t)
    return t


def lock_changes(root, names, old, new):
    out, q = [], re.escape(old)
    root_lock = os.path.join(root, "Cargo.lock")
    if os.path.exists(root_lock) and not names:
        die("Cargo.lock exists but no crate was identified as carrying the version")
    for rel in tracked(root, "*Cargo.lock"):
        p = os.path.join(root, rel)
        t, crlf = read(p)
        t2 = stamp_lock(t, names, q, new)
        if t2 == t and p == root_lock:
            die("Cargo.lock: no workspace crate entry carries the old version")
        if t2 != t:
            out.append((p, t2, crlf))
    return out


def extra_change(root, line, q, new):
    """One line of tools/stamp-extra.txt: `path` (an `=old` pin) or `path ~ REGEX` (a present-tense statement)."""
    path, _, rx = line.partition(" ~ ")
    p = os.path.join(root, path.strip())
    t, crlf = read(p)
    if not rx:
        t2, n = re.subn(r"=" + q + r"\b", "=" + new, t)
        return (p, t2, crlf) if n else die(f"{path}: no =old pin")
    t2, n = re.subn(rx.strip().replace("{old}", q) + r"(?![\w.-])", lambda m: m.group(1) + new, t)
    return (p, t2, crlf) if n == 1 else die(f"{path}: expected exactly one match for the version statement, found {n}")


def extra_changes(root, old, new):
    extra = os.path.join(root, "tools", "stamp-extra.txt")
    if not os.path.exists(extra):
        return []
    lines = [line.strip() for line in read(extra)[0].splitlines()]
    return [extra_change(root, line, re.escape(old), new) for line in lines if line and not line.startswith("#")]


def version_changes(root, old, new):
    out, names = manifest_changes(root, old, new)
    out += lock_changes(root, names, old, new)
    out += extra_changes(root, old, new)
    return out or die("nothing carries the old version")


def fragments(directory):
    if not os.path.isdir(directory):
        return []
    return [os.path.join(directory, n) for n in sorted(os.listdir(directory)) if n.endswith(".md") and n.lower() != "readme.md"]


def parse_fragment(path):
    text, _ = read(path)
    head, sep, body = text.partition("\n---\n")
    if not sep or not head.startswith("kind:"):
        die(f"{path}: expected a first line `kind: ...`, a line `---`, then the entry")
    return head[len("kind:"):].strip(), body.strip("\n")


def heading_for(t, m, version, kind):
    """The new entry's heading, in the style of the newest existing one."""
    bracket = m.group(1) == "["
    if re.match(r"^## \[?`?\d+\.\d+\.\d+[^\n]*? - \d{4}-\d{2}-\d{2}\b", t[m.start():]):
        return f"## {'[' + version + ']' if bracket else version} - {datetime.date.today().isoformat()}"
    if bracket:
        return f"## [{version}] {EM} {kind}"
    if t[m.start():].startswith("## `"):
        return f"## `{version}` {EM} {kind}"
    return f"## {version} {EM} {kind}"


def pick_kind(kinds):
    """The heading kind: the first fragment whose kind says "breaking", else the first fragment's."""
    return next((k for k in kinds if "breaking" in k.lower()), kinds[0])


def newest_entry(root):
    p = os.path.join(root, "CHANGELOG.md")
    t, crlf = read(p)
    m = re.search(r"(?m)^## (\[?)`?\d+\.\d+\.\d+", t)
    return (p, t, crlf, m) if m else die("CHANGELOG.md has no version heading to put the entry above")


def changelog_change(root, version, frags):
    if not frags:
        die("no changelog.d/*.md fragment: every pushed change needs a CHANGELOG entry")
    parsed = [parse_fragment(f) for f in frags]
    p, t, crlf, m = newest_entry(root)
    heading = heading_for(t, m, version, pick_kind([k for k, _ in parsed]))
    entry = heading + "\n\n" + "\n\n".join(b for _, b in parsed) + "\n\n"
    rule = "---\n\n" if re.search(r"(?m)^---\s*$", t[m.start(): m.start() + 4000]) else ""
    return (p, t[: m.start()] + entry + rule + t[m.start():], crlf)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("version")
    ap.add_argument("--root", default=os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    if not re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?", a.version):
        die(f"`{a.version}` is not MAJOR.MINOR.PATCH")
    root = os.path.abspath(a.root)
    old = current_version(root)
    if old == a.version:
        die(f"the crates already say {old}")
    frags = fragments(os.path.join(root, "changelog.d"))
    changes = version_changes(root, old, a.version)
    changes.append(changelog_change(root, a.version, frags))
    for path, _, _ in changes:
        print(("would change " if a.dry_run else "changed ") + os.path.relpath(path, root).replace("\\", "/"))
    if a.dry_run:
        print(f"{old} -> {a.version}; {len(frags)} fragment(s)")
        return
    for path, text, crlf in changes:
        write(path, text, crlf)
    for f in frags:
        os.remove(f)
    print(f"stamped {old} -> {a.version}; removed {len(frags)} fragment(s)")


if __name__ == "__main__":
    main()
