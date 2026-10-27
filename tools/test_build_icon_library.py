import json, tempfile, unittest
from pathlib import Path
from build_icon_library import build

BOX = '<path stroke="none" d="M0 0h24v24H0z" fill="none"/>'


def svg(body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" class="icon" viewBox="0 0 24 24">{body}</svg>'


class IconLibraryTest(unittest.TestCase):
    def test_subset_licenses_and_rust_table(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            tabler, maki = d / 'tabler', d / 'maki'
            (tabler / 'icons' / 'outline').mkdir(parents=True)
            (maki / 'icons').mkdir(parents=True)
            (tabler / 'package.json').write_text('{"version": "1.0.0"}')
            (maki / 'package.json').write_text('{"version": "2.0.0"}')
            (tabler / 'LICENSE').write_text('MIT License')
            (maki / 'LICENSE.txt').write_text('CC0 1.0 Universal')
            meta = {
                'mountain': {'category': 'Map', 'tags': ['peak', 2], 'styles': {'outline': {}}},
                'mountain-off': {'category': 'Map', 'styles': {'outline': {}}},
                'brand-x': {'category': 'Brand', 'styles': {'outline': {}}},
                'pokeball': {'category': 'Map', 'styles': {'outline': {}}},
            }
            (tabler / 'icons.json').write_text(json.dumps(meta))
            for name in meta:
                (tabler / 'icons' / 'outline' / f'{name}.svg').write_text(svg(BOX + '<path d="M3 20h18"/>'))
            (maki / 'icons' / 'volcano.svg').write_text('<?xml version="1.0"?>\n' + svg('<path d="M1 1h2"/>'))
            (maki / 'icons' / 'circle.svg').write_text(svg('<path d="M1 1h2"/>'))

            out, rust = d / 'out', d / 'table.rs'
            icons = build(tabler, maki, out, rust)
            self.assertEqual([i['id'] for i in icons], ['tabler:mountain', 'maki:volcano'])
            self.assertEqual(icons[0]['tags'], ['peak', '2'])
            text = (out / 'tabler' / 'mountain.svg').read_text()
            self.assertNotIn('stroke="none"', text)
            self.assertNotIn('class=', text)
            self.assertNotIn('<?xml', (out / 'maki' / 'volcano.svg').read_text())
            catalog = json.loads((out / 'catalog.json').read_text())
            for s in catalog['sets']:
                self.assertTrue((out / s['license_file']).is_file())
            self.assertEqual({s['license'] for s in catalog['sets']}, {'MIT', 'CC0-1.0'})
            self.assertIn('assets/icon_library/maki/volcano.svg', rust.read_text())


if __name__ == '__main__':
    unittest.main()
