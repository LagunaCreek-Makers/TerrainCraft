import json, tempfile, unittest
from pathlib import Path
from xml.etree import ElementTree as ET
from enrich_svg_metadata import run

class TestSvgMetadata(unittest.TestCase):
    def test_metadata_all_svg_and_idempotent(self):
        with tempfile.TemporaryDirectory() as td:
            a=Path(td);(a/'icons').mkdir();(a/'icons'/'one.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><path d="M0 0L10 10"/></svg>')
            (a/'icon_manifest.json').write_text(json.dumps([{'id':'x','title':'Example','svg_file':'icons/one.svg','jurisdiction':'US-UT'}]))
            (a/'institution_logo_catalog.json').write_text('[]')
            first=run(a);second=run(a)
            self.assertEqual(first['svg_total'],1);self.assertEqual(second['svg_total'],1)
            root=ET.parse(a/'icons'/'one.svg').getroot()
            self.assertEqual(len(root.findall('{http://www.w3.org/2000/svg}metadata')),1)
            info=json.loads(root.find('.//{https://terraincraft.org/ns/asset}asset').text)
            self.assertEqual(info['associated_sites'][0]['jurisdiction'],'US-UT')
    def test_missing_logo_has_generic_preview_not_official(self):
        with tempfile.TemporaryDirectory() as td:
            a=Path(td);(a/'institution_logo_catalog.json').write_text(json.dumps([{'id':'team-x','name':'Team X','category':'professional','jurisdiction':'US-CA','svg_file':''}]))
            result=run(a);self.assertEqual(result['new_fallbacks'],1)
            item=json.loads((a/'institution_logo_catalog.json').read_text())[0]
            self.assertEqual(item['svg_file'],'')
            self.assertTrue((a/item['preview_svg_file']).exists())
if __name__=='__main__':unittest.main()
