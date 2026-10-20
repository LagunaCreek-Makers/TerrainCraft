import json
import tempfile
import unittest
from pathlib import Path
from audit_icon_coverage import run

class CoverageTests(unittest.TestCase):
    def test_missing_artwork_not_counted(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'assets').mkdir()
            (p/'assets'/'institution_logo_catalog.json').write_text(json.dumps([{'id':'one','name':'Example University','jurisdiction':'US-UT','svg_file':''}]))
            result=run(p/'assets',p/'reports')
            self.assertEqual(result['missing_artwork'],1)
            self.assertEqual(result['entries_with_verified_multiple_perspectives'],0)
    def test_invalid_svg_detected(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'assets').mkdir()
            (p/'assets'/'bad.svg').write_text('<svg>')
            result=run(p/'assets',p/'reports')
            self.assertEqual(result['valid_svg_files'],0)
            self.assertEqual(len(result['invalid_svg_files']),1)

if __name__=='__main__': unittest.main()
