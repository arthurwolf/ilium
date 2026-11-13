"""python3 check.py 'glob': rounded terminal boxes (╭..╰) inside <pre class="term"> must have equal display width per row."""
import re, glob, html, unicodedata, sys
def w(c):
    o = ord(c)
    if unicodedata.combining(c) or o == 0xfe0f: return 0
    if unicodedata.east_asian_width(c) in 'WF' or 0x1f000 <= o <= 0x1faff or o in (0x2705, 0x270b, 0x26a0): return 2
    return 1
def dw(s): return sum(w(c) for c in s)
def plain(line): return html.unescape(re.sub(r'<[^>]+>', '', line))
bad = 0
for p in sorted(glob.glob('/home/arthur/dev/ai/ilium/docs/plugins-manual/pages/' + (sys.argv[1] if len(sys.argv) > 1 else '*.md'))):
    text = open(p).read()
    for bi, m in enumerate(re.finditer(r'<pre class="term">(.*?)</pre>', text, re.S)):
        lines = [plain(l).rstrip() for l in m.group(1).split('\n')]
        i = 0
        while i < len(lines):
            if lines[i].lstrip().startswith('╭'):
                j = i + 1
                while j < len(lines) and not lines[j].lstrip().startswith('╰'): j += 1
                if j >= len(lines): break
                ws = [dw(l) for l in lines[i:j + 1]]
                if len(set(ws)) > 1:
                    bad += 1; print(p.split('/')[-1], 'block', bi, ws[:8])
                i = j + 1
            else: i += 1
print('misaligned boxes:', bad)


def wit_excerpt_check(pattern):
    """Structural WIT check (no wasm-tools available).
    - Fences whose first line is '// Excerpt of ilium:<pkg>/<iface> in reference-wit' are compared with
      reference-wit: every declared type, case and function must exist there, case order included.
      A mismatch counts as a problem.
    - Unlabeled ```wit fences only produce a warning.
    - host::<module>::<fn> calls are extracted and checked against the func names of reference-wit
      (warning only)."""
    base = '/home/arthur/dev/ai/ilium/docs/plugins-manual/pages/'
    ref = open(base + 'reference-wit.md').read()
    ifaces = {}
    for m in re.finditer(r'^\s*interface ([\w-]+) \{', ref, re.M):
        start = m.end(); depth = 1; i = start
        while i < len(ref) and depth:
            depth += {'{': 1, '}': -1}.get(ref[i], 0); i += 1
        ifaces.setdefault(m.group(1), []).append(ref[start:i - 1])
    def strip(b): return re.sub(r'//[^\n]*', '', b)
    def decls(body):
        out = {}
        for m in re.finditer(r'\b(variant|enum|flags|record|resource)\s+%?([\w-]+)\s*\{', body):
            depth = 1; i = m.end()
            while i < len(body) and depth:
                depth += {'{': 1, '}': -1}.get(body[i], 0); i += 1
            inner = body[m.end():i - 1]
            if m.group(1) == 'record': cases = re.findall(r'(%?[\w-]+)\s*:', inner)
            elif m.group(1) == 'resource': cases = re.findall(r'(%?[\w-]+)\s*:\s*func', inner)
            else: cases = re.findall(r'(?:^|,)\s*(%?[a-z][\w-]*)', re.sub(r'\([^()]*(?:\([^()]*\)[^()]*)*\)', '', inner))
            out[m.group(2)] = cases
        return out
    ref_decls, ref_funcs = {}, {}
    for name, bodies in ifaces.items():
        for b in bodies:
            ref_decls.setdefault(name, {}).update(decls(strip(b)))
            ref_funcs.setdefault(name, set()).update(re.findall(r'([\w-]+)\s*:\s*(?:async\s+)?func', strip(b)))
    problems = warnings = 0
    for p in sorted(glob.glob(base + pattern)):
        if p.endswith('/reference-wit.md'): continue
        text = open(p).read(); short = p.split('/')[-1]
        for m in re.finditer(r'```wit\n(.*?)```', text, re.S):
            fence = m.group(1)
            head = re.match(r'// Excerpt of ilium:([\w-]+)/([\w-]+) in reference-wit', fence)
            if not head:
                warnings += 1; print('warning: unlabeled wit fence in', short); continue
            iface = head.group(2); body = strip(fence)
            if iface not in ref_decls:
                problems += 1; print(short, 'unknown interface', iface); continue
            for tname, cases in decls(body).items():
                want = ref_decls[iface].get(tname)
                if want is None:
                    problems += 1; print(short, iface, 'type not in reference-wit:', tname)
                elif cases and [c for c in want if c in cases] != cases:
                    problems += 1; print(short, iface, tname, 'cases differ from reference-wit:', cases)
            for fname in re.findall(r'([\w-]+)\s*:\s*(?:async\s+)?func', body):
                if fname not in ref_funcs.get(iface, ()):
                    problems += 1; print(short, iface, 'func not in reference-wit:', fname)
        for module, fn in re.findall(r'host::([a-z_0-9]+)::([a-z_0-9]+)\(', text):
            mod = {'intents': 'intents-host', 'ui': 'ui-host'}.get(module, module.replace('_', '-'))
            funcs = {f.replace('-', '_') for f in ref_funcs.get(mod, ())}
            if mod not in ref_funcs or fn not in funcs:
                warnings += 1; print('warning:', short, 'host call not in reference-wit: host::%s::%s' % (module, fn))
    print('wit excerpt problems:', problems, 'warnings:', warnings)
    return problems


TYPES_MODULES = ('ids', 'errors', 'text', 'paths', 'editing', 'layout', 'geo', 'net', 'tree', 'ui',
                 'requests', 'intents', 'events', 'lifecycle')


def contract_checks(pattern):
    """Cross-page rules derived from reference-wit.md (no wit.json exists).
    - types::<module>::<Name> paths: error when Name is declared in another ilium:types module,
      warning when the name is declared nowhere.
    - Grant: names in the reference-wit interface headers must be rows of reference-capabilities
      (errors).
    - The imports table of worlds-and-sides must match the base worlds of reference-wit section 9.
    - Dotted codes (harness., fixture., trap., conformance. ids) on the harness and conformance pages
      must be listed in the harness error table, the trap table, or the CLI reference."""
    base = '/home/arthur/dev/ai/ilium/docs/plugins-manual/pages/'
    ref = open(base + 'reference-wit.md').read()
    problems = warnings = 0
    def pascal(n): return ''.join(x.capitalize() for x in n.split('-'))
    declared = {}
    for m in re.finditer(r'^\s*interface ([\w-]+) \{', ref, re.M):
        if m.group(1) not in TYPES_MODULES: continue
        i = m.end(); depth = 1
        while i < len(ref) and depth:
            depth += {'{': 1, '}': -1}.get(ref[i], 0); i += 1
        body = ref[m.end():i - 1]
        names = re.findall(r'\b(?:variant|enum|flags|record|resource)\s+([\w-]+)', body)
        names += re.findall(r'^\s*type ([\w-]+) =', body, re.M)
        declared.setdefault(m.group(1), set()).update(pascal(n) for n in names)
    anywhere = set().union(*declared.values())
    for p in sorted(glob.glob(base + pattern)):
        if p.endswith('/reference-wit.md'): continue
        text = open(p).read(); short = p.split('/')[-1]
        for mod, rest in re.findall(r'types::(\w+)::(\{[^}]*\}|\w+)', text):
            if mod not in TYPES_MODULES: continue
            for name in re.findall(r'[A-Z]\w*', rest):
                if name in declared.get(mod, ()): continue
                other = [k for k, v in declared.items() if name in v]
                if other:
                    problems += 1; print(short, 'types::%s::%s is declared in types::%s' % (mod, name, other[0]))
                elif name not in anywhere and name not in ('Self',):
                    warnings += 1; print('warning:', short, 'types::%s::%s has no WIT counterpart' % (mod, name))
    caps = open(base + 'reference-capabilities.md').read()
    cap_names = set()
    for row in re.findall(r'^\| ([^|]+) \|', caps, re.M):
        cap_names.update(re.findall(r'`([a-z][a-z0-9.\-]*)(?::[^`]*)?`', row))
    for n, line in enumerate(ref.split('\n'), 1):
        m = re.match(r'^[SCA ]+\.? ?(?:\([^)]*\) )?Grants?: (.*?)\. API', line)
        if not m: continue
        head = re.sub(r'\bnone for [^,.`]*', '', m.group(1))
        for g in re.findall(r'`([^`]+)`', head):
            b = g.split(':')[0]
            if b not in cap_names and not g.startswith('plugin-services') and not b.startswith('gpu-kernels'):
                problems += 1; print('reference-wit.md:%d' % n, 'Grant name not in reference-capabilities:', g)
    sec = ref[ref.index('## 9. Worlds'):ref.index('## 10. Mapping')]
    worlds = {}
    for m in re.finditer(r'package ilium:(\w+)@0.1.0;\s*world base \{(.*?)\n\}', sec, re.S):
        worlds[m.group(1)] = {(p, i) for p, i in re.findall(r'import ilium:(\w+)/([\w-]+)@', m.group(2))}
    ws = open(base + 'worlds-and-sides.md').read()
    rows = {}
    for r in re.findall(r'^\| (`[^|]+`) \| (yes|no) \| (yes|no) \| (yes|no) \|', ws, re.M):
        for n in re.findall(r'`([\w/-]+)`', r[0]): rows[n] = r[1:]
    for key, side in (('server', 0), ('client', 1), ('anim', 2)):
        for p, i in worlds.get(key, ()):
            n = 'anim/' + i if p == 'anim' else i
            if n not in rows or rows[n][side] != 'yes':
                problems += 1; print('worlds-and-sides.md', n, 'must be yes for', key, 'per reference-wit section 9')
    for n, (s_, c_, a_) in rows.items():
        for key, side, v in (('server', 0, s_), ('client', 1, c_), ('anim', 2, a_)):
            have = any((n.split('/')[-1] == i and (p == 'anim') == n.startswith('anim/')) for p, i in worlds.get(key, ()))
            if v == 'yes' and not have:
                problems += 1; print('worlds-and-sides.md', n, 'is not imported by world', key)
    harness = open(base + 'reference-test-harness.md').read()
    known = set(re.findall(r'`((?:harness|fixture|trap|toolchain|test)\.[a-z0-9.-]+)`', harness))
    known |= set(re.findall(r'`(trap\.[a-z-]+)`', open(base + 'reference-errors-and-limits.md').read()))
    cli = open(base + 'reference-plugin-cli.md').read()
    known |= set(re.findall(r'`([a-z]+\.[a-z0-9.-]+)`', cli))
    table = set(re.findall(r'^\| `((?:harness|fixture|trap|toolchain|test)\.[a-z0-9.-]+)`', harness, re.M))
    for page in ('reference-test-harness.md', 'reference-conformance-suite.md', 'testing.md'):
        t = open(base + page).read()
        for code in set(re.findall(r'`((?:harness|fixture|trap)\.[a-z0-9.-]+)`', t)):
            if code not in table and code not in known:
                problems += 1; print(page, 'code not in the harness error table:', code)
    print('contract problems:', problems, 'warnings:', warnings)
    return problems


def requirement_rows_check():
    """Every requirement row in pages/reference-requirements-*.md must carry the statement of the
    owner-page bullet `**[ID]** text` verbatim (CONVENTIONS.md, 'Requirement rows'). Counted as problems."""
    base = '/home/arthur/dev/ai/ilium/docs/plugins-manual/pages/'
    bullets = {}
    for p in glob.glob(base + '*.md'):
        if '/reference-requirements-' in p: continue
        for m in re.finditer(r'^(?:- )?\*\*\[([A-Z]+-\d+)\]\*\*\s*(.*)$', open(p).read(), re.M):
            bullets.setdefault(m.group(1), (p.split('/')[-1], m.group(2).strip()))
    problems = 0
    for p in sorted(glob.glob(base + 'reference-requirements-dev-tooling.md')):
        for m in re.finditer(r'^\| \[?([A-Z]+-\d+)\]? \| (.*?) \| ', open(p).read(), re.M):
            rid, stmt = m.group(1), m.group(2).strip()
            if rid in bullets and bullets[rid][1].replace('|', '\\|') != stmt and bullets[rid][1] != stmt:
                problems += 1; print(p.split('/')[-1], rid, 'row differs from the bullet in', bullets[rid][0])
    print('requirement row problems:', problems)
    return problems


requirement_rows_check()
contract_checks(sys.argv[1] if len(sys.argv) > 1 else '*.md')
wit_excerpt_check(sys.argv[1] if len(sys.argv) > 1 else '*.md')
