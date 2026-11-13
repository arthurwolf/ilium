"""Pad or trim spaces before the right border so every row of a rounded box matches the top border width."""
import re, glob, html, sys
sys.path.insert(0, '/home/arthur/dev/ai/ilium/docs/plugins-manual/_tools')
import unicodedata
def w(c):
    o = ord(c)
    if unicodedata.combining(c) or o == 0xfe0f: return 0
    if unicodedata.east_asian_width(c) in 'WF' or 0x1f000 <= o <= 0x1faff or o in (0x2705, 0x270b, 0x26a0): return 2
    return 1
dw = lambda s: sum(map(w, s))
plain = lambda l: html.unescape(re.sub(r'<[^>]+>', '', l))
fixed = 0
for p in glob.glob('/home/arthur/dev/ai/ilium/docs/plugins-manual/pages/*.md'):
    t = open(p).read(); o = t
    def blk(m):
        global fixed
        lines = m.group(1).split('\n'); i = 0
        while i < len(lines):
            if plain(lines[i]).lstrip().startswith('╭'):
                j = i + 1
                while j < len(lines) and not plain(lines[j]).lstrip().startswith('╰'): j += 1
                if j >= len(lines): break
                tw = dw(plain(lines[i]).rstrip())
                for k in range(i + 1, j):
                    pl = plain(lines[k]).rstrip(); d = tw - dw(pl)
                    if d == 0 or not pl.endswith('│'): continue
                    # last │ in raw line
                    idx = lines[k].rfind('│')
                    if d > 0: lines[k] = lines[k][:idx] + ' ' * d + lines[k][idx:]
                    else:
                        before = lines[k][:idx]; n = len(before) - len(before.rstrip(' '))
                        if n >= -d: lines[k] = before[:len(before) + d] + lines[k][idx:]
                        else: continue
                    fixed += 1
                i = j + 1
            else: i += 1
        return '<pre class="term">' + '\n'.join(lines) + '</pre>'
    t = re.sub(r'<pre class="term">(.*?)</pre>', blk, t, flags=re.S)
    if t != o: open(p, 'w').write(t)
print('rows fixed', fixed)
