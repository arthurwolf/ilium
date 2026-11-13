"""python3 linkcheck.py 'glob': missing /pages/<slug>/ links and contract-0.2 strings."""
import re, glob, os, sys
base = '/home/arthur/dev/ai/ilium/docs/plugins-manual/pages/'
slugs = set()
for f in glob.glob(base + '*.md'):
    m = re.search(r'^permalink: /pages/([^/]+)/', open(f).read(), re.M)
    slugs.add(m.group(1) if m else os.path.basename(f)[:-3])
bad = 0
for f in sorted(glob.glob(base + (sys.argv[1] if len(sys.argv) > 1 else '*.md'))):
    t = open(f).read()
    for m in re.finditer(r'\]\(/pages/([a-z0-9-]+)/?(?:#[^)]*)?\)|href="/pages/([a-z0-9-]+)/', t):
        s = m.group(1) or m.group(2)
        if s not in slugs:
            bad += 1; print(os.path.basename(f), 'missing page:', s)
    for m in re.finditer(r'(?<![\d.])0\.2(?![\d])', t):
        ctx = t[max(0, m.start()-12):m.end()+8].replace('\n', ' ')
        if re.search(r'contract|WASI|@0\.2|version|api', ctx, re.I):
            bad += 1; print(os.path.basename(f), 'suspicious 0.2:', ctx)
print('problems:', bad)
