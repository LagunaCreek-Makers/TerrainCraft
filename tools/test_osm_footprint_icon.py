import unittest
from osm_footprint_icon import footprint, join_rings


def node(i, lon, lat):
    return {'type': 'node', 'id': i, 'lon': lon, 'lat': lat}


class FootprintTest(unittest.TestCase):
    def test_join_rings_from_split_ways(self):
        rings = join_rings([[1, 2, 3], [5, 4, 3], [5, 6, 1]])
        self.assertEqual(len(rings), 1)
        self.assertEqual(rings[0][0], rings[0][-1])
        self.assertEqual(sorted(set(rings[0])), [1, 2, 3, 4, 5, 6])

    def test_outline_and_parts_fit_the_icon(self):
        # A 2:1 building with a small dome part inside it and one outside it.
        full = {'elements': [
            node(1, 0.0, 0.0), node(2, 0.002, 0.0), node(3, 0.002, 0.001), node(4, 0.0, 0.001),
            {'type': 'way', 'id': 10, 'nodes': [1, 2, 3, 4, 1], 'tags': {'building': 'yes'}},
        ]}
        area = {'elements': full['elements'] + [
            node(5, 0.0009, 0.0004), node(6, 0.0011, 0.0004), node(7, 0.0011, 0.0006),
            node(8, 0.0009, 0.0006),
            {'type': 'way', 'id': 11, 'nodes': [5, 6, 7, 8, 5], 'tags': {'building:part': 'yes'}},
            node(9, 0.01, 0.01), node(12, 0.011, 0.01), node(13, 0.011, 0.011),
            {'type': 'way', 'id': 14, 'nodes': [9, 12, 13, 9], 'tags': {'building:part': 'yes'}},
        ]}
        svg = footprint('way', 10, full, area, parts=True)
        self.assertEqual(svg.count('<path'), 2)
        nums = [float(t) for t in svg.replace('M', ' ').replace('L', ' ').replace('Z', ' ')
                .split('d="', 1)[1].split('"')[0].split()]
        self.assertTrue(all(2 - 1e-9 <= v <= 22 + 1e-9 for v in nums))


if __name__ == '__main__':
    unittest.main()
