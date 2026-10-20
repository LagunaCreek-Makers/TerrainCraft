#!/usr/bin/env python3
"""Audit actual SVG files, catalog references, and per-site artwork coverage.

Never infer a genuine building perspective from a style treatment or a filename.
"""
import argparse
import collections
import csv
import json
from pathlib import Path
import xml.etree.ElementTree as ET

ASSET = Path(__file__).resolve().parents[1] / 'assets'
STYLES = ('original', 'simplified', 'outline', 'bold')
VIEWS = ('front', 'side', 'corner')

def read_json(path):
    return json.loads(path.read_text(encoding='utf-8')) if path.exists() else []

def svg_valid(path):
    try:
        root = ET.parse(path).getroot()
        return root.tag.rsplit('}', 1)[-1].lower() == 'svg'
    except (ET.ParseError, OSError):
        return False

def catalog_rows(assets):
    for entry in read_json(assets / 'state_cultural_icons.json'):
        yield dict(collection='state-cultural', id=entry['id'], name=entry['title'], jurisdiction=entry['jurisdiction'], artwork=entry.get('svg_file', ''), status='shared-generic')
    for entry in read_json(assets / 'institution_logo_catalog.json'):
        yield dict(collection='institution-team', id=entry['id'], name=entry['name'], jurisdiction=entry.get('jurisdiction',''), artwork=entry.get('svg_file',''), status='licensed-artwork-required' if not entry.get('svg_file') else 'imported')
    for entry in read_json(assets / 'regional_icon_catalog.json'):
        artwork='regional_icons/'+entry['variants']['original']
        yield dict(collection='regional', id=entry['id'], name=entry.get('motif',entry['name']), jurisdiction=entry['jurisdiction'], artwork=artwork, status='regional-thematic')
    for folder, collection in [('unesco_whc','unesco'), ('nps_nhl','nps-nhl')]:
        path=assets/folder/'catalog.csv'
        if path.exists():
            with path.open(encoding='utf-8',newline='') as f:
                for entry in csv.DictReader(f):
                    yield dict(collection=collection,id=entry['id'],name=entry['name'],jurisdiction=entry.get('jurisdiction',entry.get('countries','')),artwork=folder+'/'+entry.get('svg_file',''),status=entry.get('artwork_status','unknown'))

def run(assets, out):
    files=list(assets.rglob('*.svg'))
    invalid=[str(p.relative_to(assets)) for p in files if not svg_valid(p)]
    rows=list(catalog_rows(assets))
    for r in rows:
        rel=r['artwork']
        r['svg_exists']=bool(rel and (assets/rel).is_file())
        r['simplified_exists']=False
        r['authentic_perspectives']=0
        if r['collection']=='regional':
            base=assets/'regional_icons'/r['id']
            r['simplified_exists']=(Path(str(base)+'--simplified.svg')).is_file()
        elif r['collection'] in ('unesco','nps-nhl') and rel:
            r['simplified_exists']=(assets/rel.replace('.svg','--simplified.svg')).is_file()
        elif r['collection']=='state-cultural' and rel:
            stem=rel.replace('/','__').replace('.svg','')
            r['simplified_exists']=(assets/'icon_variants'/f'{stem}__simplified.svg').is_file()
        # Authentic perspectives are only counted when explicit per-site provenance is documented.
    out.mkdir(parents=True,exist_ok=True)
    with (out/'site_coverage.csv').open('w',encoding='utf-8',newline='') as f:
        writer=csv.DictWriter(f,fieldnames=list(rows[0]) if rows else ['collection','id','name','jurisdiction','artwork','status','svg_exists','simplified_exists','authentic_perspectives']);writer.writeheader();writer.writerows(rows)
    summary={'svg_files':len(files),'valid_svg_files':len(files)-len(invalid),'invalid_svg_files':invalid,'svg_by_directory':dict(sorted(collections.Counter(p.parent.relative_to(assets).as_posix() for p in files).items())), 'catalog_entries':len(rows),'catalog_by_collection':dict(sorted(collections.Counter(r['collection'] for r in rows).items())), 'entries_with_svg':sum(r['svg_exists'] for r in rows),'entries_with_simplified':sum(r['simplified_exists'] for r in rows),'entries_with_verified_multiple_perspectives':0,'missing_artwork':sum(not r['svg_exists'] for r in rows),'important':'Generic/shared motifs and algorithmic styles do not count as authentic per-site artwork or perspectives. NPS/UNESCO entries only count if synced catalog CSV exists.'}
    (out/'coverage_summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8')
    return summary

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--assets',type=Path,default=ASSET);p.add_argument('--output',type=Path,default=ASSET/'coverage_audit');a=p.parse_args()
    print(json.dumps(run(a.assets,a.output),indent=2))
