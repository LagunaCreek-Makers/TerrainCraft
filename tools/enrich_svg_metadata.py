#!/usr/bin/env python3
"""Embed searchable, truthful provenance metadata in every SVG; create non-logo fallbacks.

Fallbacks are explicitly generic and never claim to depict an institution's marks.
"""
import argparse, collections, json, re
from pathlib import Path
from xml.etree import ElementTree as ET

NS='http://www.w3.org/2000/svg'; TC='https://terraincraft.org/ns/asset'
ET.register_namespace('',NS);ET.register_namespace('tc',TC)
ROOT=Path(__file__).resolve().parents[1]/'assets'

def load(path):
    return json.loads(path.read_text(encoding='utf8')) if path.exists() else []

def build_index(assets):
    index=collections.defaultdict(list)
    for source,collection,key in [('icon_manifest.json','generic','svg_file'),('state_cultural_icons.json','state-cultural','svg_file')]:
        for r in load(assets/source):
            if r.get(key):index[r[key]].append({'id':r.get('id',''),'name':r.get('title',''),'jurisdiction':r.get('jurisdiction',''),'collection':collection,'artwork_status':'shared-generic' if collection=='state-cultural' else 'generic-original','source':r.get('source',''),'license':r.get('license','')})
    for r in load(assets/'institution_logo_catalog.json'):
        if r.get('svg_file'):index[r['svg_file']].append({'id':r['id'],'name':r['name'],'jurisdiction':r.get('jurisdiction',''),'collection':'institution-or-site','artwork_status':'generic-placeholder' if r.get('category') in ('cathedral','ski-resort','lds-temple') else 'unverified','source':r.get('source_url',''),'license':r.get('license','')})
    for r in load(assets/'regional_icon_catalog.json'):
        for style,filename in r.get('variants',{}).items():
            index['regional_icons/'+filename].append({'id':r['id'],'name':r.get('motif',r.get('name','')),'jurisdiction':r.get('jurisdiction',''),'collection':'regional','artwork_status':'regional-thematic-'+style,'source':'TerrainCraft regional illustrations','license':'Original procedural illustration'})
    for r in load(assets/'unesco_curated'/'curated_catalog.json'):
        for view, filename in r.get('variants', {}).items():
            index['unesco_curated/'+filename].append({'id':r['id'],'name':r['name'],'jurisdiction':r['jurisdiction'],'collection':'unesco-curated','artwork_status':r['artwork_status'],'perspective':view,'perspective_verified':False,'source':r['source_url'],'license':'Original TerrainCraft procedural drawing'})
    for r in load(assets/'icon_variants_manifest.json'):
        for entry in index.get(r['base'],[]):
            index[r['file']].append({**entry,'artwork_status':entry['artwork_status']+'-'+r['style'],'derived_from':r['base']})
    for folder,coll in [('unesco_whc','unesco'),('nps_nhl','nps')]:
        import csv
        path=assets/folder/'catalog.csv'
        if path.exists():
            with path.open(encoding='utf8',newline='') as f:
                for r in csv.DictReader(f):
                    if r.get('svg_file'):
                        index[folder+'/'+r['svg_file']].append({'id':r.get('id',''),'name':r.get('name',''),'jurisdiction':r.get('jurisdiction',r.get('countries','')),'collection':coll,'artwork_status':r.get('artwork_status','unverified'),'source':r.get('source_url',''),'license':r.get('license','')})
    return index

def enrich(path,assets,index):
    relative=path.relative_to(assets).as_posix(); tree=ET.parse(path);root=tree.getroot()
    if root.tag != '{'+NS+'}svg':raise ValueError('Not SVG: '+relative)
    for old in list(root):
        if old.tag in ('{'+NS+'}metadata','{'+NS+'}title','{'+NS+'}desc') and old.get('data-terraincraft')=='true':root.remove(old)
    entries=index.get(relative,[])
    # Filename-derived metadata is not a claim of landmark authenticity.
    category=relative.split('/')[0]
    info={'schema_version':1,'asset_path':relative,'category':category,'associated_sites':entries,'associated_site_count':len(entries),'artwork_represents':'generic-symbol' if not entries else ('regional-theme' if category=='regional_icons' else 'see-associated-site-artwork-status'),'perspective_verified':False,'single_layer_engraving':True}
    title=ET.Element('{'+NS+'}title',{'data-terraincraft':'true'});title.text=entries[0]['name'] if len(entries)==1 else path.stem.replace('_',' ').replace('-',' ')
    desc=ET.Element('{'+NS+'}desc',{'data-terraincraft':'true'});desc.text=('Generic engraving artwork; associations are catalog metadata, not a claim of architectural accuracy.' if category!='regional_icons' else 'Regional thematic artwork; not a verified view of a specific site.')
    meta=ET.Element('{'+NS+'}metadata',{'data-terraincraft':'true'}); payload=ET.SubElement(meta,'{'+TC+'}asset');payload.text=json.dumps(info,ensure_ascii=False,sort_keys=True)
    root.insert(0,meta);root.insert(0,desc);root.insert(0,title)
    tree.write(path,encoding='unicode',xml_declaration=True)
    return info

def generate_missing(assets):
    catalog=assets/'institution_logo_catalog.json';data=load(catalog);target=assets/'unlicensed_placeholders';target.mkdir(exist_ok=True)
    made=0
    for row in data:
        if row.get('svg_file'):continue
        # Do not fill svg_file: that would falsely imply an official logo exists.
        ident=re.sub(r'[^a-z0-9-]','-',row['id'].lower()).strip('-')
        filename='unlicensed_placeholders/'+ident+'.svg'; path=assets/filename
        label='U' if row.get('category') in ('university','college') else 'S'
        if not path.exists():
            path.write_text(f'<svg xmlns="{NS}" viewBox="0 0 100 100"><rect x="8" y="8" width="84" height="84" rx="12" fill="white" stroke="black" stroke-width="5"/><path d="M25 72 H75 M25 78 H75" fill="none" stroke="black" stroke-width="3"/><text x="50" y="61" font-size="43" font-family="sans-serif" font-weight="bold" text-anchor="middle" fill="black">{label}</text></svg>',encoding='utf8');made+=1
        row['preview_svg_file']=filename
        row['preview_artwork_status']='generic-unlicensed-placeholder-not-official-logo'
    catalog.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
    return made

def run(assets):
    created=generate_missing(assets);index=build_index(assets)
    for row in load(assets/'institution_logo_catalog.json'):
        if row.get('preview_svg_file'):
            index[row['preview_svg_file']].append({'id':row['id'],'name':row['name'],'jurisdiction':row.get('jurisdiction',''),'collection':'institution','artwork_status':'generic-unlicensed-placeholder-not-official-logo','source':'TerrainCraft original fallback','license':'Original generic illustration'})
    report=[]
    for path in sorted(assets.rglob('*.svg')):report.append(enrich(path,assets,index))
    counts=collections.Counter('site-linked' if r['associated_site_count'] else 'unlinked-generic' for r in report)
    summary={'svg_total':len(report),'new_fallbacks':created,'with_embedded_metadata':len(report),'site_linked_svg_files':counts['site-linked'],'unlinked_generic_svg_files':counts['unlinked-generic'],'catalog_missing_official_logos':sum(not x.get('svg_file') for x in load(assets/'institution_logo_catalog.json')),'warning':'Generic fallback icons are not site-specific depictions or official logos. UNESCO/NPS live catalogs require separate synchronization.'}
    (assets/'svg_metadata_summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf8')
    return summary

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--assets',type=Path,default=ROOT);a=p.parse_args();print(json.dumps(run(a.assets),indent=2))
