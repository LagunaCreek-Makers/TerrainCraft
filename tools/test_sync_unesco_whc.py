import tempfile
import unittest
from pathlib import Path
import sync_unesco_whc as u

XML=b'''<query><row><id_number>12</id_number><site>Historic Town</site><category>C</category><states>France</states><date_inscribed>1985</date_inscribed></row><row><id_number>35</id_number><site>Mountain Reserve</site><category>N</category><states>Canada, USA</states><date_inscribed>1994</date_inscribed></row><row><id_number>55</id_number><site>Mixed Landscape</site><category>M</category><states>Japan</states></row></query>'''

class UNESCOTests(unittest.TestCase):
 def test_parse_and_export(self):
  records=u.parse(XML)
  self.assertEqual([r['unesco_id'] for r in records],['12','35','55'])
  self.assertEqual(records[1]['countries'],'Canada, USA')
  with tempfile.TemporaryDirectory() as tmp:
   p=Path(tmp);u.export(records,p,'test')
   self.assertEqual(len(list((p/'svg').glob('*.svg'))),6)
   self.assertIn('unique_properties', (p/'coverage.json').read_text())
   self.assertIn('viewBox', (p/records[0]['svg_file']).read_text())
 def test_curated_artwork_and_fallback(self):
  with tempfile.TemporaryDirectory() as tmp:
   base=Path(tmp);curated=base/'curated';curated.mkdir()
   (curated/'unesco-12.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0L1 1"/></svg>')
   records=u.parse(XML);u.export(records,base/'output','test',curated)
   self.assertEqual(records[0]['artwork_status'],'curated-standard-only')
   self.assertEqual(records[1]['artwork_status'],'generic-category-placeholder')
   report=__import__('json').loads((base/'output/coverage.json').read_text())
   self.assertEqual(report['curated_properties'],1)
   self.assertEqual(report['placeholder_properties'],2)
   self.assertIn('M0 0L1 1',(base/'output'/records[0]['svg_file']).read_text())
 def test_unsafe_curated_svg_rejected(self):
  with tempfile.TemporaryDirectory() as tmp:
   curated=Path(tmp)
   (curated/'unesco-12.svg').write_text('<svg><script>bad()</script></svg>')
   with self.assertRaisesRegex(ValueError,'Unsupported SVG element'):
    u.curated_svg(curated,'12','standard')
 def test_duplicates_rejected(self):
  with self.assertRaisesRegex(ValueError,'Duplicate'):
   u.parse(XML.replace(b'</query>',b'<row><id_number>12</id_number><site>Duplicate</site><category>C</category></row></query>'))
 def test_invalid_category_rejected(self):
  with self.assertRaisesRegex(ValueError,'Unknown UNESCO category'):
   u.parse(XML.replace(b'<category>N</category>',b'<category>Q</category>'))

if __name__=='__main__':unittest.main()
