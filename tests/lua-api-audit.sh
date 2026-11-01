#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

exec python3 - "$ROOT" "$@" <<'PY'
import re
import sys
from pathlib import Path

ROOT = Path(sys.argv[1])
WRITE = sys.argv[2:] == ["--write"]
if sys.argv[2:] not in ([], ["--write"]):
    raise SystemExit("usage: tests/lua-api-audit.sh [--write]")
LUA = ROOT / "player" / "lua"
DOC = ROOT / "docs" / "lua-api.md"
FILES = ["defaults.lua", "assdraw.lua", "options.lua", "input.lua", "osc.lua"]
PROVIDERS = {"host", "defaults.lua", "assdraw.lua", "options.lua", "input.lua", "data"}
SCOPES = {"required", "built-in", "none", "data"}
CLASS_BEGIN = "<!-- lua-api:classify-begin -->"
CLASS_END = "<!-- lua-api:classify-end -->"
CALLS_BEGIN = "<!-- lua-api:calls-begin -->"
CALLS_END = "<!-- lua-api:calls-end -->"

cases = []


def check(name, ok, detail=""):
    cases.append(ok)
    line = f"{'PASS' if ok else 'FAIL'} {name}"
    if detail and not ok:
        line += f": {detail}"
    print(line, flush=True)


def scan(src, strip_comments, keep_strings):
    out = []
    i = 0
    n = len(src)
    while i < n:
        c = src[i]
        if c == "-" and i + 1 < n and src[i + 1] == "-":
            j = i + 2
            m = re.match(r"\[(=*)\[", src[j:])
            if m:
                close = "]" + m.group(1) + "]"
                k = src.find(close, j + len(m.group(0)))
                end = n if k < 0 else k + len(close)
            else:
                k = src.find("\n", j)
                end = n if k < 0 else k
            out.append(
                "".join(ch if ch == "\n" else " " for ch in src[i:end])
                if strip_comments
                else src[i:end]
            )
            i = end
            continue
        if c in "'\"":
            q = c
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == q:
                    j += 1
                    break
                if src[j] == "\n":
                    break
                j += 1
            out.append(
                src[i:j]
                if keep_strings
                else "".join(ch if ch == "\n" else " " for ch in src[i:j])
            )
            i = j
            continue
        m = re.match(r"\[(=*)\[", src[i:])
        if m:
            close = "]" + m.group(1) + "]"
            k = src.find(close, i + len(m.group(0)))
            j = n if k < 0 else k + len(close)
            out.append(
                src[i:j]
                if keep_strings
                else "".join(ch if ch == "\n" else " " for ch in src[i:j])
            )
            i = j
            continue
        out.append(c)
        i += 1
    return "".join(out)


def aliases(src):
    code = scan(src, strip_comments=True, keep_strings=True)
    found = {}
    for m in re.finditer(
        r"local\s+(\w+)\s*=\s*require\s*\(?\s*(['\"])mp\.([\w.]+)\2\s*\)?", code
    ):
        found[m.group(1)] = "mp." + m.group(3)
    for m in re.finditer(
        r"local\s+(\w+)\s*=\s*package\.loaded\[\s*(['\"])mp\.([\w.]+)\2\s*\]", code
    ):
        found[m.group(1)] = "mp." + m.group(3)
    return found


def collect():
    symbols = {}
    for name in FILES:
        path = LUA / name
        if not path.exists():
            raise FileNotFoundError(path)
        src = path.read_text()
        code = scan(src, strip_comments=True, keep_strings=False)
        for alias, prefix in aliases(src).items():
            for m in re.finditer(
                r"(?<![\w.])" + re.escape(alias)
                + r"\.([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*)",
                code,
            ):
                symbol = prefix + "." + m.group(1)
                line = code.count("\n", 0, m.start()) + 1
                symbols.setdefault(symbol, []).append((name, line))
        for m in re.finditer(
            r"(?<![\w.])mp\.([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*)", code
        ):
            symbol = "mp." + m.group(1)
            line = code.count("\n", 0, m.start()) + 1
            symbols.setdefault(symbol, []).append((name, line))
    for symbol, sites in symbols.items():
        seen = set()
        unique = []
        for site in sites:
            if site not in seen:
                seen.add(site)
                unique.append(site)
        unique.sort(key=lambda site: (FILES.index(site[0]), site[1]))
        symbols[symbol] = unique
    return symbols


def parse_classification(text):
    if CLASS_BEGIN not in text or CLASS_END not in text:
        return None, "classification markers are missing"
    section = text.split(CLASS_BEGIN, 1)[1].split(CLASS_END, 1)[0]
    rows = {}
    problems = []
    for raw in section.splitlines():
        line = raw.strip()
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if len(cells) != 4:
            problems.append(f"malformed row: {line}")
            continue
        symbol, provider, scope, semantics = cells
        symbol = symbol.strip("`")
        if symbol in ("", "Symbol") or set(symbol) <= set("-: "):
            continue
        if symbol in rows:
            problems.append(f"duplicate row: {symbol}")
            continue
        if provider not in PROVIDERS:
            problems.append(f"{symbol}: unknown provider {provider}")
        if scope not in SCOPES:
            problems.append(f"{symbol}: unknown scope {scope}")
        if not semantics:
            problems.append(f"{symbol}: empty semantics")
        rows[symbol] = (provider, scope, semantics)
    if not rows:
        problems.append("no classification rows")
    return rows, "; ".join(problems)


def render_calls(symbols):
    rows = [
        "| Symbol | Count | Sites |",
        "|---|---|---|",
    ]
    for symbol in sorted(symbols):
        sites = symbols[symbol]
        rendered = ", ".join(f"`{name}:{line}`" for name, line in sites)
        rows.append(f"| `{symbol}` | {len(sites)} | {rendered} |")
    return "\n".join(rows)


def main():
    try:
        symbols = collect()
        sources_ok = True
    except FileNotFoundError as exc:
        check("lua-api: sources present", False, str(exc))
        symbols = {}
        sources_ok = False
    else:
        check("lua-api: sources present", True)

    if not DOC.exists():
        check("lua-api: classification table", False, f"missing {DOC}")
        check("lua-api: call table", False, f"missing {DOC}")
        return finish()

    text = DOC.read_text()
    rows, problems = parse_classification(text)
    check("lua-api: classification table", rows is not None and not problems,
          problems or "classification table is invalid")

    if rows is not None:
        missing = sorted(set(symbols) - set(rows))
        detail = ", ".join(
            f"{symbol} ({symbols[symbol][0][0]}:{symbols[symbol][0][1]})"
            for symbol in missing[:10]
        )
        if len(missing) > 10:
            detail += f", and {len(missing) - 10} more"
        check("lua-api: every reference is classified", not missing, detail)

        stale = []
        for symbol in rows:
            if symbol in symbols:
                continue
            if any(name.startswith(symbol + ".") for name in symbols):
                continue
            stale.append(symbol)
        check("lua-api: no stale classifications", not stale,
              ", ".join(sorted(stale)[:10]))
    else:
        check("lua-api: every reference is classified", False, "no classification data")
        check("lua-api: no stale classifications", False, "no classification data")

    if sources_ok:
        pattern = re.compile(
            re.escape(CALLS_BEGIN) + r".*?" + re.escape(CALLS_END), re.S
        )
        replacement = CALLS_BEGIN + "\n" + render_calls(symbols) + "\n" + CALLS_END
        if not pattern.search(text):
            check("lua-api: call table", False, "call table markers are missing")
        else:
            updated = pattern.sub(replacement, text, count=1)
            if WRITE and updated != text:
                DOC.write_text(updated)
                check("lua-api: call table", True)
            else:
                check("lua-api: call table", updated == text,
                      "docs/lua-api.md call table is stale; run "
                      "tests/lua-api-audit.sh --write")
    else:
        check("lua-api: call table", False, "sources are unavailable")

    return finish()


def finish():
    passed = sum(1 for ok in cases if ok)
    failed = len(cases) - passed
    print(f"TOTAL {passed} passed, {failed} failed", flush=True)
    return 0 if failed == 0 else 1


sys.exit(main())
PY
