"""Draw a building's OpenStreetMap footprint as a 24 x 24 line icon.

The outline of the given way or multipolygon relation is drawn north-up, with the
outlines of its building:part ways (domes, towers, wings) inside it when --parts is set.
The result is a plan view in the stroke style of the bundled icon library.

OpenStreetMap data is (c) OpenStreetMap contributors and available under the ODbL;
icons made from it must credit "(c) OpenStreetMap contributors".

Usage:
    python tools/osm_footprint_icon.py way 40790652 --parts -o capitol.svg
"""
import argparse
import json
import math
import urllib.request

API = 'https://api.openstreetmap.org/api/0.6'
USER_AGENT = 'TerrainCraft-footprint-icon/0.1 (+https://github.com/LagunaCreek-Makers/TerrainCraft)'
LEFT, RIGHT, TOP, BOTTOM = 2.0, 22.0, 2.0, 22.0


def fetch(url):
    req = urllib.request.Request(url, headers={'User-Agent': USER_AGENT})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def index(elements):
    nodes = {e['id']: (e['lon'], e['lat']) for e in elements if e['type'] == 'node'}
    ways = {e['id']: e for e in elements if e['type'] == 'way'}
    rels = {e['id']: e for e in elements if e['type'] == 'relation'}
    return nodes, ways, rels


def join_rings(chains):
    """Join open node chains that share end nodes into closed rings."""
    chains = [list(c) for c in chains if len(c) >= 2]
    rings = []
    while chains:
        ring = chains.pop(0)
        changed = True
        while ring[0] != ring[-1] and changed:
            changed = False
            for i, c in enumerate(chains):
                if c[0] == ring[-1]:
                    ring += c[1:]
                elif c[-1] == ring[-1]:
                    ring += c[-2::-1]
                elif c[-1] == ring[0]:
                    ring = c[:-1] + ring
                elif c[0] == ring[0]:
                    ring = c[:0:-1] + ring
                else:
                    continue
                chains.pop(i)
                changed = True
                break
        if ring[0] == ring[-1] and len(ring) >= 4:
            rings.append(ring)
    return rings


def outline_rings(kind, ident, elements):
    """Node-id rings of the target's outline (outer and inner)."""
    nodes, ways, rels = index(elements)
    if kind == 'way':
        return [ways[ident]['nodes']], nodes
    members = rels[ident]['members']
    chains = [ways[m['ref']]['nodes'] for m in members if m['type'] == 'way' and m['ref'] in ways]
    return join_rings(chains), nodes


def point_in(p, ring):
    x, y = p
    inside = False
    for (x1, y1), (x2, y2) in zip(ring, ring[1:]):
        if (y1 > y) != (y2 > y) and x < x1 + (y - y1) * (x2 - x1) / (y2 - y1):
            inside = not inside
    return inside


def project(rings_lonlat):
    """Local meters, x east and y south, around the rings' center."""
    pts = [p for r in rings_lonlat for p in r]
    lon0 = sum(p[0] for p in pts) / len(pts)
    lat0 = sum(p[1] for p in pts) / len(pts)
    k = math.cos(math.radians(lat0))
    return [[((lon - lon0) * 111320 * k, (lat0 - lat) * 110540) for lon, lat in r] for r in rings_lonlat]


def to_icon(rings):
    """Scale rings uniformly into the icon box, centered."""
    pts = [p for r in rings for p in r]
    x0, x1 = min(p[0] for p in pts), max(p[0] for p in pts)
    y0, y1 = min(p[1] for p in pts), max(p[1] for p in pts)
    s = min((RIGHT - LEFT) / max(x1 - x0, 1e-9), (BOTTOM - TOP) / max(y1 - y0, 1e-9))
    ox = LEFT + ((RIGHT - LEFT) - (x1 - x0) * s) / 2
    oy = TOP + ((BOTTOM - TOP) - (y1 - y0) * s) / 2
    return [[(ox + (x - x0) * s, oy + (y - y0) * s) for x, y in r] for r in rings]


def drop_short_edges(ring, tol=0.15):
    """Remove vertices closer than `tol` icon units to the previous kept vertex."""
    out = [ring[0]]
    for p in ring[1:-1]:
        if math.dist(p, out[-1]) >= tol:
            out.append(p)
    out.append(ring[-1])
    return out if len(out) >= 4 else ring


def svg(rings):
    paths = []
    for r in rings:
        r = drop_short_edges(r)
        d = ' '.join(f'{"M" if i == 0 else "L"}{x:.2f} {y:.2f}' for i, (x, y) in enumerate(r[:-1]))
        paths.append(f'  <path d="{d} Z" />')
    return ('<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" '
            'fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" '
            'stroke-linejoin="round">\n' + '\n'.join(paths) + '\n</svg>\n')


def footprint(kind, ident, full, area=None, parts=False, min_part=0.002):
    """Icon SVG from the target's full elements and, for parts, the surrounding map data."""
    rings, nodes = outline_rings(kind, ident, full['elements'])
    outline = [[nodes[n] for n in r] for r in rings]
    drawn = list(outline)
    if parts and area is not None:
        a_nodes, a_ways, _ = index(area['elements'])
        outer = outline[0]
        out_area = abs(shoelace(outer))
        for w in a_ways.values():
            t = w.get('tags', {})
            if 'building:part' not in t or w['nodes'][0] != w['nodes'][-1] or w['id'] == ident:
                continue
            ring = [a_nodes[n] for n in w['nodes'] if n in a_nodes]
            if len(ring) < 4:
                continue
            c = (sum(p[0] for p in ring[:-1]) / (len(ring) - 1), sum(p[1] for p in ring[:-1]) / (len(ring) - 1))
            if point_in(c, outer) and abs(shoelace(ring)) >= min_part * out_area:
                drawn.append(ring)
    return svg(to_icon(project(drawn)))


def shoelace(ring):
    return sum(x1 * y2 - x2 * y1 for (x1, y1), (x2, y2) in zip(ring, ring[1:])) / 2


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument('kind', choices=['way', 'relation'])
    p.add_argument('id', type=int)
    p.add_argument('--parts', action='store_true', help='also draw building:part outlines inside')
    p.add_argument('-o', '--output', required=True)
    a = p.parse_args()
    full = fetch(f'{API}/{a.kind}/{a.id}/full.json')
    area = None
    if a.parts:
        lons = [e['lon'] for e in full['elements'] if e['type'] == 'node']
        lats = [e['lat'] for e in full['elements'] if e['type'] == 'node']
        pad = 0.0002
        bbox = f'{min(lons) - pad},{min(lats) - pad},{max(lons) + pad},{max(lats) + pad}'
        area = fetch(f'{API}/map.json?bbox={bbox}')
    with open(a.output, 'w', encoding='utf-8', newline='\n') as f:
        f.write(footprint(a.kind, a.id, full, area, a.parts))
    print(f'Wrote {a.output}')


if __name__ == '__main__':
    main()
