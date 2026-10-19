import re,glob,os
R='/home/arthur/dev/ai/ilium/docs/plugins-manual/'
nav=open(R+'_data/nav.yml').read()
have=set(re.findall(r'url: /pages/([^/]+)/',nav))
add=''
for f in sorted(glob.glob(R+'pages/*.md')):
    s=open(f).read()
    m=re.search(r'^permalink: /pages/([^/]+)/',s,re.M); slug=m.group(1) if m else os.path.basename(f)[:-3]
    if slug in have: continue
    t=re.search(r'^title: "?(.*?)"?\s*$',s,re.M).group(1)
    add+=f'- title: "{t}"\n  url: /pages/{slug}/\n'; print('add',slug)
open(R+'_data/nav.yml','w').write(nav.rstrip('\n')+'\n'+add)
