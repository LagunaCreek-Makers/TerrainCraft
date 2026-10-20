#!/usr/bin/env python3
"""Generate original, stylized (NOT survey-accurate) UNESCO landmark engravings.

The view variants are independently drawn schematic compositions, not geometric
rotations or verified elevations. No external images or trademarks are used.
"""
import argparse, json
from pathlib import Path
from xml.etree import ElementTree as ET

NS='http://www.w3.org/2000/svg'; TC='https://terraincraft.org/ns/asset'
ET.register_namespace('',NS); ET.register_namespace('tc',TC)
# Distinctive stylized outlines, with different features visible by viewpoint.
SITES={
 '252':('Taj Mahal','India', {
  'front':'M9 79H91 M15 74V42H85V74 M20 42Q50 7 80 42 M30 42Q50 18 70 42 M40 74V55Q50 42 60 55V74 M12 74V24H20V74 M80 74V24H88V74 M10 24H22 M78 24H90 M50 19V8',
  'side':'M8 79H92 M17 74V44H78V74 M26 44Q46 11 68 44 M38 74V55Q46 44 54 55V74 M75 74V23H85V74 M73 23H87 M15 74V33H22V74 M45 21V9',
  'three-quarter':'M7 79H93 M16 70L45 78L83 66V39L45 50L16 40Z M16 40L43 15L83 39 M45 50V78 M30 73V55Q37 47 43 55V76 M59 72V55Q68 45 75 51V68 M13 69V27 M83 65V23 M43 15V6',
  'simplified':'M9 79H91 M17 74V44H83V74 M22 44Q50 7 78 44 M39 74V55Q50 41 61 55V74 M12 74V22H20V74 M80 74V22H88V74'}),
 '438':('Great Wall','China',{
  'front':'M5 79L19 65L31 69L44 47L60 55L74 31L94 40 M5 89L20 75L32 79L46 57L61 65L76 41L94 50 M17 66V53H29V70 M40 52V37H53V54 M70 35V20H84V39 M18 53H29 M41 37H53 M70 20H84',
  'side':'M5 76Q24 69 39 58T66 40T95 32 M5 87Q24 80 39 69T66 51T95 43 M23 70V50H36V62 M58 49V28H72V42 M24 50H36 M58 28H72',
  'three-quarter':'M5 82L24 67L41 72L58 48L79 51L95 31 M5 92L24 77L41 82L58 58L79 61L95 41 M24 67V46L36 49V72 M24 46L36 39L47 43L36 49 M74 51V27L86 30V50 M74 27L84 20L95 24L86 30',
  'simplified':'M4 82L20 66L38 72L55 48L72 54L94 30 M4 91L20 75L38 81L55 57L72 63L94 39 M16 69V52H28V69 M51 51V35H63V51 M75 51V23H87V45'}),
 '394':('Acropolis, Athens','Greece',{
  'front':'M6 82H94 M11 76H89 M19 29H81 M14 36H86 M19 69H81 M22 69V36 M34 69V36 M46 69V36 M58 69V36 M70 69V36 M78 69V36 M17 29L50 17L83 29 M17 73H83',
  'side':'M6 83H94 M12 73H88 M20 32H83 M16 39H87 M22 70H81 M24 70V39 M40 70V39 M56 70V39 M72 70V39 M18 32L65 19L86 32',
  'three-quarter':'M8 77L41 85L92 68 M17 33L48 40L85 27 M17 40L48 47L85 34 M17 67L48 75L85 61 M17 40V67 M29 43V70 M41 46V73 M48 47V75 M60 43V70 M73 39V65 M85 34V61 M17 33L52 16L85 27 M48 40L52 16',
  'simplified':'M8 81H92 M16 73H84 M17 35H83 M18 29L50 16L82 29 M22 69V36 M36 69V36 M50 69V36 M64 69V36 M78 69V36 M16 69H84'}),
 '320':('Petra','Jordan',{
  'front':'M6 87L16 20L27 31L36 15L51 27L66 12L82 26L94 87 M24 75H76 M29 70V42H71V70 M32 42L50 30L68 42 M39 70V49 M61 70V49 M45 70V56Q50 49 55 56V70 M27 77H73',
  'side':'M7 87L18 25L34 33L47 16L62 29L77 18L93 87 M27 75V44H70V75 M32 44L48 31L70 44 M39 75V51 M58 75V51 M43 75V60Q49 51 54 60V75',
  'three-quarter':'M7 87L20 22L37 30L55 13L75 24L94 87 M23 71L54 79L77 69V41L54 49L23 40Z M23 40L46 29L77 41 M54 49V79 M31 73V50 M42 76V53 M63 75V55 M70 72V52',
  'simplified':'M6 87L17 21L34 29L52 14L70 27L84 19L94 87 M25 75V43H75V75 M25 43L50 29L75 43 M43 75V57Q50 46 57 57V75'}),
 '274':('Historic Sanctuary of Machu Picchu','Peru',{
  'front':'M3 81L25 30L39 53L55 19L76 48L89 33L97 81 M12 83H89 M19 72H73 M25 72V61H42V72 M42 72V56H61V72 M61 72V64H78V72 M30 61L34 54L42 61 M42 56L51 48L61 56',
  'side':'M3 81L20 36L36 56L58 20L82 53L97 81 M14 84H90 M20 75H84 M27 75V64H51V75 M51 75V58H73V75 M31 64L39 56L51 64',
  'three-quarter':'M3 82L24 30L43 54L59 17L79 49L95 82 M16 76L47 84L83 72 M21 67L48 75L75 65 M21 67L36 55L63 62L48 75 M36 55V70 M48 75V84 M63 62V78 M30 70V61 M57 77V68',
  'simplified':'M3 84L24 29L41 52L58 18L79 48L97 84 M16 80H86 M25 75V63H47V75 M47 75V57H69V75 M31 63L39 55L47 63'}),
 '373':('Stonehenge, Avebury and Associated Sites','United Kingdom',{
  'front':'M6 82H94 M16 76V36H29V76 M38 76V29H51V76 M61 76V35H74V76 M14 34H53V24H14Z M59 33H78V24H59Z M82 76V48H89V76',
  'side':'M6 82H94 M18 76V35H31V76 M33 76V32H47V76 M15 34H50V24H15Z M60 76V45H72V76 M74 76V43H86V76 M58 43H88V35H58Z',
  'three-quarter':'M6 82H94 M18 71L29 76V36L18 32Z M29 36L39 31V72L29 76 M18 32L29 36L39 31L27 27Z M43 73L55 77V39L43 35Z M55 39L66 34V73L55 77 M43 35L55 39L66 34L54 30Z M20 27L53 34L66 27L32 20Z',
  'simplified':'M6 82H94 M16 76V36H29V76 M38 76V36H51V76 M60 76V36H73V76 M14 35H53V24H14Z M58 35H76V24H58Z'})
}

def build(out):
 out.mkdir(parents=True,exist_ok=True)
 catalog=[]
 for uid,(name,country,views) in SITES.items():
  record={'id':'unesco-'+uid,'unesco_id':uid,'name':name,'jurisdiction':country,'source_url':'https://whc.unesco.org/en/list/'+uid+'/','artwork_status':'original-stylized-site-specific-unverified','perspective_verified':False,'variants':{}}
  for view,path in views.items():
   filename=f'unesco-{uid}'+('--simplified' if view=='simplified' else '' if view=='front' else '--'+view)+'.svg'
   root=ET.Element('{'+NS+'}svg',{'viewBox':'0 0 100 100','width':'100','height':'100'})
   ET.SubElement(root,'{'+NS+'}title').text=name+' — '+view+' stylized engraving'
   ET.SubElement(root,'{'+NS+'}desc').text='Original schematic illustration, not a measured or verified architectural elevation.'
   metadata=ET.SubElement(root,'{'+NS+'}metadata')
   ET.SubElement(metadata,'{'+TC+'}asset').text=json.dumps({'schema_version':1,'site_id':'unesco-'+uid,'site_name':name,'jurisdiction':country,'source_url':record['source_url'],'view':view,'style':'simplified' if view=='simplified' else 'line-engraving','artwork_status':record['artwork_status'],'perspective_verified':False,'license':'Original TerrainCraft procedural drawing'},ensure_ascii=False)
   ET.SubElement(root,'{'+NS+'}path',{'d':path,'fill':'none','stroke':'#000','stroke-width':'4' if view=='simplified' else '2.4','stroke-linejoin':'round','stroke-linecap':'round'})
   ET.ElementTree(root).write(out/filename,encoding='utf-8',xml_declaration=True)
   record['variants'][view]=filename
  catalog.append(record)
 (out/'curated_catalog.json').write_text(json.dumps(catalog,indent=2,ensure_ascii=False)+'\n',encoding='utf-8')
 return catalog

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,default=Path('assets/unesco_curated'));a=p.parse_args()
 print('Created',sum(len(r['variants']) for r in build(a.output)),'SVGs')
