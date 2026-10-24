"""Ownership check for docs/design/linux-cef/design.md section 8 (and the trees of 1.2, 1.3 and UA 5.1).

Checks:
  1. every unit named in a Depends cell exists;
  2. two units whose file lists overlap are ordered (one depends on the other, directly, through the
     chain, or through the wave barrier); a file may pass only along that order;
  3. every owned entry of the design's 1.2 and 1.3 trees is covered by its owner unit's file list;
  4. every UA 5.1 path is covered by some unit, unless the design's 1.3 change table removes it;
  5. every file under crates/vsesvit-gpui/src/self_test/checks/ has exactly one owner (W0.1 aside), the one its header names.
Usage: python -I check_ownership.py <repo root>
"""
import itertools
import re
import sys
from pathlib import Path

ROOTS = ("crates/", "xtask/", ".github/", "docs/", "packaging/", "third_party/")
ROOT_FILES = {"Cargo.toml", ".gitattributes", "clippy.toml"}
GPUI = "crates/vsesvit-gpui/"
VERBOSE = "--verbose" in sys.argv
UNIT = r"(?:WX\.\d|W\d\.\d|C\.\d)"


def expand(p):
    m = re.search(r"\{([^{}]*)\}", p)
    if not m:
        return [p]
    out = []
    for alt in m.group(1).split(","):
        out += expand(p[: m.start()] + alt.strip() + p[m.end():])
    return out


def absolute(tok, mode=None):
    if mode == "core":
        return "crates/vsesvit-core/src/" + tok.replace("::", "/")
    if mode == "winui":
        return "crates/vsesvit-winui/src/" + tok
    if tok.startswith(ROOTS) or tok in ROOT_FILES:
        return tok
    if tok.startswith("core::"):
        segs = [s for s in tok[6:].split("::") if not s[:1].isupper()]
        return "crates/vsesvit-core/src/" + "/".join(segs)
    for crate in ("vsesvit_sync", "vsesvit_update"):
        if tok.startswith(crate + "::"):
            return "crates/" + crate.replace("_", "-") + "/src/" + tok.split("::", 1)[1].replace("::", "/")
    if tok.startswith("examples/") or tok.startswith("assets/"):
        return GPUI + tok
    return GPUI + "src/" + tok


def pattern(p):
    """(kind, text): 'file' exact, 'prefix' directory or module prefix."""
    if "*" in p:
        return ("prefix", p.split("*")[0].rstrip("/") + "/")
    if p.endswith("/"):
        return ("prefix", p)
    if re.search(r"\.[A-Za-z0-9]+$", p.rsplit("/", 1)[-1]):
        return ("file", p)
    return ("prefix", p)  # a module path such as crates/vsesvit-core/src/cli


def covers(pat, path):
    kind, text = pat
    if kind == "file":
        return path == text
    return path == text.rstrip("/") or path.startswith(text.rstrip("/") + "/") or path.startswith(text) or (
        path.startswith(text.rstrip("/") + ".")
    )


def overlap(a, b):
    """Return a representative path in both patterns, or None."""
    (ka, ta), (kb, tb) = a, b
    if ka == "file" and kb == "file":
        return ta if ta == tb else None
    if ka == "file":
        return ta if covers(b, ta) else None
    if kb == "file":
        return tb if covers(a, tb) else None
    if ta.startswith(tb.rstrip("/")) and covers(b, ta.rstrip("/") + "/x"):
        return ta
    if tb.startswith(ta.rstrip("/")) and covers(a, tb.rstrip("/") + "/x"):
        return tb
    return None


def split_cells(line):
    cells = re.split(r"(?<!\\)\|", line.strip())
    return [c.strip() for c in cells[1:-1]]


def is_pathish(tok):
    if " " in tok or tok.startswith("-") or tok.startswith("[") or "::" in tok and not tok.startswith(("core::", "vsesvit_")):
        return False
    return "/" in tok or tok in ROOT_FILES or re.search(r"\.[a-z0-9]+$", tok) is not None or tok.startswith(("core::", "vsesvit_"))


def parse_files(cell, tree12):
    includes, excludes = [], []
    if "files marked W1.3 in §1.2" in cell:
        includes += [p for p, o in tree12 if o == "W1.3"]
        cell = cell.replace("the `crates/vsesvit-cef` files marked W1.3 in §1.2", "")
    mode = None
    pos = 0
    last_inc = None
    for m in re.finditer(r"(core:\s*|WinUI:\s*|except\s+)?`([^`]+)`", cell):
        lead, tok = m.group(1), m.group(2)
        lead = (lead or "").strip() or None
        if lead == "core:":
            mode = "core"
        elif lead == "WinUI:":
            mode = "winui"
        if lead == "except":
            lead = "except "
        tok = tok.replace(", ", ",")
        bare = mode in ("core", "winui") and re.fullmatch(r"[a-z_:/.]+", tok)
        if not (is_pathish(tok) or bare):
            continue
        if lead and lead.startswith("except") and last_inc is not None:
            for t in expand(tok):
                if t.startswith(ROOTS):
                    excludes.append(pattern(t))
                    continue
                base = last_inc[1]
                first = t.split("/")[0]
                k = base.find("/" + first + "/")
                if k >= 0:
                    excludes.append(pattern(base[: k + 1] + t))
                else:
                    excludes.append(pattern(base.split("*")[0].rstrip("/") + "/" + t))
            continue
        for t in expand(tok):
            pat = pattern(absolute(t, mode if bare else None))
            includes.append(pat)
            last_inc = pat
    return includes, excludes


def strip_parens(text):
    """text without balanced parenthesised groups (a child's description in a directory entry)."""
    out, depth = [], 0
    for ch in text:
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth = max(depth - 1, 0)
        elif depth == 0:
            out.append(ch)
    return "".join(out)


MAX_ENTRY_INDENT = 8  # the trees nest at most three levels of two spaces; deeper lines wrap a description


def parse_tree(block, root):
    """Entries (path, owner) of an indented tree with owners at line ends.

    An entry is a line plus its wrapped continuation lines: the lines indented to or past the column where the
    entry's description starts (or past MAX_ENTRY_INDENT, for an entry whose name overruns that column). The owner is the unit at the end of the last of them. A directory entry whose
    description starts with a file list (`x11/   mod.rs (...), surface.rs (...)`) owns the directory and each
    listed file. A line that is neither an entry nor a continuation is an error, never silently dropped."""
    groups = []  # [indent, desc_col, [lines]]
    for raw in block:
        line = raw.rstrip()
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip())
        if groups and groups[-1][1] is not None and indent >= min(groups[-1][1], MAX_ENTRY_INDENT):
            groups[-1][2].append(line)
            continue
        parts = line.strip().split(None, 1)
        desc_col = line.index(parts[1], indent + len(parts[0])) if len(parts) > 1 else None
        groups.append([indent, desc_col, [line]])

    entries, stack = [], [(-1, root)]
    for indent, _, lines in groups:
        first = lines[0].strip()
        tok = first.split()[0]
        if not ("." in tok or tok.endswith("/") or "{" in tok):
            raise ValueError(f"tree line is neither an entry nor a continuation: {lines[0].strip()!r}")
        while stack and stack[-1][0] >= indent:
            stack.pop()
        base = stack[-1][1] if stack else root
        if tok.startswith("crates/"):
            base = ""
        owner = re.search(r"\s(" + UNIT + r")\s*$", lines[-1])
        if tok.endswith("/"):
            stack.append((indent, base + tok))
        if not owner:
            if tok.endswith("/") and len(lines) == 1 and len(first.split()) == 1:
                continue  # a bare directory line: only the nesting level
            raise ValueError(f"tree entry without an owner: {first!r}")
        paths = [base + t for t in expand(tok)]
        desc = " ".join(l.strip() for l in lines)[len(tok):]
        desc = re.sub(r"\s" + UNIT + r"\s*$", "", strip_parens(desc)).strip()
        if tok.endswith("/") and re.match(r"[a-z_0-9]+\.rs", desc):
            paths += [base + tok + n for n in re.findall(r"([a-z_0-9]+\.rs)", desc)]
        entries += [(p, owner.group(1)) for p in paths]
    return entries


def code_block_after(text, heading, nth=1):
    i = text.index(heading)
    blocks = re.findall(r"```\n(.*?)```", text[i:], re.S)
    return blocks[nth - 1].splitlines()


def main(repo):
    design = (repo / "docs/design/linux-cef/design.md").read_text(encoding="utf-8")
    ua = (repo / "docs/design/linux-cef/ui-architecture.md").read_text(encoding="utf-8")
    problems = []

    tree12 = parse_tree(code_block_after(design, "### 1.2 `vsesvit-cef` module tree"), "crates/vsesvit-cef/")
    tree13 = parse_tree(code_block_after(design, "### 1.3 `vsesvit-gpui` module tree"), "")
    tree12 = [(p, o) for p, o in tree12]
    tree12_pats = [(pattern(p), o) for p, o in tree12]

    sec8 = design[design.index("## 8. Implementation waves"): design.index("## 9. Open questions")]
    units, header = {}, None
    for line in sec8.splitlines():
        if not line.startswith("|"):
            header = None
            continue
        cells = split_cells(line)
        if cells and cells[0] == "Unit":
            header = cells
            continue
        if header is None or not cells or set(cells[0]) <= {"-"}:
            continue
        m = re.match(UNIT, cells[0])
        if not m:
            continue
        uid = m.group(0)
        col = dict(zip(header, cells))
        files = col.get("Files") or col.get("Core files") or ""
        inc, exc = parse_files(files, tree12_pats)
        if uid.startswith("C."):
            inc.append(pattern("crates/vsesvit-winui/**"))
        deps = col.get("Depends", "")
        units[uid] = {"inc": inc, "exc": exc, "deps": deps}

    order = list(units)
    edges = {u: set() for u in units}
    for u, d in units.items():
        text = d["deps"]
        for a, b in re.findall(r"(W\d\.\d|C\.\d)\s*[–-]\s*(W\d\.\d|C\.\d)", text):
            pa, pb = a[:-1], b[:-1]
            for x in range(int(a[-1]), int(b[-1]) + 1):
                edges[u].add(pa + str(x))
        for t in re.findall(UNIT, text):
            edges[u].add(t)
        for w in re.findall(r"\bW(\d)\b(?!\.\d)", text):
            edges[u] |= {v for v in units if re.fullmatch(rf"W{w}\.\d", v)}
    for u in units:
        if u != "W0.1":
            edges[u].add("W0.1")
    main_waves = {}
    for u in units:
        m = re.fullmatch(r"W(\d)\.\d", u)
        if m and m.group(1) != "6":
            main_waves.setdefault(int(m.group(1)), []).append(u)
    for k, us in main_waves.items():
        prev = [v for j in main_waves if j < k for v in main_waves[j]]
        if k == 7:
            prev += [v for v in units if v.startswith("W6.")]
        for u in us:
            edges[u] |= set(prev)
    cs = sorted(u for u in units if u.startswith("C."))
    for a, b in zip(cs, cs[1:]):
        edges[b].add(a)
    if cs:
        edges[cs[0]].add("W0.3")

    for u, ds in edges.items():
        for d in ds:
            if d not in units:
                problems.append(f"{u}: depends on unknown unit {d}")

    reach = {u: set() for u in units}
    for u in units:
        todo = list(edges[u])
        while todo:
            v = todo.pop()
            if v in reach[u] or v not in units:
                continue
            reach[u].add(v)
            todo += edges[v]

    def excluded(unit, path):
        return any(covers(e, path) for e in units[unit]["exc"])

    shared = 0
    for u, v in itertools.combinations(order, 2):
        if "W0.1" in (u, v):
            continue
        for a in units[u]["inc"]:
            for b in units[v]["inc"]:
                p = overlap(a, b)
                if p is None or excluded(u, p) or excluded(v, p):
                    continue
                shared += 1
                if VERBOSE:
                    print("shared:", u, v, p)
                if v not in reach[u] and u not in reach[v]:
                    problems.append(f"unordered units share a file: {u} and {v} on {p}")

    for p, o in tree12 + tree13:
        if o not in units:
            problems.append(f"tree entry {p}: owner {o} is not a unit")
            continue
        if not any(covers(i, p) for i in units[o]["inc"]) or excluded(o, p):
            problems.append(f"tree entry {p}: not in {o}'s file list")

    removed = []
    for line in design[design.index("| UA path | Change |"):].splitlines()[2:]:
        if not line.startswith("|"):
            break
        cells = split_cells(line)
        if re.search(r"\b(replaced|gone)\b", cells[1]):
            for tok in re.findall(r"`([^`]+)`", cells[0]):
                removed += [absolute(t) for t in expand(tok.replace(", ", ","))]
    ua_block = code_block_after(ua, "### 5.1 Module tree and owners")
    joined, buf = [], ""
    for line in ua_block:
        buf = buf + " " + line.strip() if buf else line
        if buf.count("{") == buf.count("}") and not buf.rstrip().endswith(","):
            joined.append(buf)
            buf = ""
    ua_paths, cwd = [], ""
    for line in joined:
        indent = len(line) - len(line.lstrip())
        body = re.sub(r"\s+U\d\d\b.*$", "", line).strip()
        if body == "crates/vsesvit-gpui/":
            cwd = GPUI
            continue
        if body == "src/":
            cwd = GPUI + "src/"
            continue
        if indent == 0:
            base = ""
        elif indent == 2:
            base = GPUI
        else:
            base = cwd
        for piece in re.split(r",\s*(?![^{]*\})", body):
            piece = re.sub(r"\s*\(.*\)$", "", piece.strip()).replace(", ", ",")
            if not piece:
                continue
            if not is_pathish(piece):
                problems.append(f"UA 5.1: cannot read tree text as a path: {piece!r}")
                continue
            ua_paths += [base + t for t in expand(piece)]
    uncovered = []
    for p in ua_paths:
        if any(covers(pattern(r), p) or p == r for r in removed):
            continue
        glob = "*" in p
        if glob:
            p = p.split("*")[0].rstrip("/")
        owned = any(covers(i, p) or covers(i, p + "/x") for u in units for i in units[u]["inc"] if not excluded(u, p))
        if glob and not owned:
            # a per-unit directory such as self_test/checks/*.rs: covered when units own files inside it (§8.1)
            owned = any(i[1].startswith(p + "/") for u in units for i in units[u]["inc"])
        if not owned:
            uncovered.append(p)
    for p in uncovered:
        problems.append(f"UA 5.1 path without an owner: {p}")

    checks_dir = "crates/vsesvit-gpui/src/self_test/checks/"
    files = sorted(f.name for f in (repo / checks_dir).glob("*") if f.is_file())
    if not files:
        problems.append(f"no files under {checks_dir}")
    for f in files:
        path = checks_dir + f
        owners = [u for u in units if u != "W0.1" and any(covers(i, path) for i in units[u]["inc"]) and not excluded(u, path)]
        if len(owners) != 1:
            problems.append(f"{path}: owned by {len(owners)} units {owners}, expected exactly one")
            continue
        head = (repo / path).read_text(encoding="utf-8").splitlines()[:3]
        stated = re.search(r"Owned by (" + UNIT + r")", " ".join(head))
        if stated and stated.group(1) != owners[0]:
            problems.append(f"{path}: its header says {stated.group(1)} but the table gives it to {owners[0]}")

    print(f"units: {len(units)}; dependency edges: {sum(len(e) for e in edges.values())}; "
          f"tree entries: {len(tree12) + len(tree13)}; UA 5.1 paths: {len(ua_paths)} ({len(removed)} removed by 1.3); "
          f"overlapping file pairs checked: {shared}; self_test/checks files: {len(files)}")
    for p in problems:
        print("PROBLEM:", p)
    print("OK" if not problems else f"{len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(Path(sys.argv[1])))
