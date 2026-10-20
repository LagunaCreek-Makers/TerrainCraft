#!/usr/bin/env python3
"""Build a one-to-one NPS NHL engraving placeholder catalog from the official state list.

Requires beautifulsoup4. Fetches only the one index page, or accepts a saved HTML file.
Every extracted NHL gets its own SVG filename, but the artwork is CATEGORY-LEVEL,
NOT a verified depiction. Never geocodes or exports restricted site locations.
"""
import argparse, csv, hashlib, json, re, urllib.request
from pathlib import Path
from bs4 import BeautifulSoup

URL = 'https://www.nps.gov/subjects/nationalhistoriclandmarks/list-of-nhls-by-state.htm'
STATES = {'Alabama':'AL','Alaska':'AK','Arizona':'AZ','Arkansas':'AR','California':'CA','Colorado':'CO','Connecticut':'CT','Delaware':'DE','Florida':'FL','Georgia':'GA','Hawaii':'HI','Idaho':'ID','Illinois':'IL','Indiana':'IN','Iowa':'IA','Kansas':'KS','Kentucky':'KY','Louisiana':'LA','Maine':'ME','Maryland':'MD','Massachusetts':'MA','Michigan':'MI','Minnesota':'MN','Mississippi':'MS','Missouri':'MO','Montana':'MT','Nebraska':'NE','Nevada':'NV','New Hampshire':'NH','New Jersey':'NJ','New Mexico':'NM','New York':'NY','North Carolina':'NC','North Dakota':'ND','Ohio':'OH','Oklahoma':'OK','Oregon':'OR','Pennsylvania':'PA','Rhode Island':'RI','South Carolina':'SC','South Dakota':'SD','Tennessee':'TN','Texas':'TX','Utah':'UT','Vermont':'VT','Virginia':'VA','Washington':'WA','West Virginia':'WV','Wisconsin':'WI','Wyoming':'WY','District of Columbia':'DC','Puerto Rico':'PR','Virgin Islands':'VI','American Samoa':'AS','Guam':'GU','Northern Mariana Islands':'MP'}
DATE = re.compile(r'\b(?:0?[1-9]|1[0-2])/(?:[0-3]?\d)/(?:\d{2}|\d{4})\b')

def classify(name):
 n=name.lower()
 for category, terms in [
  ('ship',('uss ','ship','schooner','steamboat','submarine','battleship','vessel')),
  ('bridge',('bridge','viaduct','aqueduct')),
  ('church',('church','cathedral','chapel','synagogue','temple','mission','mosque')),
  ('industrial',('mill','factory','furnace','mine','dam','railroad','railway','station','canal')),
  ('fort',('fort ','battlefield','arsenal','military','barracks')),
  ('archaeology',('archeolog','archaeolog','ruin','mound','pueblo','site','petroglyph')),
  ('landscape',('park','garden','grove','forest','trail','mountain')),
  ('building',('house','hall','hotel','school','library','capitol','courthouse','building','home','theater','theatre'))]:
  if any(t in n for t in terms):return category
 return 'historic-landmark'

SHAPES = {
 'ship':"<path d='M6 39 H58 L49 54 H17 Z M14 39 V17 H43 V39 M22 17 V7 M22 8 L43 17 H22 Z'/>",
 'bridge':"<path d='M5 49 H59 M9 49 V30 H55 V49 M9 30 Q32 4 55 30 M20 30 V49 M44 30 V49'/>",
 'church':"<path d='M9 56 V25 H55 V56 M24 25 V10 H40 V25 M32 4 V15 M26 9 H38 M26 56 V39 Q32 31 38 39 V56'/>",
 'industrial':"<path d='M8 55 V28 L23 37 V28 L38 37 V20 H52 V55 Z M42 20 V9 H49 V20 M14 47 H20 M28 47 H34'/>",
 'fort':"<path d='M8 55 V22 H16 V15 H24 V22 H40 V15 H48 V22 H56 V55 Z M27 55 V40 H37 V55'/>",
 'archaeology':"<path d='M6 54 H58 M12 54 L20 32 H44 L52 54 M19 32 L25 19 H39 L45 32 M29 19 V10 H35 V19'/>",
 'landscape':"<path d='M4 55 L22 15 L33 39 L43 24 L60 55 Z M16 55 L24 40 L32 55 M44 55 V35 M38 42 L44 31 L50 42'/>",
 'building':"<path d='M9 55 V21 H55 V55 M5 21 L32 8 L59 21 M18 30 H25 V39 H18 Z M39 30 H46 V39 H39 Z M28 55 V42 H36 V55'/>",
 'historic-landmark':"<path d='M9 55 H55 M15 55 V23 H49 V55 M11 23 L32 8 L53 23 M27 55 V36 H37 V55'/>",
}

def svg(category, simple=False):
 shape=SHAPES[category]
 width='4' if simple else '2.5'
 return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64"><g fill="none" stroke="black" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round">{shape}</g></svg>\n'

def parse(html):
 soup=BeautifulSoup(html,'html.parser')
 results=[]; current=None
 # The NPS list uses h4 headings with state counts, followed by paragraphs.
 for tag in soup.find_all(['h3','h4','p','li']):
  if tag.name in ('h3','h4'):
   title=tag.get_text(' ',strip=True)
   match=re.match(r'^(.+?)\s*\((\d+)\)',title)
   current=STATES.get(match.group(1).strip()) if match else None
   continue
  if not current:continue
  raw=tag.get_text(' ',strip=True)
  if not DATE.search(raw):continue
  if 'Withdrawal of Designation' in raw or 'Properties Determined Eligible' in raw:continue
  # Names are before the first designation date; skip nested child paragraphs.
  if tag.find_parent(['p','li']):continue
  name=DATE.split(raw,1)[0].strip(' \u2013-:')
  name=re.sub(r'\s*\((?:restricted|redacted)\)\s*',' ',name,flags=re.I).strip()
  name=re.sub(r'\s*[-–]\s*$','',name).strip()
  if len(name)<3 or len(name)>240:continue
  restricted=bool(re.search(r'\b(restricted|redacted)\b',raw,re.I))
  key=current+'|'+name.casefold()
  digest=hashlib.sha256(key.encode()).hexdigest()[:14]
  link=next((a.get('href','') for a in tag.find_all('a',href=True) if a.get('href','').startswith('http')), '')
  results.append(dict(id='nhl-'+current.lower()+'-'+digest,name=name,jurisdiction='US-'+current,category=classify(name),restricted=str(restricted).lower(),artwork_status='generic-placeholder',source_url=URL,nomination_url=link))
 unique={r['id']:r for r in results}
 return sorted(unique.values(),key=lambda r:(r['jurisdiction'],r['name']))

def export(records,output):
 output.mkdir(parents=True,exist_ok=True)
 icons=output/'svg';icons.mkdir(exist_ok=True)
 fields=['id','name','jurisdiction','category','restricted','artwork_status','source_url','nomination_url','svg_file','simple_svg_file']
 for r in records:
  r['svg_file']='svg/'+r['id']+'.svg'
  r['simple_svg_file']='svg/'+r['id']+'--simplified.svg'
  (output/r['svg_file']).write_text(svg(r['category']),encoding='utf-8')
  (output/r['simple_svg_file']).write_text(svg(r['category'],True),encoding='utf-8')
 with (output/'catalog.csv').open('w',newline='',encoding='utf-8') as f:
  writer=csv.DictWriter(f,fieldnames=fields);writer.writeheader();writer.writerows(records)
 counts={s:sum(r['jurisdiction']=='US-'+s for r in records) for s in STATES.values()}
 (output/'coverage.json').write_text(json.dumps({'source':URL,'total_records':len(records),'total_svg_files':2*len(records),'by_state':counts,'status':'generic placeholders only; no site-specific visual verification','restricted_sites':'No coordinates exported'},indent=2)+'\n')
 return counts

def main():
 p=argparse.ArgumentParser(description=__doc__)
 p.add_argument('--html',type=Path,help='Saved HTML from NPS (for reproducible/offline operation)')
 p.add_argument('--output',type=Path,default=Path('assets/nps_nhl'))
 p.add_argument('--minimum',type=int,default=1000,help='Reject suspiciously incomplete online extraction')
 a=p.parse_args()
 if a.html:html=a.html.read_text(encoding='utf-8')
 else:
  req=urllib.request.Request(URL,headers={'User-Agent':'TerrainCraft-NHL-Catalog/1.0 (manual one-page fetch)'})
  with urllib.request.urlopen(req,timeout=45) as f:html=f.read().decode('utf-8')
 records=parse(html)
 if len(records)<a.minimum:raise SystemExit(f'Incomplete NPS extraction: {len(records)} records (< {a.minimum}); no files written')
 counts=export(records,a.output)
 print(f'Generated {len(records)} catalog records and {2*len(records)} SVG files in {a.output}')
 print('Jurisdictions with records:',sum(v>0 for v in counts.values()))
if __name__=='__main__':main()
