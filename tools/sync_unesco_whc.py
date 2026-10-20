#!/usr/bin/env python3
"""Sync UNESCO World Heritage properties from official World Heritage Centre XML.

Each property gets two original CATEGORY-level engraving placeholders. This does
NOT claim that the pictograms are visually accurate depictions of each property.
No coordinates are published by this tool (serial/transnational properties can
span multiple countries and contain sensitive locations).
"""
import argparse
import collections
import csv
import hashlib
import json
from pathlib import Path
import re
import urllib.request
import xml.etree.ElementTree as ET

SOURCE = 'https://whc.unesco.org/en/list/xml/'
FIELDS = ['id','unesco_id','name','countries','category','year','source_url','artwork_status','svg_file','simple_svg_file']

SHAPES = {
 'cultural': '<path d="M7 55H57 M13 55V24H51V55 M9 24L32 8L55 24 M21 32V43 M32 32V43 M43 32V43 M28 55V46H36V55"/>',
 'natural': '<path d="M3 55L22 14L34 37L44 22L61 55Z M16 55L24 38L32 55 M46 55V34 M39 43L46 29L53 43"/>',
 'mixed': '<path d="M3 55L19 25L31 43L42 18L61 55 M10 55V43H27V55 M15 43V33H23V43 M35 55V46H51V55"/>',
}

def tag_name(tag):
 return tag.rsplit('}', 1)[-1].lower()

def child_text(el, *names):
 for child in el:
  if tag_name(child.tag) in names:
   return ' '.join(''.join(child.itertext()).split())
 return ''

def parse(xml_bytes):
 root = ET.fromstring(xml_bytes)
 records = []
 seen = set()
 for el in root.iter():
  if tag_name(el.tag) not in ('row', 'property', 'site'):
   continue
  unesco_id = child_text(el, 'id_number', 'id', 'unesco_id')
  name = child_text(el, 'site', 'name_en', 'name')
  if not unesco_id.isdigit() or not name:
   continue
  if unesco_id in seen:
   raise ValueError(f'Duplicate UNESCO property ID: {unesco_id}')
  seen.add(unesco_id)
  raw_category = child_text(el, 'category', 'category_short').lower()
  category = {'c':'cultural', 'n':'natural', 'm':'mixed'}.get(raw_category, raw_category)
  if category not in SHAPES:
   raise ValueError(f'Unknown UNESCO category {raw_category!r} for {unesco_id}')
  countries = child_text(el, 'states', 'states_name_en', 'state', 'country')
  year = child_text(el, 'date_inscribed', 'year')
  key = 'unesco-' + unesco_id
  records.append(dict(id=key, unesco_id=unesco_id, name=name,
    countries=countries, category=category, year=year,
    source_url=f'https://whc.unesco.org/en/list/{unesco_id}/',
    artwork_status='generic-category-placeholder',
    svg_file=f'svg/{key}.svg',simple_svg_file=f'svg/{key}--simplified.svg'))
 return sorted(records,key=lambda r:int(r['unesco_id']))

def svg(category, simplified=False):
 width = 4 if simplified else 2.4
 return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" '
         'width="64" height="64"><g fill="none" stroke="#000" '
         f'stroke-width="{width}" stroke-linejoin="round" stroke-linecap="round">'
         + SHAPES[category] + '</g></svg>\n')

def curated_svg(curated, unesco_id, variant):
 """Return an approved local SVG override, or None. No network SVG ingestion."""
 if curated is None:
  return None
 suffix = '--simplified' if variant == 'simple' else ''
 path = curated / f'unesco-{unesco_id}{suffix}.svg'
 if not path.is_file():
  return None
 data = path.read_text(encoding='utf-8')
 # These are local curator-supplied assets, not arbitrary downloaded SVG.
 # Reject executable content and references; the application must still sanitize on import.
 import xml.etree.ElementTree as ET
 root = ET.fromstring(data)
 if root.tag.rsplit('}',1)[-1] != 'svg':
  raise ValueError(f'Invalid SVG root: {path}')
 for element in root.iter():
  tag = element.tag.rsplit('}',1)[-1].lower()
  if tag in ('script','foreignobject','image','use','iframe','animate','set'):
   raise ValueError(f'Unsupported SVG element {tag}: {path}')
  for key, value in element.attrib.items():
   attr = key.rsplit('}',1)[-1].lower()
   if attr.startswith('on') or attr in ('href','style') or 'url(' in value.lower():
    raise ValueError(f'Unsupported SVG attribute {key}: {path}')
 return data

def export(records, output, source_hash, curated=None):
 output.mkdir(parents=True,exist_ok=True)
 (output/'svg').mkdir(exist_ok=True)
 with (output/'catalog.csv').open('w',encoding='utf-8',newline='') as f:
  w=csv.DictWriter(f,fieldnames=FIELDS);w.writeheader();w.writerows(records)
 (output/'catalog.json').write_text(json.dumps(records,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
 curated_count = 0
 for record in records:
  original = curated_svg(curated, record['unesco_id'], 'standard')
  simple = curated_svg(curated, record['unesco_id'], 'simple')
  if original is not None:
   curated_count += 1
   record['artwork_status'] = 'curated-site-specific' if simple is not None else 'curated-standard-only'
  (output/record['svg_file']).write_text(original if original is not None else svg(record['category']),encoding='utf-8')
  (output/record['simple_svg_file']).write_text(simple if simple is not None else svg(record['category'],True),encoding='utf-8')
 # Rewrite catalogs after artwork status has been resolved.
 with (output/'catalog.csv').open('w',encoding='utf-8',newline='') as f:
  w=csv.DictWriter(f,fieldnames=FIELDS);w.writeheader();w.writerows(records)
 (output/'catalog.json').write_text(json.dumps(records,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
 counts=dict(collections.Counter(r['category'] for r in records))
 (output/'coverage.json').write_text(json.dumps(dict(source=SOURCE,source_sha256=source_hash,
   unique_properties=len(records),svg_files=2*len(records),by_category=counts,
   curated_properties=curated_count,placeholder_properties=len(records)-curated_count,
   artwork_status='Only explicitly curated artwork is site-specific; remaining artwork is category-level placeholder',
   note='Transboundary properties counted once by UNESCO ID; no coordinates exported'),indent=2)+'\n',encoding='utf-8')

def main():
 p=argparse.ArgumentParser(description=__doc__)
 p.add_argument('--xml',type=Path,help='Use locally saved official UNESCO XML (offline/reproducible)')
 p.add_argument('--output',type=Path,default=Path('assets/unesco_whc'))
 p.add_argument('--minimum',type=int,default=1000,help='Minimum number of records before exporting')
 p.add_argument('--curated',type=Path,help='Directory of manually curated unesco-ID.svg and unesco-ID--simplified.svg assets')
 args=p.parse_args()
 if args.xml:
  data=args.xml.read_bytes()
 else:
  request=urllib.request.Request(SOURCE,headers={'User-Agent':'TerrainCraft-UNESCO-Sync/1.0 (contact: project-maintainer)'})
  with urllib.request.urlopen(request,timeout=60) as response:data=response.read()
 records=parse(data)
 if len(records)<args.minimum:
  raise SystemExit(f'Incomplete UNESCO extraction ({len(records)} < {args.minimum}); no output written')
 export(records,args.output,hashlib.sha256(data).hexdigest(),args.curated)
 print(f'Generated {len(records)} UNESCO records and {2*len(records)} SVG placeholders in {args.output}')

if __name__=='__main__':main()
