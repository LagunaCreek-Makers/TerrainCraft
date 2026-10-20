import tempfile, unittest, json
from pathlib import Path
from xml.etree import ElementTree as ET
from build_curated_unesco import build

class CuratedTest(unittest.TestCase):
 def test_site_views_and_metadata(self):
  with tempfile.TemporaryDirectory() as d:
   out=Path(d); rows=build(out)
   self.assertEqual(len(rows),6)
   self.assertEqual(len(list(out.glob('*.svg'))),24)
   for r in rows:
    self.assertEqual(set(r['variants']),{'front','side','three-quarter','simplified'})
    self.assertFalse(r['perspective_verified'])
    for view,f in r['variants'].items():
     root=ET.parse(out/f).getroot()
     self.assertTrue(root.tag.endswith('svg'))
     asset=next(x for x in root.iter() if x.tag.endswith('asset'))
     self.assertEqual(json.loads(asset.text)['view'],view)
    paths=[next(x.attrib['d'] for x in ET.parse(out/f).getroot().iter() if x.tag.endswith('path')) for f in r['variants'].values()]
    self.assertEqual(len(set(paths)),4)
if __name__=='__main__':unittest.main()
