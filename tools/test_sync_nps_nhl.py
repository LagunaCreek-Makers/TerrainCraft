import tempfile,unittest
from pathlib import Path
from sync_nps_nhl import parse,export
class TestNps(unittest.TestCase):
 def test_parse_and_export(self):
  html='''<h4>Alabama (2)</h4><p><a href="https://example.org/1">Edmund Pettus Bridge</a> - 02/27/13</p><p>Selma, AL</p><p>Secret Archeological Site (restricted) - 01/20/61</p><h4>Utah (1)</h4><p>Temple Square - 01/01/70</p>'''
  r=parse(html);self.assertEqual(len(r),3)
  self.assertEqual([x for x in r if x['restricted']=='true'][0]['category'],'archaeology')
  with tempfile.TemporaryDirectory() as t:
   export(r,Path(t));self.assertEqual(len(list((Path(t)/'svg').glob('*.svg'))),6)
   self.assertNotIn('Selma', (Path(t)/'catalog.csv').read_text())
if __name__=='__main__':unittest.main()
